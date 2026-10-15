# Upstream study and the design decision

Repositories researched:

- SparkPlayer (GPL-2.0-only): https://github.com/dividebysandwich/sparkplayer
  - `crates/sparkplayer-core/src/backend.rs`: separate AudioBackend and VideoBackend traits
  - `crates/sparkplayer-native/src/backends.rs`: ratatui-image terminal image rendering
  - `crates/sparkplayer-native/src/video.rs`: independent threaded FFmpeg frames
  - `crates/sparkplayer-core/src/app.rs`: audio-master video scheduling
- VideoLAN VLC / LibVLC:
  - Official source: https://code.videolan.org/videolan/vlc
  - GitHub mirror: https://github.com/videolan/vlc
  - C API: https://videolan.videolan.me/vlc-3.0/group__libvlc__media__player.html
  - Public API headers: https://github.com/videolan/vlc/blob/master/include/vlc/libvlc_media_player.h
  - Embedding docs: https://github.com/videolan/vlc/blob/master/doc/standalone/libvlc.md

## Findings

SparkPlayer has a separable TUI and media backend, but its playback uses both a Rodio audio backend and a distinct FFmpeg video decoder. A correct LibVLC replacement needs a **single shared playback session** controlling both audio and video to avoid duplicate decoding and competing clocks. The first MVP here implements that shared playback session as a standalone module, without copying any GPL-only SparkPlayer code. Its text UI takes inspiration from the existence of a file-browser/video-pane layout, not implementation text or assets.

LibVLC provides audio, seeking, track control and the relevant in-memory callbacks needed to render RV32 frames in a terminal. LibVLC memory output can have extra CPU cost and limited hardware decoding. TUI performance is thus an independent challenge.

**Clone status:** The build container cannot resolve GitHub / code.videolan.org DNS. Full git clones could not be performed in this environment. Relevant upstream files were inspected via the GitHub connector and official LibVLC documentation. An optional script (`scripts/clone-upstreams.sh`) is included to fetch both full codebases on a machine with network access. Cloning sources is *not* needed to run TermVLC; it loads an installed LibVLC runtime.

## Future roadmap

1. Replace the minimal Python UI with a standalone Rust/Crossterm/Ratatui front-end, preserving the LibVLC integration API and frame queue protocol.
2. Add Kitty/Sixel image-protocol backends for dramatically higher terminal graphics fidelity.
3. Use a bounded frame queue and measuring/rendering threads for tighter latency and higher FPS.
4. Add VLC stream discovery, media-info panels, per-track subtitle selection, chapters, resume history and playlist persistence.
5. Create reproducible automated builds and tests on real VLC 3.x for Windows/Linux/macOS.
6. Explore embedding LibVLC 4 behind a versioned adapter, subject to its callback ABI.

This project is not a completed SparkPlayer+VLC source merge. It is an independently authored first integration prototype demonstrating a VLC media engine and terminal video rendering. It is meant to serve as a validated starting point for that fuller undertaking.
