# Terminal-first release platform research

Verified against primary documentation on 17 October 2026. Recommendations below distinguish build coverage, automated decode coverage and physical-device acceptance.

## Native CI and release matrix

These six architecture/OS combinations have native GitHub-hosted runners. Pin explicit labels rather than depending on `-latest` migrations.

| Platform | Runner label | Rust target | Initial release artifact |
| --- | --- | --- | --- |
| Linux x64 | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` | `.tar.gz` application, system LibVLC dependency |
| Linux ARM64 | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `.tar.gz` application, system LibVLC dependency |
| macOS Intel | `macos-15-intel` | `x86_64-apple-darwin` | `.tar.gz` application, matching VLC.app dependency |
| macOS Apple Silicon | `macos-15` | `aarch64-apple-darwin` | `.tar.gz` application, matching VLC.app dependency |
| Windows x64 | `windows-2025` | `x86_64-pc-windows-msvc` | `.zip` application, matching VLC runtime dependency |
| Windows ARM64 | `windows-11-arm` | `aarch64-pc-windows-msvc` | `.zip` application, matching VLC runtime dependency |

GitHub also lists `ubuntu-26.04` and `ubuntu-26.04-arm`. Add a headless Ubuntu 26.04 ARM64 integration lane to cover the user's OS version, while building Linux release binaries on 24.04 as an older distribution baseline. This baseline choice is a recommendation, not a universal Linux compatibility guarantee. Sources: [GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [runner image inventory](https://github.com/actions/runner-images), [Rust target support](https://doc.rust-lang.org/rustc/platform-support.html).

Run native locked build, unit tests, Clippy and a separate required LibVLC decode smoke on each claimed runtime platform. The smoke must fail if LibVLC is missing and require decoded frames and advancing playback, rather than accepting `Opening`/`Buffering` or silently skipping. Use dummy audio for a headless CI decode test. Re-run the smoke against the extracted release artifact with explicit runtime paths, so a developer installation cannot mask packaging errors. These are recommended project gates.

## Ubuntu 26.04 Server and Pi 5

The Ubuntu Resolute index lists LibVLC 3 packages and ARM64 builds; its examined package version is 3.0.23-1. A practical inline-video runtime installation is:

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends libvlc5 vlc-plugin-base
```

Enable Ubuntu's `universe` component if those packages have no candidate. Let APT resolve matching `libvlccore9` and other shared dependencies; avoid mixing downloaded VLC libraries with distribution plugins. Sources: [Resolute LibVLC package](https://packages.ubuntu.com/km/resolute/libvlc5), [Ubuntu source/binary package split](https://packages.ubuntu.com/source/resolute/vlc).

