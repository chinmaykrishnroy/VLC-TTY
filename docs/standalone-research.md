# Standalone runtime packaging research

Research checked on 2026-10-17. This document records packaging recommendations and the evidence behind them; it does not certify that a particular release has passed these gates.

Implementation note: the initial launcher uses a new private temporary directory per invocation instead of a persistent cache. This avoids cache reuse/tampering and concurrent extraction collisions; cached startup is a future performance option. The source-provenance limitations below still apply.

## Decision

Ship one native executable **per operating system and CPU architecture**. Embed a compressed runtime payload, extract it to a private versioned cache on first use, and run the extracted application with its matching LibVLC libraries and plugins. This meets the portable-file requirement without an installer or administrator privileges. DLLs and dynamically loaded plugins still need files on disk; a single download does not imply diskless execution. Keep a runtime override so developers and users can run a compatible modified LibVLC.

Use separate Windows x64/ARM64, Linux x64/ARM64 and macOS Intel/Apple Silicon downloads. PowerShell is a shell, and does not identify the CPU architecture or terminal graphics protocol. Inspect the PE/ELF/Mach-O machine type during packaging and run the resulting artifact on its matching native runner. A Windows ARM64 executable cannot be offered as the universal Windows download.

An installer adds little value to the initial SSH-oriented product. It can become optional later if users want managed PATH integration or enterprise deployment. Portable downloads should remain first class.

## Linux: practical bundling plan

Build and collect the runtime on Ubuntu 24.04 for both native architectures initially. Declare **glibc 2.39 or newer**, and test Ubuntu 24.04 plus the user's Ubuntu 26.04 Pi. Do not promise Alpine/musl support or compatibility with older Ubuntu releases. The documented AppImage portability rule is to build on the oldest intended base and verify that base; packaging cannot make newer glibc symbol requirements disappear. [AppImage best practices](https://docs.appimage.org/reference/best-practices.html?highlight=libc)

Recommended collector algorithm, an engineering proposal:

