# Validation — TermVLC v0.4.0 (source candidate)

## Verified in packaging environment

- Three-platform CLI instructions and helper scripts exist, are nonempty, and are included in the archive.
- OS-specific LibVLC candidate paths include the Windows `libvlc.dll`, Linux `libvlc.so.5`, macOS VLC.app layout plus Intel/Apple Silicon Homebrew library prefixes.
- `app.rs` uses `ratatui::crossterm` instead of relying on an undeclared direct `crossterm` dependency (fixed from v0.3).
- CI is configured for Windows, Ubuntu, and macOS compilation and unit tests; **it has not yet been run**.
- Cargo manifest and workflow YAML structurally parse. Archive ZIP integrity checked.
- Bundled sample MP4 is present and FFprobe-readable.

## Not verified

- `cargo build`, `cargo test`, Rust formatting and Clippy: no Rust toolchain/crates registry available in the sandbox.
- Real LibVLC video decoding/audio playback on Windows, Linux, or macOS.
- Terminal graphics autodetection, protocol redraw performance, or high-FPS playback.
- Native-window behavior on each OS.

## Local acceptance matrix

| Platform | Minimum tests |
| --- | --- |
| Windows 11 | `cargo test`; `--doctor`; auto graphics; Sixel if supported; ANSI; `--native-window`; clean exit |
| Linux | Same, then Kitty/Sixel with a compatible emulator, resize and SSH checks |
| macOS Intel/ARM | Same, iTerm2/Kitty graphic paths, VLC.app plugin loading, native-window test |

Please capture the **first failing command and full error output** instead of assuming the package is a verified release.