The base package provides memory video output and audio plugins; it is the relevant package for the callback rendering path. The official Noble ARM64 file list and Resolute ARMHF file list show the memory (`vmem`), dummy and ALSA/Pulse output modules. The Resolute ARM64 file-list endpoint could not be retrieved during this research, so confirm its installed contents with `dpkg -L vlc-plugin-base` and an actual required decode smoke in CI. Sources: [Ubuntu ARM64 base-plugin files](https://packages.ubuntu.com/noble/arm64/vlc-plugin-base/filelist), [Resolute base-plugin files](https://packages.ubuntu.com/resolute/armhf/vlc-plugin-base/filelist).

`vlc-plugin-video-output` is a separate desktop-output package with X11/Wayland/GL dependencies; the Qt interface is also separately packaged. Inline memory callbacks do not require a running desktop compositor. Do not install a desktop environment merely to run this terminal frontend. Sources: [Resolute desktop-output package](https://packages.ubuntu.com/resolute/vlc-plugin-video-output), [package split](https://packages.ubuntu.com/source/resolute/vlc), [LibVLC memory-output API](https://videolan.videolan.me/vlc-3.0/group__libvlc__media__player.html).

An ARM64 cloud runner is not a Raspberry Pi. It cannot establish Pi 5 decoding speed, thermals, driver acceleration, physical audio output, SSH bandwidth or the behavior of the user's terminal. Require an actual Pi 5 + Ubuntu Server 26.04 + SSH playback session before calling that deployment verified. Standard memory callbacks themselves carry CPU copy/conversion costs documented by LibVLC; software-decoding throughput is a measurement, not a consequence of an ARM64 build passing.

## SSH graphics and audio

Allocate a pseudo-terminal when launching the TUI as an SSH command, for example `ssh -t user@pi 'vlc-tty --ssh --silent /path/to/movie.mp4'`. OpenSSH documents `-t` for remote screen-based applications. [OpenSSH manual](https://man.openbsd.org/ssh).

The receiving terminal determines image-protocol support. Kitty documents image rendering over SSH and requires remote clients to transmit pixel data directly within escape sequences; remote filesystem or shared-memory references are not a valid transport to the local terminal. Probe capabilities and fall back to ANSI. Multiplexers and network throughput need their own tests. Sources: [Kitty image display over SSH](https://sw.kovidgoyal.net/kitty/kittens/icat/), [Kitty remote-client graphics transport](https://sw.kovidgoyal.net/kitty/graphics-protocol/#remote-client).

In this application's architecture, LibVLC runs on the Pi and selects audio output there. An ordinary SSH TUI session does not turn that output into sound on the SSH client's speakers; terminal graphics protocols transfer images, not the application's audio stream. This follows from the current playback architecture and the documented terminal transport. Provide explicit silent mode and remote-device audio behavior. Listening locally requires a separately designed audio-stream/forwarding facility and synchronization; do not imply it is already provided by SSH.

## Runtime downloads and coherent archives

VideoLAN's examined current stable download tree is 3.0.24. Use versioned URLs and verified pinned digests rather than mutable `last` URLs. Exact official upstream asset URLs are:

- Windows x64: `https://download.videolan.org/pub/videolan/vlc/3.0.24/win64/vlc-3.0.24-win64.zip`
- Windows ARM64: `https://download.videolan.org/vlc/3.0.24/winarm64/vlc-3.0.24-winarm64.zip`
- macOS ARM64: `https://download.videolan.org/pub/videolan/vlc/3.0.24/macosx/vlc-3.0.24-arm64.dmg`
- macOS Intel: `https://download.videolan.org/pub/videolan/vlc/3.0.24/macosx/vlc-3.0.24-intel64.dmg`

Each directory lists corresponding `.sha256` and signature files. Implementation follow-up: the four official checksum files were retrieved over HTTPS and their SHA256 values are pinned in `scripts/runtime-manifest.json`. CI verifies the downloaded bytes before extraction and fails on mismatch. Sources: [Windows x64 index](https://download.videolan.org/pub/videolan/vlc/3.0.24/win64/), [Windows ARM64 index](https://download.videolan.org/vlc/3.0.24/winarm64/), [macOS index](https://download.videolan.org/pub/videolan/vlc/3.0.24/macosx/).

Initially distribute six application archives with executable, README/runtime setup, MIT license, dependency notices and an archive checksum manifest. Clearly label them as requiring LibVLC; a standalone Rust executable is not a self-contained player when native decoding libraries are external.

If adding runtime-inclusive archives, use separate architecture-specific outputs and preserve a coherent upstream runtime. Windows needs `libvlc.dll`, `libvlccore.dll`, plugins and any selected runtime data/dependencies. macOS's upstream bundle places libraries under `VLC.app/Contents/MacOS/lib`, plugins under `Contents/MacOS/plugins`, and additional data under `Contents/MacOS/share`. Preserve dylib relationships, symlinks and applicable signatures; copying one dylib is insufficient. Test the extracted archive in a clean environment. Sources: [VideoLAN Windows build/runtime guide](https://github.com/videolan/vlc/blob/master/doc/BUILD-win32.md), [official VLC 3 macOS packaging source](https://raw.githubusercontent.com/videolan/vlc/3.0.x/extras/package/macosx/package.mak).

For Linux, prefer distro-managed native libraries initially; a single bundled `.so` will not capture codecs, access modules and their transitive dependencies. Keep distro patch versions in test logs rather than pretending that every OS uses an identical runtime build.

## Distribution notices

Retain the application's MIT notice and record Rust dependencies. For redistributed VLC runtimes, inventory the actual shipped components and retain their licenses/notices and required corresponding-source access. Include runtime version, official origin, original asset hash, selected files and any modifications in a manifest. VideoLAN distinguishes LGPL engine code from its GPL desktop application and warns that some plugins have stricter licenses. An MIT application label or a link to upstream source alone is not proof that the exact bundled distribution meets every obligation. Sources: [LibVLC source documentation](https://github.com/videolan/vlc/blob/master/doc/standalone/libvlc.md), [VideoLAN license explanation](https://images.videolan.org/press/lgpl-libvlc.html), [official VLC source](https://github.com/videolan/vlc).

Application-only releases avoid redistributing the media runtime while that inventory is incomplete. Runtime downloads installed only for CI playback validation are a separate matter from uploading those files in a public release. This is a practical release strategy, not a legal opinion.
