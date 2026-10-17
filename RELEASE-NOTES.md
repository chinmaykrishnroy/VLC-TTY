**Draft — unpublished.** These notes prepare v0.0.1; no release is published while the source-correspondence gate remains unresolved. VLC-TTY remains the working name; displaying the requested VLC Media Player title is not a claim of VideoLAN affiliation or trademark clearance.

The application is a terminal-first media player using LibVLC and Ratatui, including silent video playback on a headless server over SSH.

### Download

Download **one executable** for your OS and CPU. No installer or separate VLC installation is required.

| Machine | File |
| --- | --- |
| Intel/AMD Windows PC | `vlc-tty-0.0.1-windows-x64.exe` |
| ARM64 Windows device | `vlc-tty-0.0.1-windows-arm64.exe` |
| Linux x64 | `vlc-tty-0.0.1-linux-x64` |
| Raspberry Pi / Linux ARM64 | `vlc-tty-0.0.1-linux-arm64` |
| Intel Mac | `vlc-tty-0.0.1-macos-x64` |
| Apple Silicon Mac | `vlc-tty-0.0.1-macos-arm64` |

On Linux/macOS, make the file executable with `chmod +x FILE`. Linux requires glibc 2.39+ (Ubuntu 24.04 or newer). The embedded runtime extracts to a private temporary directory while the app runs. Unsigned binaries may encounter platform security prompts.

Run `vlc-tty --help`. For SSH, allocate a terminal: `ssh -t user@server 'vlc-tty --ssh /path/to/movie.mp4'`. Rename the downloaded file to `vlc-tty` if desired. SSH defaults to silent playback, 15 FPS and a 720p callback cap; it does not forward sound.

### Included

- Local files and network URLs; pause, seek, stop, speed and frame stepping.
- File browser, M3U playlists, export, shuffle and repeat.
- SRT/VTT captions, track menus and timing controls.
- ANSI video fallback plus Kitty, Sixel and iTerm2 graphics options.
- Diagnostics, opt-in server profiling, real decoding smoke checks and supervised runtime cleanup.
- Experimental `connect HOST MEDIA` audio forwarding through authenticated loopback SSH channels and a local CPAL receiver. Requires a matching server build; measured lip-sync and 60-minute A/V acceptance remain open.
- `stream HOST FILE` transfers original compressed media over SSH into local LibVLC, with local sound and byte-range seeking. Requires Python 3 on a Linux/macOS server and currently opens one regular file.
- Forwarded audio volume uses explicit requested gain; receiver regressions cover packet jitter and early device callbacks instead of accepting startup-only audio evidence.
- Fullscreen uses the complete terminal grid when text overlays are hidden. Host profile padding and aspect-ratio bars remain separate settings/behavior.

### Validation and limits

All six native targets require runtime decoding and standalone checks before publication. Windows x64 and a physical Pi 5 on Ubuntu Server 26.04 also passed terminal interaction checks. Long sessions, all pixel-protocol/client combinations and audible output remain open acceptance work; this preview does not claim desktop VLC feature parity.

### Runtime licenses and sources

Application sources retain their MIT grant. Bundled VLC, plugins and codecs include GPL/LGPL and other component terms; this portable distribution is not MIT-only. Embedded notices are accessible with `--extract-to NEW_DIRECTORY`, and `VLC_TTY_RUNTIME` permits runtime replacement.

Source companions and `SHA256SUMS` accompany the executables. Windows/macOS use hash-verified unmodified official VLC 3.0.24 runtime archives. Their source snapshot preserves the hash-verified original VLC source archive, build recipes/patches and checksum/git-pinned contributed dependency sources. Linux companions preserve exact distro source package versions.

Exact independent Windows/macOS binary-to-contrib-source correspondence remains unverified and is recorded in build manifests. The collected source snapshot is not a completed correspondence or reproducible-build certification. Publication, including prereleases, requires the strict correspondence gate. Source archives are for inspection/rebuilding and are not required for playback.
