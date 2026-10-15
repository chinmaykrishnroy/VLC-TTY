# TermVLC 0.5.0 — VLC-style terminal media player

A **Rust + Ratatui** terminal application using **LibVLC 3.x** for demuxing, decoding, playback, audio and audio/video synchronization. The design and interface are original (not a fork of VLC or SparkPlayer).

**This release is a source candidate.** The packaging environment did not contain `cargo`, `rustc`, or LibVLC, and could not download them. Static checks and sample media validation were possible; compilation and real playback have **NOT** been verified. See [VALIDATION.md](VALIDATION.md). Don't assume it's production ready until `cargo test` and a local playback smoke test succeed.

## Changes over v0.4.0

- **VLC-inspired cone logo**: multi-line orange/light ANSI-art on the launch/empty player, compact cone in sidebar, and ▲ in header. TermVLC is **not affiliated with VideoLAN**.
- **True, reversible fullscreen**: `F` / `F11` removes the library, title, progress, and toolbar. The image fills the terminal while maintaining its aspect ratio. A small `‹ BACK [Esc / F]` control is clickable. `Esc`, `Backspace` or `F` restores the normal layout, including the browser. Terminal is cleared on transitions and image caches are invalidated. Image graphics are temporarily hidden behind dialogs to avoid terminals painting images over menu text.
- **Embedded subtitle/audio track menus**: `S` / `A`, arrow keys and Enter select tracks via LibVLC.
- **Reliable text subtitle overlay** for UTF-8 `.srt` / `.vtt` files: automatic sidecar discovery (`Movie.srt`, `Movie.en.srt`, etc.), `L` to load a file at runtime, `--subtitle` at startup, ±100 ms sync via `,`/`.`. The separate text path is necessary because custom LibVLC video callbacks do not reliably produce composited subtitles on every system. ASS/SSA go via LibVLC and may not be visible in embedded terminal rendering.
- **Mouse interactions**: click fullscreen/back/play/subtitles/audio, click or drag progress to seek, double click media to open, wheel scrolls lists.
- **VLC-style UI**: media file browser, timeline, pause/play, volume/mute, speed, status/OSD, help (`?`) and info (`I`) windows, `O` to open file/folder/URL, `X` stop.
- **All v0.4 graphics retained**: Kitty/Sixel/iTerm2 auto-detection, high-quality pixel rendering, bounded image-encoding worker, ANSI half-block fallback, `G` toggle, native VLC window alternative.
- One Rust source tree for Windows, Linux and macOS; Windows is the first recommended test platform.

## Windows 11 — PowerShell

