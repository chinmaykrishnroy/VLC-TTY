<p align="center">
  <img src="assets/vlc-tty.svg" width="160" alt="VLC-TTY — traffic cone inside a terminal">
</p>
<h1 align="center">VLC-TTY</h1>
<p align="center">Your media. Your terminal. Even over SSH.</p>
<p align="center">
  <a href="https://github.com/chinmaykrishnroy/VLC-TTY/actions/workflows/ci.yml"><img src="https://github.com/chinmaykrishnroy/VLC-TTY/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/engine-LibVLC%203-orange" alt="LibVLC 3">
  <img src="https://img.shields.io/badge/UI-Ratatui-orange" alt="Ratatui">
</p>

**VLC-TTY plays media inside your terminal**, with familiar VLC controls powered by LibVLC. Play local media, stream compressed files from a server to your local speakers, or render video on a headless server over SSH.

> Development preview. Portable builds embed LibVLC 3; source builds use an installed runtime. A compatible terminal is required. Pi 5 headless decoding and ANSI controls have been tested; pixel-protocol visual acceptance and long sessions remain open. See [validation](VALIDATION.md).

## Features

- Local files, folders and network URLs through LibVLC.
- Play/pause, seek, stop, speed control and frame stepping.
- File browser, UTF-8 M3U playlists, shuffle and repeat modes.
- External SRT/VTT captions (UTF-8, BOM-marked UTF-16 and Windows-1252), track selection and subtitle timing controls.
- Kitty, Sixel and iTerm2 pixel graphics, plus ANSI half-block fallback. Kitty uses lossless compression when confirmed by the terminal; `--no-kitty-compression` opts out.
- Sixel video follows the widget's pixel size on resize. Adaptive colour-palette training preserves spatial detail, with light dithering for gradients; static album artwork keeps its full-detail encoder.
- Silent SSH profile with bounded decode size and frame rate.
- Original compressed-file streaming over SSH with local decoding, speakers and seeking.
- Experimental server-rendered playback with PCM audio forwarded to local speakers.
- Runtime diagnostics and a headless decoding check.

## Get started

Portable builds target **Linux, Windows and macOS**, on **x64 and ARM64**, starting at **v0.0.1**. Each download is one executable with LibVLC and its plugins embedded; there is no installer or separate VLC installation. The runtime is extracted privately while the app runs. Linux requires glibc 2.39+ (Ubuntu 24.04 or newer).

Choose **windows-x64** for Intel/AMD PCs, **windows-arm64** for ARM Windows devices, and **linux-arm64** for a 64-bit Raspberry Pi. Windows ARM64 files cannot run on Windows x64.

