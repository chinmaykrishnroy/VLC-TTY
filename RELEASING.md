# Release process

CI runs formatting, locked unit tests, strict Clippy, packaging regressions, required LibVLC runtime tests, a release build and a decode check against each extracted application archive.

| Target | Native runner | Archive |
| --- | --- | --- |
| Linux x64 | Ubuntu 24.04 | `.tar.gz` |
| Linux ARM64 | Ubuntu 24.04 ARM64 | `.tar.gz` |
| macOS Intel | macOS 15 Intel | `.tar.gz` |
| macOS ARM64 | macOS 15 ARM64 | `.tar.gz` |
| Windows x64 | Windows 2025 | `.zip` |
| Windows ARM64 | Windows 11 ARM64 | `.zip` |

A separate Ubuntu 26.04 ARM64 lane checks headless decoding with distro LibVLC and no desktop packages. It is not a Raspberry Pi performance test. Linux release binaries use Ubuntu 24.04 as their baseline; compatibility with older glibc distributions is not promised.

## Artifacts

Every archive contains the executable, usage/runtime instructions, original SVG, MIT notice, Cargo dependency inventory/license texts and `BUILD.json` (version, target, source commit, compiler and binary hash). Each archive has a SHA256 sidecar. Release publication combines them into `SHA256SUMS` and refuses missing/extra targets or checksum mismatches.

**LibVLC is not included.** Install it separately as described in the README. CI installs coherent runtimes only for testing: distro packages on Linux; hash-pinned official 3.0.24 archives on Windows/macOS. The download manifest lives in `scripts/runtime-manifest.json`. Runtime-inclusive distribution needs a separate component, license and corresponding-source inventory before implementation.

Windows executables are not Authenticode signed and macOS archives are not notarized. Test OS security prompts before calling these end-user installers. There is no automatic install script that changes a user's machine.

## Publishing

1. Update Cargo version/lockfile, changelog and validation evidence. Complete the manual terminal/device acceptance appropriate to claims made in release notes.
2. Push changes and require all native CI jobs to pass. A manual **Release** workflow run builds downloadable CI artifacts without publishing a GitHub release.
3. When publication is approved, create and push `v<package version>` (for example `v0.6.0`). The tag must exactly match Cargo.toml.
4. The tag workflow rebuilds and tests every target. Only after all jobs succeed does it publish six archives and `SHA256SUMS` to GitHub Releases. No runtime libraries are uploaded. Existing releases are not overwritten.

Creating this pipeline does not create a version tag or publish a release. CI artifacts are available on the workflow run for 14 days. Local development archives can contain uncommitted changes; official release archives are built from the clean tagged checkout.
