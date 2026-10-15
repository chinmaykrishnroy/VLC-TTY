# TermVLC v0.4.0 — cross-platform terminal video player

**Windows prioritized; Linux and macOS supported in the same Rust/Ratatui codebase.** LibVLC 3.x is loaded at runtime. The video pane attempts full-color **Kitty, Sixel, or iTerm2 pixel graphics** and automatically falls back to ANSI/Unicode half-blocks if terminal probing cannot confirm support. For conventional smooth, full-resolution playback, use `--native-window` (a **separate** LibVLC video window, not an embedded GPU window).

**Important validation limitation:** This ZIP is a **source build candidate**. There is no Rust toolchain or LibVLC runtime in the packaging environment, so this release has not yet been compiled or smoke-tested on Windows, Linux, or macOS. The included CI workflow performs compile/tests on the three operating systems *once published to GitHub*. Passing compilation tests does not prove that native VLC playback or image protocol detection work. See `VALIDATION.md`.

## Platform choices

| OS | Suggested terminals | Pixel output | VLC installation |
| --- | --- | --- | --- |
| **Windows 11 (priority)** | Windows Terminal (Sixel where supported), Kitty-capable terminal, WezTerm | Sixel / Kitty / iTerm2 if detected; otherwise ANSI | 64-bit **VLC 3.x**, default `C:\Program Files\VideoLAN\VLC` |
| **Linux** | Kitty, WezTerm, foot (Sixel), other image-capable terminals | Kitty / Sixel / iTerm2 if detected; otherwise ANSI | Distro `libvlc` runtime + VLC plugins |
| **macOS (Intel + Apple Silicon)** | **iTerm2**, Kitty, WezTerm, Terminal.app | iTerm2 / Kitty if detected; otherwise ANSI | VLC 3.x app bundle (normally `/Applications/VLC.app`) |

Terminal graphics support depends on the **terminal emulator**, not solely on the OS. Windows Terminal support varies by version and environment. WezTerm is often detected using iTerm2-compatible graphics rather than Kitty; trust `--graphics auto` first. Apple's built-in Terminal.app generally cannot display these pixel graphics, so ANSI mode is expected.

## Windows PowerShell — first platform to test

1. Install Rust (https://rustup.rs), including the Microsoft C++ Build Tools needed for the MSVC target, and the **matching-architecture VLC 3.x** desktop application (https://www.videolan.org/vlc/).
2. Extract this ZIP. Open **PowerShell in the folder containing `Cargo.toml`**.
3. Run:

```powershell
$env:VLC_HOME = "C:\Program Files\VideoLAN\VLC"
$env:PATH = "$env:VLC_HOME;$env:PATH"
cargo build --release
cargo test
.\target\release\termvlc.exe --doctor
.\target\release\termvlc.exe --graphics auto ".\samples\sample.mp4"
```

For your own video:

```powershell
.\target\release\termvlc.exe --graphics auto "E:\data\Media\Anime\Ben 10 Omniverse\Season 03\Episode.mkv"
```

If auto falls back to ANSI despite confirmed terminal graphics support, try Sixel on a compatible Windows Terminal version:

```powershell
.\target\release\termvlc.exe --graphics sixel --fps 15 "E:\data\Media\Anime\Ben 10 Omniverse\Season 03\Episode.mkv"
```

For a native, separate video window with conventional playback quality:

```powershell
.\target\release\termvlc.exe --native-window "E:\data\Media\Anime\Ben 10 Omniverse\Season 03\Episode.mkv"
```

Or run `powershell -ExecutionPolicy Bypass -File scripts\windows-run.ps1` to build and play the sample.

## Linux

Install VLC and its shared library with your distribution's package manager:

```bash
# Ubuntu / Debian
sudo apt update && sudo apt install -y vlc libvlc5
# Install Rust via rustup.rs if cargo is missing
cargo build --release
cargo test
./target/release/termvlc --doctor
./target/release/termvlc --graphics auto ./samples/sample.mp4
```

Fedora/Arch package names differ. For a library in a nonstandard installation prefix, set `VLC_HOME=/your/prefix`. For local playback replace the sample path. `bash scripts/linux-run.sh "/path/to/video.mkv"` builds, tests, and launches in one command.

**Linux over SSH:** image support and audio behavior depend on the *local* terminal emulator and how audio is routed. Remote LibVLC audio will ordinarily play on the remote host, not automatically on your computer.

## macOS — Intel and Apple Silicon

Install [VLC 3.x for macOS](https://www.videolan.org/vlc/download-macosx.html) and Rust via [rustup](https://rustup.rs/). Open **iTerm2** or Kitty for the best chance of real pixel graphics:

```bash
export VLC_HOME="/Applications/VLC.app"
cargo build --release
cargo test
./target/release/termvlc --doctor
./target/release/termvlc --graphics auto ./samples/sample.mp4
```

Alternatively: `bash scripts/macos-run.sh "/Users/me/Movies/movie.mkv"`.

TermVLC checks the VLC.app LibVLC bundle and common Homebrew prefixes (`/opt/homebrew` for Apple Silicon, `/usr/local` for Intel). If `--doctor` reports that the library cannot be loaded, confirm VLC.app is installed and try setting `VLC_HOME` to the app bundle or your lib directory. Homebrew's `vlc` cask and formula availability may differ, so the official VLC.app is the recommended starting point.

## Rendering modes and controls

```text
--graphics auto      Query terminal graphics, fall back to ANSI if unsupported (default)
--graphics kitty     Force Kitty image protocol; needs terminal support
--graphics sixel     Force Sixel raster graphics; needs terminal support
--graphics iterm2    Force iTerm2 OSC 1337 graphics; needs terminal support
--graphics ansi      Unicode half-block fallback
--fps 24             Cap graphics conversion requests, 1–60
--native-window      Use LibVLC's separate native video window
--doctor             Print LibVLC loading results and detected library path
```

Controls: `Space` play/pause, `←/→` seek 10s, `↑/↓` browse files, `Enter` play/open, `Backspace` parent directory, `+/-` volume, `m` mute, `[` and `]` playback rate, `f` focus video, `g` switch between pixel graphics and ANSI, `q` quit.

**Quality caveat:** The decoded source is capped to 1080p by default to avoid excessive frame conversion. Use `TERMVLC_DECODE_MAX=0` to request original dimensions (high RAM/CPU usage); terminal presentation still scales to its available pixel area and cannot promise 1080p/60 output. `--native-window` is still the best option for normal video-player quality.

## How it is structured

- `src/vlc.rs`: LibVLC 3.x FFI loader, OS-specific search paths, video callback, audio/playback control.
- `src/graphics.rs`: Kitty/Sixel/iTerm2 probing, conversion worker, fallback.
- `src/renderer.rs`: Ratatui panes, video and status controls.
- `src/app.rs`: file browser and keyboard interactions.
- `src/main.rs`: CLI and lifecycle; built from the same source tree for all three OSes.
- `.github/workflows/rust.yml`: Windows, Ubuntu and macOS compile/tests when pushed to GitHub.
- `scripts/windows-run.ps1`, `scripts/linux-run.sh`, `scripts/macos-run.sh`: OS launch guides.

This is a standalone implementation inspired by upstream architecture, **not a copy** of VLC or SparkPlayer source. See `UPSTREAM.md` for links and licensing notes.