Get verified files from [Releases](https://github.com/chinmaykrishnroy/VLC-TTY/releases) when available, or from a successful [CI run](https://github.com/chinmaykrishnroy/VLC-TTY/actions/workflows/ci.yml). Source/notice companions are provided alongside the executables; they are not needed for ordinary playback. See [release details](RELEASE.md).

```sh
vlc-tty /path/to/movie.mp4
vlc-tty --subtitle movie.en.srt movie.mkv
vlc-tty --graphics ansi movie.mp4
vlc-tty --playlist favourites.m3u8 --shuffle --repeat all
vlc-tty --playlist favourites.m3u8 --export-playlist copy.m3u8
vlc-tty --volume 80 --rate 1.25 movie.mp4
vlc-tty --fullscreen movie.mp4
vlc-tty --help
```

To build from source, install Rust stable and run `cargo build --locked --release`. The executable is `target/release/vlc-tty` (`vlc-tty.exe` on Windows). See [BUILDING.md](BUILDING.md).

## Watch over SSH

For music or a video file stored on a server, stream the original file to your local player:

```sh
vlc-tty stream user@pi '/home/user/Music/song.mp3'
```

Decoding, terminal rendering and audio run locally. The server needs SSH and Python 3, with no VLC-TTY installation or public HTTP port. Byte ranges support seeking without transcoding or a mandatory full download. Track tags update from LibVLC, and embedded MP3 artwork loads through bounded ID3 range reads. The UI shows readable filenames instead of URL escapes. This mode currently opens one regular file; remote directory browsing, playlist expansion and separate folder-cover images remain follow-up work.

To run decoding and the terminal UI on the headless server instead, use the server player:

On the server, copy the matching Linux portable executable and make it executable with `chmod +x`. Then launch from your local terminal:

```sh
ssh -t user@pi 'vlc-tty --ssh --silent /path/to/movie.mp4'
```

No display or desktop session is required on the server. SSH sessions default to silent playback, 15 FPS and a 720p callback height cap. Your **local terminal** determines image-protocol support. Try `--graphics ansi` for broad compatibility or `--fps 10 --decode-height 480` on slower connections.

```sh
vlc-tty --ssh --doctor
vlc-tty --ssh --smoke-test /path/to/movie.mp4
```

The smoke check requires decoded frames and an advancing playback clock, without opening a UI. Plain SSH does not forward sound to client speakers. For server-rendered video with forwarded audio, use `vlc-tty connect user@pi /path/to/movie.mp4`; both machines need compatible builds. This [experimental audio helper](docs/REMOTE-AUDIO.md) is implemented, but physical A/V synchronization and movie-length reliability remain unvalidated. Prefer `stream` for ordinary remote files. Read the [SSH guide](docs/SSH.md) for terminal setup, tmux and Pi acceptance checks.

## Controls

| Key | Action |
| --- | --- |
| `Space` / `s` | Play or pause / stop |
| `←` / `→` | Seek backward / forward 10 seconds |
| `f` / `Esc` | Fullscreen / return |
| `+` / `-` / `m` | Volume / mute (local or server audio) |
| `[` / `]` / `=` | Slower / faster / normal speed |
| `e` | Next frame |
| `n` / `p` / `Tab` | Next / previous / queue |
| `z` / `Shift+R` | Shuffle / cycle repeat off, all, one |
| `o` / `l` | Open media / load external captions |
| `Shift+S` / `Shift+A` | Subtitle / audio track picker |
| `,` / `.` | Adjust subtitle delay |
| `g` | Toggle pixel graphics / ANSI fallback |
| `?` / `i` / `q` | Help / media information / quit |

Mouse controls and menus are available where terminal mouse reporting is supported. Hover highlights menu headers and dropdown rows; with a menu open, moving across headers switches menus. Commands activate on click or Enter. Scroll over the timeline to seek 10 seconds forward/backward; elsewhere in the player, the wheel changes volume. Open menus and dialogs consume wheel events. Fullscreen also requests local Windows Terminal window fullscreen through its default F11 binding, with geometry confirmation and restoration on exit. Other hosts and server-side SSH playback use the terminal's own fullscreen shortcut; see the [fullscreen guide](docs/WINDOWS-TERMINAL.md).

For outer borders in Windows Terminal, set profile padding to `0` and hide its scrollbar; see the [terminal appearance guide](docs/WINDOWS-TERMINAL.md). Artwork/video keeps its aspect ratio, so differently shaped media can still have black bars.

## Documentation

[SSH guide](docs/SSH.md) · [Building](BUILDING.md) · [Architecture](ARCHITECTURE.md) · [Objective](OBJECTIVE.md) · [Roadmap](ROADMAP.md) · [Releasing](RELEASE.md) - [Contributing](CONTRIBUTING.md) · [Validation](VALIDATION.md) · [Pi 5 results](docs/PI5-VALIDATION.md) · [Changelog](CHANGELOG.md)

## Contributing

Bug reports are welcome. Include OS/architecture, terminal/version, `vlc-tty --doctor` output, media codec/resolution and steps to reproduce. For SSH issues, include whether tmux is involved. Keep private media URLs and credentials out of reports.

Before submitting changes, run formatting, unit tests and strict Clippy as described in [BUILDING.md](BUILDING.md). Playback changes also require the LibVLC runtime tests.

## License

Application code is [MIT licensed](LICENSE). Bundled LibVLC, plugins and codecs retain their own licenses; the portable distribution is not MIT-only. Runtime source and notice companions accompany releases. VLC-TTY is an independent project and is not affiliated with VideoLAN. The terminal-cone SVG is original project artwork inspired by the familiar media-player traffic cone.