1. Install `libvlc5` and `vlc-plugin-base` without recommends. Record exact installed binary and source package versions, architecture and repository metadata. The runtime version on Linux comes from Ubuntu, and must not be mislabeled as the 3.0.24 Windows/macOS runtime. The Noble package page identifies its source package separately. [Ubuntu libvlc5](https://packages.ubuntu.com/noble/libvlc5)
2. Seed the collector with `libvlc.so.5`, `libvlccore.so.9`, every `.so` in the installed VLC plugin tree and the application itself. Copy the VLC package's applicable data files as well, including Lua resources when present.
3. Walk the ELF `DT_NEEDED` closure of every seed. `ldd` on trusted distribution packages is a convenient native implementation; fail if any dependency is unresolved. Preserve the requested SONAME filename even when the original path is a symlink. Hash copied files and reject conflicting contents under the same destination name.
4. Place dependencies under `runtime/lib`, plugins under `runtime/plugins`, and VLC resources under `runtime/share`. Exclude the host ELF loader and libraries belonging to glibc, rather than mixing an arbitrary private libc with the host loader. Identify ownership through the package database instead of an incomplete library-name heuristic.
5. Start a **new process** with the extracted library path established before its dynamic loader runs. Set `VLC_PLUGIN_PATH` and the application's explicit LibVLC location to the extracted runtime. Keep paths absolute; retain the caller's working directory so relative media filenames continue to work.
6. Audit libraries opened through `dlopen`, as they are absent from a plain `DT_NEEDED` walk. Audio backends, hardware drivers and external services need explicit coverage or a documented boundary. Silent software decoding is the primary acceptance profile. Do not treat successfully loading `libvlc.so` as successful playback.

The loader searches `LD_LIBRARY_PATH` and ELF runpaths, with different transitive behavior for `DT_RUNPATH` and `DT_RPATH`. `$ORIGIN` supports relocation. A launcher that starts the extracted application with the required environment avoids relying on a late environment update after startup. [Linux dynamic loader manual](https://man7.org/linux/man-pages/man8/ld.so.8.html)

The decisive smoke test runs the **packaged executable** inside a clean Ubuntu container with neither LibVLC nor codec packages installed, no display server, an empty runtime cache, no network and a bundled or mounted test video. Require actual decoded frames and advancing media time. Repeat after moving the executable, with a path containing spaces, and after cache corruption. A CI runner with LibVLC installed can silently conceal a missing bundled dependency.

## Windows

Retain a coherent matching upstream directory: `libvlc.dll`, `libvlccore.dll`, runtime DLL dependencies, plugins and necessary resources. An isolated extracted `vlc-tty.exe` beside the matching runtime DLLs is easier to reason about than an arbitrary global PATH modification. Load the chosen library by absolute path and avoid current-directory DLL discovery. Microsoft documents that dependency resolution has its own search order even when the top-level DLL was selected explicitly. [Microsoft DLL search order](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order)

Require native x64 and ARM64 smoke tests with unrelated installed VLC locations unavailable and `VLC_HOME` unset. Inspect PE machine values (`0x8664` x64; `0xaa64` ARM64) as a separate preflight. Check runtime architecture as well as application architecture. Exercise `--help`, `--version`, `--doctor`, actual silent decoding and an interactive console launch. Windows code signing is a separate trust concern and does not fix a CPU mismatch.

Existing pinned upstream runtime downloads are recorded in `scripts/runtime-manifest.json`:

- [Windows x64 ZIP](https://download.videolan.org/pub/videolan/vlc/3.0.24/win64/vlc-3.0.24-win64.zip)
- [Windows ARM64 ZIP](https://download.videolan.org/vlc/3.0.24/winarm64/vlc-3.0.24-winarm64.zip)

## macOS

Preserve the complete extracted `VLC.app` internal directory layout and symlinks instead of flattening its dylibs. Inspect every non-system dependency with `otool -L`, and inspect `LC_RPATH`. Resolve `@loader_path`, `@executable_path` and `@rpath` in the context of the **actual extracted application**, whose executable path differs from VLC's original launcher. Run native Intel and Apple Silicon smoke tests with `/Applications/VLC.app` unavailable. Apple's loader guidance explains the dependency lookup mechanisms; it does not guarantee that a copied VLC bundle will work under a different launcher. [Apple dynamic library usage](https://developer.apple.com/library/archive/documentation/DeveloperTools/Conceptual/DynamicLibraries/100-Articles/DynamicLibraryUsageGuidelines.html)

The 3.0.24 upstream packaging recipe puts libraries in `Contents/MacOS/lib`, plugins in `Contents/MacOS/plugins`, resources under `Contents/MacOS/share`, and additional frameworks under `Contents/Frameworks`. It preserves symlinks in its ZIP recipe. This was inspected in the checksum-verified source archive below. Avoid applying assumptions from VLC 4 packaging to this version.

- [Apple Silicon DMG](https://download.videolan.org/pub/videolan/vlc/3.0.24/macosx/vlc-3.0.24-arm64.dmg)
- [Intel DMG](https://download.videolan.org/pub/videolan/vlc/3.0.24/macosx/vlc-3.0.24-intel64.dmg)

Unsigned downloads can encounter Gatekeeper independently of runtime correctness. Do not claim signing or notarization without actually configuring and validating them.

## Redistribution and corresponding source

LibVLC and the complete VLC distribution do not have identical license scope. VideoLAN identifies LibVLC's LGPL licensing, explicitly warns that some modules are GPLv2, and requires checking modules before redistribution. Treat the complete plugin bundle as GPL-compatible distribution rather than an MIT-only artifact. The application's existing MIT source grant can remain, but it must not replace third-party license terms. [VideoLAN redistribution FAQ](https://docs.videolan.me/vlc-user/desktop/3.0/en/support/faq/legalconcerns.html)

Include applicable notices and full license texts. Provide source corresponding to the runtime actually shipped, including bundled underlying libraries, patches and build scripts; a VLC tarball alone is insufficient for a bundle containing third-party codecs. VideoLAN's legal page specifically includes the underlying libraries in its source-distribution guidance. [VideoLAN legal information](https://images.videolan.org/legal.html)

For Linux, map **every copied file** to its binary package, then record `${source:Package}` and `${source:Version}`. `dpkg-query -S` finds file ownership and `dpkg-query -W` exposes those source fields. Include package copyright files and resolve referenced common-license texts. [Debian dpkg-query manual](https://manpages.debian.org/bookworm/dpkg/dpkg-query.1.en.html)

Enable matching `deb-src` repositories for every installed package origin. Download the exact source version with `apt-get source --download-only --only-source SOURCE=VERSION`, retaining `.dsc`, all referenced original tarballs, Debian patches and metadata, plus hashes. Fail rather than substituting a different version when an exact source package has disappeared. Ubuntu documents matching source repositories and retrieving specific versions. [Ubuntu source package instructions](https://ubuntu.com/project/docs/contributors/setup/get-the-source-of-a-package/), [Ubuntu source repository setup](https://ubuntu.com/kernel/docs/how-to/source-code/enable-source-repositories/)

For Windows/macOS, the official 3.0.24 source asset was downloaded and its published digest verified:

```text
https://download.videolan.org/pub/videolan/vlc/3.0.24/vlc-3.0.24.tar.xz
SHA256 e7cab503d1d7d5849b89d2cf0e1ee60d0ef6d012407791b644b9cfc0cc225fdf
```

[Official source directory](https://download.videolan.org/pub/videolan/vlc/3.0.24/), [published SHA256](https://download.videolan.org/pub/videolan/vlc/3.0.24/vlc-3.0.24.tar.xz.sha256)

The archive contains the VLC build/packaging scripts, patches and contrib recipes. `contrib/src/main.mak` has `fetch` and `fetch-all` source-download targets and per-package checksum validation. Its current source mirror root is **`https://download.videolan.org/pub/contrib/`**; `/pub/videolan/contrib/` contains older prebuilt archives and is a different location. [Official contrib source directory](https://download.videolan.org/pub/contrib/)

Do not assert that every official prebuilt codec exactly matches an arbitrarily selected contrib recipe snapshot. Record the exact recipe/build identity and included versions, or build the runtime from the collected source. Attach a source companion to the same release with a per-component inventory and hashes. A missing component's corresponding source is a release gate, not something a generic upstream homepage link resolves automatically.

The source archive's `COPYING.LIB` section 6 requires notice, license availability and an applicable source/relinking mechanism. Preserve a compatible external runtime override and document it so an extracted runtime can be replaced without bypassing forced cache repair. `COPYING` section 3 governs GPL binary source provision. These exact license texts, not the executable's single-file format, control redistribution.

## Extraction and release gates

Use a private cache with restrictive permissions, a payload digest, bounded extraction and safe paths. Reject absolute paths, traversal, escaping symlinks and duplicate archive entries. Extract into a unique staging directory, verify the manifest, then atomically publish the cache. Never extract beside an executable in a shared writable directory or trust a cache merely because its directory exists. Concurrency and interrupted extraction require regression coverage.

`--help` and `--version` should avoid decoding initialization. Keep `--doctor` explicit about embedded versus overridden runtime, target architecture and actual library location. Add a license/source information command or documented extraction command. A caller-requested runtime override must remain explicit in diagnostics.

Publish only after native packaged-runtime decoding succeeds on all advertised targets, source/license artifacts are complete, and Linux runs in a clean environment without system VLC. A one-file release can reasonably require base OS facilities, writable extraction space and a compatible terminal; document those requirements instead of claiming universal execution.

## Follow-up: source companion implementation

The official 3.0.24 release root and Windows/macOS directory indexes were checked. They expose the VLC source tarball and runtime binaries/checksums; **no dedicated complete contrib-source bundle or binary-to-contrib build manifest was found there**. Requests to the separate `artifacts.videolan.org/vlc-3.0/` service failed TLS negotiation during this check, so its retained CI artifacts could not be assessed. This is an evidence limitation, not proof such an artifact never existed.

The verified source tar contains **111 `contrib/src/*/SHA512SUMS` files**. A source collector can enumerate these, retain the recipe path, expected digest and archive filename, then attempt `https://download.videolan.org/pub/contrib/RECIPE/FILENAME`. Use the recipe's actual upstream URL as fallback and verify the same digest. Do not silently omit failures. Live HEAD probes succeeded for these exact mirrored archives:

| Recipe | Archive |
| --- | --- |
| ffmpeg | `ffmpeg-8.1.2.tar.xz` |
| zlib | `zlib-1.3.2.tar.xz` |
| gnutls | `gnutls-3.8.13.tar.xz` |
| libxml2 | `libxml2-2.15.3.tar.xz` |
| ass | `libass-0.17.5.tar.xz` |
| vpx | `libvpx-1.16.0.tar.gz` |
| freetype2 | `freetype-2.14.3.tar.xz` |
| lua | `lua-5.1.5.tar.gz` |
| dvdcss | `libdvdcss-1.4.3.tar.bz2` |
| qt | `qt-5.6.3.tar.xz` |
| png | `libpng-1.6.58.tar.xz` |

`dav1d/dav1d-1.5.4.tar.xz` returned 404 at this mirror, demonstrating why a uniform mirror-only collector is insufficient. HEAD success is availability evidence; actual downloads still require digest verification. All these archive identities and expected digests come from the checked 3.0.24 source, rather than guesses about a library's current release.

Some active recipes use a pinned Git revision rather than an archive SHA512SUMS entry:

| Recipe | Pinned revision |
| --- | --- |
| daala | `e248823a04292a8c2f56aa260f5c0b369d41d64e` |
| postproc | `e4982b227779f24f7c54d699d2e247652f69c475` |
| fluidlite | `b0f187b404e393ee0a495b277154d55d7d03cbeb` |
| tremor | `b56ffce0` (abbreviated; resolve and record the full commit) |
| x264 | `e067ab0b530395f90b578f6d05ab0a225e2efdf9` |
| x262 | `bb887aa4c0a4da955524aa220b62998c3b50504e` |

Fetch the actual pinned repository objects, verify the full commit, preserve source contents/submodules as required by the recipe, and record the generated archive digest. Commented-out Git alternatives in FFmpeg, AMF and mingw64 must not be mistaken for their active release-archive recipes.

For a make-based collector, `fetch-all` requests `.sum-*` targets for every recipe, whereas `fetch` selects enabled dependencies. Inspecting the code shows that the Windows bootstrap path does not require a Windows SDK. The Darwin path checks an SDK directory and uses `xcrun`, so invoking a Darwin bootstrap directly on Ubuntu is unsuitable. A Windows-host **source superset** is the simpler source-only route. Candidate commands, requiring an actual dry-run on Ubuntu before adoption:

```sh
# Run in a fresh directory underneath the extracted contrib directory.
../bootstrap --build=x86_64-linux-gnu --host=x86_64-w64-mingw32
make -n fetch-all CC=cc CXX=c++ MINGW_W64_VERSION=14
make fetch-all CC=cc CXX=c++ MINGW_W64_VERSION=14 \
  CONTRIB_VIDEOLAN=https://download.videolan.org/pub/contrib \
  VIDEOLAN=https://download.videolan.org/pub/videolan \
  GNU=https://ftpmirror.gnu.org/gnu
```

Native compilers in this candidate answer configuration probes; the fetch targets should not compile target libraries. This command sequence was **source-inspected, not executed** in the Windows research environment. Historical source endpoints, Git access and individual recipe prerequisites can still fail. Collect logs and fail the source job visibly instead of continuing with a partial source archive.

A checked source superset is a pragmatic source companion, but does not by itself establish the exact build identity of the downloaded Windows/macOS binaries. To close that last evidence gap, capture their dependency versions/configuration, locate matching upstream build metadata, or build the runtime from the collected source with retained logs. Preserve that distinction in release notes and publication gates.
