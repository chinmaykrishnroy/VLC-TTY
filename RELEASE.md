# Releases

The public application version starts at **0.0.1**. Earlier 0.4–0.6 numbers were development prototype checkpoints, not public versioned releases.

The preferred distribution is **one portable executable per OS and CPU architecture**, with embedded LibVLC libraries/plugins. Windows uses `.exe`; Linux/macOS use native executable files. An installer is unnecessary for the initial TUI release.

The portable launcher extracts the player and runtime into a private temporary directory, inherits the caller's terminal and removes the temporary directory after normal exit. It installs no system services and needs no installed VLC. Base OS libraries remain platform requirements; Linux builds use Ubuntu 24.04 / glibc 2.39 as their compatibility baseline.

Choose `windows-x64` for normal Intel/AMD Windows PCs and `windows-arm64` for ARM Windows devices. An ARM64 PE file cannot run on x64 Windows. Linux ARM64 is the build for a 64-bit Raspberry Pi OS/Ubuntu installation.

## Portable controls

- `-h` / `--help`: complete usage, without extracting the runtime.
- `--bundle-info`: embedded version, architecture, source commit and runtime provenance.
- `--extract-to NEW_DIRECTORY`: expose player, libraries and notices for inspection/replacement.
- `VLC_TTY_RUNTIME`: use a compatible replacement runtime instead of the embedded one.
- `TMPDIR` / `TEMP`: control the temporary filesystem; Linux extraction must allow execution.

## Publication gates

Required gates include architecture inspection, formatting, unit and real LibVLC tests, strict Clippy, portable decoding with installed-runtime paths disabled, Unix terminal interaction, component/license inventory and corresponding-source records. A Windows launch/decoding smoke is required for both Windows architectures on their native runners.

Windows player and launcher builds statically link their Microsoft C runtime; import-table checks reject a dependency on an installed Visual C++ redistributable. Unix SIGTERM/SIGHUP and console Ctrl+C/Break request normal cleanup. Windows Job Objects stop the extracted child when its supervisor dies. Uncatchable termination or power loss can leave temporary files; no application can run cleanup after SIGKILL/TerminateProcess.

**Current publication blocker:** exact Windows/macOS runtime-to-contrib-source correspondence has not been established. The preserved recipe source superset alone is insufficient evidence; `release_manifest.py` refuses publication while that identity is unverified. Linux records exact distro source package versions. CI development artifacts remain available for testing. Resolve upstream build provenance or build the runtime from preserved sources before tagging the public release.

Publish single executable assets, SHA256 checksums, build manifests and source/notice companions after all gates pass. Source companions accompany the component records; they are not installers. The build manifest must describe remaining runtime/system dependencies and source-evidence limitations honestly.

CI uploads executable assets separately from the larger Linux runtime-source archives, so downloading a player artifact does not also download its source companion. The tag workflow gathers both artifact types before checking publication requirements.

Use a version tag matching Cargo.toml only after the complete gates pass. Creating or updating this pipeline does not itself create a tag or publish a release. See [validation](VALIDATION.md), [standalone research](docs/standalone-research.md) and [contributing](CONTRIBUTING.md).
