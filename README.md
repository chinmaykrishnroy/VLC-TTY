# TermVLC — LibVLC-powered terminal video player (MVP v0.1.0)

A **standalone** terminal video player. Actual media playback, audio/video synchronization, seeking, audio tracks and subtitles are provided by **LibVLC 3.x**; the terminal UI and video display are implemented in standard-library Python using the documented LibVLC C callbacks. Frames render directly in the terminal with 24-bit ANSI colors and Unicode half-blocks (not a separate GUI VLC window).

> **Scope:** This is a runnable *proof-of-concept*, not full VLC feature parity. Its frame renderer is half-block TrueColor, rather than Kitty/Sixel high-resolution graphics. Performance depends heavily on terminal speed. It is an independent learning project; not affiliated with VideoLAN or SparkPlayer. No VLC or SparkPlayer source code is bundled.

## Prerequisites

* **Python 3.9+**, 64-bit if VLC is 64-bit. No third-party Python packages are required to run from source.
* **VLC 3.x / LibVLC 3.x** installed and accessible. Python's architecture must match VLC's architecture.
* An interactive terminal with UTF-8, VT/ANSI and preferably 24-bit RGB support. Windows Terminal, WezTerm, Kitty, Ghostty, and most modern Linux/macOS terminals work for this ANSI rendering mode.
* For a Windows desktop, use **Windows Terminal or WezTerm** with a reasonably large terminal window (at least 90x30 is recommended).

### Windows 10/11 (PowerShell in Windows Terminal)