Install:
1. [Rust](https://rustup.rs/) (stable, Windows MSVC toolchain and C++ Build Tools).
2. [VLC 3.x](https://www.videolan.org/vlc/) 64-bit. **VLC 4 has a different API**.
3. A graphics-capable terminal (try `--graphics auto`; ANSI fallback works without one).

Extract the zip, then run these commands **in the extracted folder containing `Cargo.toml`**:

```powershell
$env:VLC_HOME = 'C:\Program Files\VideoLAN\VLC'
$env:PATH = "$env:VLC_HOME;$env:PATH"
cargo build --release
cargo test
.\target\release\termvlc.exe --doctor
.\target\release\termvlc.exe --graphics auto ".\samples\sample.mp4"
```

Play an episode:

```powershell
.\target\release\termvlc.exe --graphics auto "E:\data\Media\Anime\Ben 10 Omniverse\Season 03\Episode.mkv"
```

Play with a specific external subtitle:

```powershell
.\target\release\termvlc.exe --graphics auto --subtitle "C:\Videos\my-movie.en.srt" "C:\Videos\my-movie.mkv"
```

If your terminal doesn't support image protocols, use `--graphics ansi`. For conventional GPU-decoded video **in a separate native window**, use `--native-window` instead of expecting GPU-equivalent smoothness inside the terminal.

## Ubuntu / Debian Linux

```bash
sudo apt update && sudo apt install -y vlc libvlc5
# Install Rust from https://rustup.rs first if needed
cargo build --release
cargo test
./target/release/termvlc --doctor
./target/release/termvlc --graphics auto ./samples/sample.mp4
```

Use Kitty or a suitable Sixel terminal for real pixel images. You can also run `bash scripts/linux-run.sh /path/to/movie.mkv`.

## macOS (Intel and Apple Silicon)

Install official VLC 3.x `.app` in `/Applications` and Rust stable. Run from iTerm2 or Kitty:

```bash
export VLC_HOME="/Applications/VLC.app"
cargo build --release
cargo test
./target/release/termvlc --doctor
./target/release/termvlc --graphics auto ./samples/sample.mp4
```

Built-in Terminal.app may fall back to ANSI half-block video. You can also use `bash scripts/macos-run.sh /path/to/movie.mkv`.

## Shortcuts

| Key / mouse | Behavior |
| --- | --- |
| `Space` / click Play | Play or pause |
| `F`, `F11` | Toggle fullscreen video |
| `Esc`, `Backspace`, click **‹ Back** | Return from fullscreen (Esc closes open menus first) |
| `Left / Right` | Seek ±10 seconds |
| Click/drag orange timeline | Seek to position |
| `+ / -`, `M` | Volume ±5%, mute |
| `X` | Stop |
| `[` / `]` | Playback rate in 0.25× increments |
| `S` / click CC | Choose VLC subtitle track, disable it, or use external sidecar |
| `L` | Load SRT, WebVTT, ASS/SSA subtitle file from a path |
| `,` / `.` | Delay local text subtitles earlier/later by 100 ms |
| `A` / click Audio | Choose audio track |
| `O` | Type a local file/folder/HTTP(S) URL to open |
| `↑ / ↓`, `Enter`, `Backspace` | Browse and open; parent folder in standard layout |
| Double-click file | Open media from sidebar |
| `N / P` | Next / previous media in current directory |
| `G` | Toggle high-quality terminal graphics / ANSI fallback |
| `?` / `F1`, `I` | Help / media information |
| `R`, `Q` | Refresh directory / quit |

The archive contains `samples/sample.mp4` (8-second H.264/AAC video) and a matching `samples/sample.srt` that should be detected automatically. You can also use `--subtitle testdata/sample.vtt` to verify WebVTT parsing.

## Design and limitations

- Native LibVLC C symbols are resolved at runtime (`libloading`). **No VLC SDK or headers needed to compile**, but VLC runtime libraries and plugins are needed to run.
- `src/graphics.rs` probes available terminal graphics; does not promise pixel output when the terminal cannot support Kitty/Sixel/iTerm2 or forward queries. Auto falls back to ANSI. `--graphics kitty` is an opt-in override, not guaranteed to work everywhere.
- Terminal inline-video smoothness will vary: larger images must be encoded/transferred every frame and terminals may be bandwidth limited. `TERMVLC_DECODE_MAX=0` requests the original full video frame dimensions, default decode cap 1080p; this **does not guarantee 1080p video display**. `--native-window` uses a separate conventional VLC video window.
- SRT/WebVTT captions are intentionally displayed outside pixel graphics to prevent protocol graphics from covering text. In **native-window** mode, external subtitle files are delegated to LibVLC's compositor instead; UTF-8 parsing is only used for inline terminal mode.
- Embedded VLC subtitles can be **selected**, but rendering inside custom-memory video output may depend on VLC internals and output modules. For reliable terminal captions, use an external UTF-8 `.srt` or `.vtt`.
- No optical disc menus, DVD navigation, equalizer, snapshot saving, playlists on disk, resume history, or hardware accelerated **inline** video yet.
- The test suite has never run against real Windows Terminal/Kitty/iTerm2 + LibVLC in this workspace. Run `cargo test` and a real `samples/sample.mp4` playback test locally before relying on it.

## Source and licenses

TermVLC's code is MIT-licensed. Using installed LibVLC does not make this an official VideoLAN product; distribution with bundled plugins requires a separate audit of their LGPL/GPL obligations. SparkPlayer is GPL-2.0-only and its code was **not** copied into this repository.
