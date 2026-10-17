# TermVLC 0.5.0 — VLC-style terminal media player

A **Rust + Ratatui** terminal application using **LibVLC 3.x** for demuxing, decoding, playback, audio and audio/video synchronization. The design and interface are original (not a fork of VLC or SparkPlayer).

**Product direction:** terminal-first. Video playback and controls belong inside the terminal, with familiar VLC workflows adapted to keyboard and mouse use. Ratatui and terminal image protocols are the primary frontend. The existing separate native-video-window mode is an optional fallback.

The next milestone focuses on terminal rendering latency and frame pacing, captions/menu ordering over video, responsive input and resizing, and real audio/video synchronization. Queue editing, shuffle, saved settings and resume positions follow that playback-quality work.

**Development build.** The reliability foundation has been compiled and tested on Windows with LibVLC 3.0.24, including actual local/HTTP frame decoding and playback controls. Audible synchronization, terminal graphics quality, clean-machine installation, and Linux/macOS runtime behavior still need acceptance testing. See [VALIDATION.md](VALIDATION.md).

## Reliability foundation — 17 October 2026

- Playback controls have a testable interface separate from the terminal event loop.
- A playback queue stays independent of the file browser. Opening a local file seeds it from that file's folder; URLs seed a single-item queue. Next/Previous respect queue boundaries, end-of-media advances automatically, and **Playback → Repeat** cycles Off / All / One.
- **Tab / Ctrl+L** opens the actual queue; **View → Browse Files** opens the directory browser. Opening a directory also opens the browser.
- Fixed negative volume, stuck subtitle cycling, overlapping text captions, Unicode metadata, mouse-menu Quit, renderer menu handling, and rapid-key input loss.
- Audio/embedded-subtitle synchronization reaches LibVLC with the correct time units. Frame stepping uses LibVLC's frame-step operation. Failed controls report failure rather than claiming success.
- Double-click video changes fullscreen without pausing playback. Single-click the Play button or press Space to pause.
- `--doctor` reports the actual LibVLC version. Incompatible major versions are rejected before using the LibVLC 3 interface.
- Unit tests run without VLC. Explicit runtime tests require it and fail if it is unavailable; Windows CI installs VLC and runs those tests separately.

## Changes over v0.4.0

- **VLC-inspired cone logo**: multi-line orange/light ANSI-art on the launch/empty player, compact cone in sidebar, and ▲ in header. TermVLC is **not affiliated with VideoLAN**.
- **In-terminal fullscreen**: `f` / `F11` removes the menus, timeline and toolbar; `Esc`, `Backspace`, `f` or double-click returns. The top-left click target also returns. Use the terminal host's own controls for operating-system window fullscreen; TermVLC does not inject global F11 keystrokes.
- **Embedded subtitle/audio track menus**: `Shift+S` / `Shift+A`, arrow keys and Enter select tracks via LibVLC; `v` / `b` cycle tracks.
- **Text subtitle overlay** for UTF-8 `.srt` / `.vtt` files: automatic sidecar discovery (`Movie.srt`, `Movie.en.srt`, etc.), `l` to load a file, `--subtitle` at startup, and ±100 ms sync via `,`/`.`. ASS/SSA and embedded tracks use LibVLC; their actual appearance in each terminal protocol still needs verification.
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
cargo test -- --ignored --test-threads=1 --nocapture
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
| `Shift+S` / click CC, `v` | Choose subtitle track / cycle subtitles |
| `L` | Load SRT, WebVTT, ASS/SSA subtitle file from a path |
| `,` / `.` | Delay local text subtitles earlier/later by 100 ms |
| `Shift+A` / click Audio, `b` | Choose audio track / cycle audio tracks |
| `o` / `Ctrl+O` | Open file picker (path entry on Linux/macOS) |
| `Ctrl+N` / `Ctrl+U` | Type a network URL or local path |
| `Ctrl+D` | Enter a folder path to browse |
| `↑ / ↓`, `Enter`, `Backspace` | Browse and open; parent folder in standard layout |
| Double-click file | Open media from sidebar |
| `n / p` | Next / previous item in the playback queue |
| `Tab` / `Ctrl+L` | Show or hide playback queue |
| Playback → Repeat | Cycle repeat Off / All / One |
| `G` | Toggle high-quality terminal graphics / ANSI fallback |
| `?` / `F1`, `I` | Help / media information |
| `R`, `Q` | Refresh directory / quit |

The archive contains `samples/sample.mp4` (8-second H.264/AAC video) and a matching `samples/sample.srt` that should be detected automatically. You can also use `--subtitle testdata/sample.vtt` to verify WebVTT parsing.

## Design and limitations

- Native LibVLC C symbols are resolved at runtime (`libloading`). **No VLC SDK or headers needed to compile**, but VLC runtime libraries and plugins are needed to run.
- `src/graphics.rs` probes available terminal graphics; does not promise pixel output when the terminal cannot support Kitty/Sixel/iTerm2 or forward queries. Auto falls back to ANSI. `--graphics kitty` is an opt-in override, not guaranteed to work everywhere.
- Terminal inline-video smoothness will vary: images must be encoded/transferred every frame. The current default uses uncapped source dimensions (`TERMVLC_DECODE_MAX=0`); set `TERMVLC_DECODE_MAX=1080` to reduce callback-buffer dimensions and CPU work. This does not guarantee 1080p terminal display. `--native-window` uses a separate conventional VLC video window.
- SRT/WebVTT captions are text overlays on the video area. Image/text ordering still needs verification per terminal protocol. In **native-window** mode, external subtitle files use LibVLC's compositor after playback starts; UTF-8 parsing is used for inline mode.
- Embedded VLC subtitles can be **selected**, but rendering inside custom-memory video output may depend on VLC internals and output modules. For reliable terminal captions, use an external UTF-8 `.srt` or `.vtt`.
- No optical disc menus, DVD navigation, equalizer, snapshot saving, queue editing/import/export, shuffle, resume history, or hardware accelerated **inline** video yet.
- The decoder/control tests use dummy audio output and do not verify audible sound or terminal visuals. Run the manual checks in [VALIDATION.md](VALIDATION.md) before relying on a particular terminal or audio device.

## Source and licenses

TermVLC's code is MIT-licensed. Using installed LibVLC does not make this an official VideoLAN product; distribution with bundled plugins requires a separate audit of their LGPL/GPL obligations. SparkPlayer is GPL-2.0-only and its code was **not** copied into this repository.