1. Install **64-bit VLC 3.x** from [VideoLAN](https://www.videolan.org/vlc/download-windows.html). Also install Python 3.9+ if you don't already have it.
2. Extract this ZIP to any folder, then in a PowerShell tab:

```powershell
cd "C:\path\to\termvlc_project"
py -m termvlc --doctor
py -m termvlc ".\samples\sample.mp4"
```

To play your own video:

```powershell
py -m termvlc "D:\Media\Video\Movies\YourMovie.mkv" --fps 15
```

To browse a folder:

```powershell
py -m termvlc "D:\Media\Video\Movies"
```

If VLC was installed in a nonstandard directory, point the app to its DLL:

```powershell
$env:TERMVLC_LIBVLC_PATH = "C:\Program Files\VideoLAN\VLC\libvlc.dll"
py -m termvlc --doctor
```

To use the command `termvlc` globally, run `py -m pip install -e .` from the project directory. Then `termvlc "path\to\movie.mp4"` should work in that Python environment. This optional install may require setuptools/internet; direct `py -m termvlc` works without installation.

### Ubuntu/Debian Linux

```bash
sudo apt update
sudo apt install -y python3 libvlc5 vlc
cd /path/to/termvlc_project
python3 -m termvlc --doctor
python3 -m termvlc samples/sample.mp4
python3 -m termvlc "$HOME/Videos/movie.mkv"
```

If installed from system packages but `--doctor` reports missing LibVLC, inspect your distro's LibVLC library path with `ldconfig -p | grep libvlc`.

### macOS

Install VLC 3.x using the official [VLC macOS distribution](https://www.videolan.org/vlc/download-macosx.html); then:

```bash
cd /path/to/termvlc_project
python3 -m termvlc --doctor
python3 -m termvlc samples/sample.mp4
```

If loading fails, use `TERMVLC_LIBVLC_PATH=/Applications/VLC.app/Contents/MacOS/lib/libvlc.dylib` with `python3 -m termvlc --doctor`. Be sure the LibVLC binary and your Python interpreter have matching architectures.

## Playing videos

```bash
python3 -m termvlc samples/sample.mp4
python3 -m termvlc /home/you/Videos/movie.mkv --fps 15
python3 -m termvlc /home/you/Videos
python3 -m termvlc 'https://example.org/movie.mp4'
python3 -m termvlc 'rtsp://example.org/live'
python3 -m termvlc /home/you/Videos/movie.mkv --ascii
python3 -m termvlc /home/you/Videos/movie.mkv --decode-width 320 --fps 18
```

Use `py -m termvlc` on Windows. A high `--decode-width` increases VLC's CPU scaling load; high `--fps` increases terminal I/O bandwidth. Default is 224-pixel-wide decoded frames and 12 terminal refreshes per second.

### Controls

| Key | Action |
|---|---|
| `Space` | Pause/resume |
| `Left` / `Right` | Seek -5/+5 seconds |
| `+` / `-` | Volume up/down |
| `m` | Mute |
| `[` / `]` | Playback speed -/+0.25x |
| `s` | Cycle subtitle track, including off |
| `a` | Cycle audio track |
| `f` | Focus/fullscreen video within the terminal (hides library pane) |
| `Up` / `Down` | Navigate the library |
| `Enter` | Open directory / play selected file |
| `n` / `p` | Next/previous media file in current directory |
| `q` | Quit safely |

## Architecture

```text
termvlc/__main__.py   ── CLI / diagnostics
termvlc/app.py        ── file browser + keyboard controls + main loop
termvlc/terminal.py   ── cross-platform raw terminal session and input
termvlc/render.py     ── TrueColor ANSI half-block image compositor
termvlc/engine.py     ── ctypes bindings to LibVLC 3.x
                            │
                      libvlc.dll / libvlc.so.5 / libvlc.dylib
                            │
                 demux + decode + audio + A/V master clock
                            │
       libvlc_video_{format,lock,display}_cb
                            │
                   newest complete RV32 frame
                            │
                    ANSI half-block renderer
```

**Important:** Video data is delivered using `libvlc_video_set_callbacks()` and `libvlc_video_set_format_callbacks()`. VLC owns the audio output and media clock. Video pictures are **not** displayed from LibVLC's callback thread; the TUI loop draws only the most recently delivered frame and drops older pictures. Pixel buffers are 32-byte aligned and decode-scale to keep terminal rendering manageable.

The terminal's renderer has limited effective pixel resolution: one upper-half Unicode block can encode two color samples. This can look colorful and animated, but will not be as sharp as VLC in a GUI window. Native audio continues independently of terminal frame rate.

## Tests (developer)

```bash
python3 -m unittest discover -s tests -v
```

On Linux with GCC, the suite builds `tests/fake_vlc.c` into a temporary shared library that emulates the relevant LibVLC 3.x API signatures, injects a rendered test frame, tests playback controls and opens a **real PTY** to exercise the whole terminal application. The shim is **only for automated tests**, not a media decoder. Tests do not require system VLC.

**Verification performed for this packaged version:** Python syntax compilation, unit tests, LibVLC **ABI simulation with a C shim**, and interactive PTY rendering/shutdown all passed. The included `samples/sample.mp4` is a valid real H.264/AAC video generated and inspected using FFmpeg. However, **actual LibVLC playback on Windows/macOS/Linux was not run in the build environment** because LibVLC is not installed there. Please run `--doctor` and play the sample locally to verify your specific OS/VLC combination.

## Troubleshooting

* **Cannot load LibVLC**: Install 64-bit VLC 3.x, match Python architecture, or set `TERMVLC_LIBVLC_PATH` to `libvlc.dll`/`.so.5`/`.dylib`.
* **VLC could not initialize**: VLC plugins may be missing. On Windows, keep the `plugins` directory beside the installed LibVLC DLLs; the project sets `VLC_PLUGIN_PATH` automatically for recognized Windows installation paths.
* **Blank video with audible audio**: Try a local H.264/AAC MP4, a larger terminal, and `--decode-width 128 --fps 10`. Some formats/VideoLAN builds might not support memory callbacks the same way.
* **Terminal looks scrambled**: Use a modern terminal with UTF-8 and ANSI TrueColor support; test `--ascii` as a fallback. Don't redirect stdout to a file.
* **Video is choppy**: Decrease `--fps` (e.g. `--fps 8`) or widen the terminal less. LibVLC itself may remain synchronized while terminal picture updates are dropped.
* **Codec/plugin or VLC 4 compatibility issues**: This MVP targets the LibVLC **3.x** C ABI specifically; future LibVLC versions may require adapter changes.

## License and upstream references

The **new TermVLC code in this ZIP is MIT licensed**. VLC binaries and plugins are **not** bundled; their separate licenses remain applicable. SparkPlayer is GPL-2.0-only and its code is **not copied** here. See [SOURCE_NOTES.md](SOURCE_NOTES.md) for upstream source links and the reasons for an independent first implementation.
