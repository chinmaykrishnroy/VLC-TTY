# TermVLC v0.2.0 — Rust / Ratatui / LibVLC TUI

A terminal media player prototype: **Rust**, **Ratatui**, **Crossterm**, and **LibVLC 3.x**, with native audio and inline true-color terminal video. This is a new Rust implementation, not a merge of VLC or SparkPlayer source code.

> **Validation status:** The code is provided as a development prototype. The packaging environment did **not** contain Cargo/rustc, LibVLC, or working Git network access, so **it was not compiled or playback-tested here**. Static/source validation and the supplied sample video's integrity can be checked, but successful compilation/playback on your machine is not yet verified. Report any compile error and I can revise the source.

## Required software

- Rust stable and Cargo, from https://rustup.rs/
- LibVLC **3.x** and compatible VLC plugins (64-bit). VLC 4 APIs are different; this adapter targets 3.x.
- A terminal supporting 24-bit color: Windows Terminal, WezTerm, Kitty, GNOME Terminal, etc.

### Windows 10/11 (PowerShell)

1. Install stable Rust + Cargo via https://rustup.rs/ and restart PowerShell.
2. Install 64-bit VLC **3.x** via https://www.videolan.org/vlc/ (the executable alone is not sufficient; the directory must include `libvlc.dll`, `libvlccore.dll`, and `plugins`).
3. Unzip the project. In the extracted directory:

```powershell
cd .\termvlc_rust_v0.2.0
$env:VLC_HOME = "C:\Program Files\VideoLAN\VLC"
cargo build --release
cargo run --release -- --doctor
cargo run --release -- .\samples\sample.mp4
# Your own video:
cargo run --release -- "D:\Media\Video\Movies\example.mkv"
# Browse a folder:
cargo run --release -- "D:\Media\Video\Movies"
# Stream URL:
cargo run --release -- "https://example.com/video.mp4"
```

If the Windows DLL loader reports it cannot find `libvlccore.dll`, add the installation directory to your current PowerShell session:

```powershell
$env:PATH = "$env:VLC_HOME;$env:PATH"
```

### Ubuntu/Debian

```bash
sudo apt update
sudo apt install -y vlc libvlc5 libvlc-bin libvlc-dev pkg-config
# Install Rust from https://rustup.rs if 'cargo' is not already installed.
cargo build --release
cargo run --release -- --doctor
cargo run --release -- ./samples/sample.mp4
cargo run --release -- "$HOME/Videos/example.mkv"
```

### macOS

Install Rust from https://rustup.rs and VLC 3.x from VideoLAN. The loader looks for `/Applications/VLC.app/Contents/MacOS/lib/libvlc.dylib`. Packaging and native playback on macOS have not been verified.

## Controls

| Key | Action |
| --- | --- |
| Space | Pause/resume |
| Left/Right (or h/l) | Seek -/+ 10 seconds |
| s | Cycle subtitle tracks (including off) |
| a | Cycle audio tracks |
| n / p | Next / previous video in directory |
| Up/Down or j/k | Select file/directory |
| Enter | Open selected video or directory |
| Backspace | Parent directory |
| + / - | Volume |
| M | Mute/unmute |
| F | Focus video / hide library browser |
| [ / ] | Lower/higher playback speed |
| R | Reload directory |
| Q/Esc | Quit |

## CLI Options

```bash
cargo run --release -- [OPTIONS] [FILE|DIRECTORY|URL]

Options:
  --fps <N>           Terminal refresh limit (1 to 60, default 15)
  --decode-width <N>  LibVLC video callback width (64 to 1920, default 480)
  --ascii             Render in monochrome ASCII mode
  --doctor            Check LibVLC runtime availability
  -h, --help          Show help message
```

## Design

- `src/vlc.rs` — **real LibVLC C-ABI calls loaded with libloading**. LibVLC handles audio output, decoding, playback timing, subtitles, audio tracks, and seeking. Video callback buffers are 32-byte aligned. The display callback produces an owned latest frame for the UI. Windows dynamically auto-configures PATH and plugin paths.
- `src/renderer.rs` — Ratatui layout, browser, transport gauge and video in the content panel. Supports ANSI TrueColor (`▀`) and monochrome ASCII rendering.
- `src/app.rs` — keyboard, directory browsing, track cycling, and playback commands.
- `src/main.rs` — CLI parser, terminal lifecycle, event loop, and initialization.
- `samples/sample.mp4` — actual 8-second H.264/AAC test clip.

## Important limitations

This is a terminal media player targeting **Ratatui**.

- ANSI TrueColor rendering, not Kitty/Sixel image graphics. FPS can be tuned via `--fps` and resolution via `--decode-width`.
- No playlist saving, disc menus, media database, hardware rendering, or equalizer.
- URL playback is accepted as a command-line argument; browsing is limited to local files.
- Test with **VLC 3.x**. Versions must match 64-bit Rust/64-bit VLC.

## Test locally

```bash
cargo fmt --check
cargo test
cargo run -- --doctor
cargo run -- ./samples/sample.mp4
```

Verify that you see changing colors (not just the startup screen), hear the sample's tone, can pause/resume and seek, and the terminal is restored after quitting. If an error occurs, save the terminal output.

## License and upstream references

This original code is MIT-licensed (see `LICENSE`); it does not contain copied SparkPlayer or VLC source. For upstream examples/ideas, see `UPSTREAM.md`. LibVLC and its plugins have their own LGPL/GPL licensing and redistribution rules.
