# Changelog

## 0.6.0 (development)

- Rename application and executable to VLC-TTY; add original terminal-cone SVG and focused product documentation.
- Add silent SSH defaults, bounded decode height/FPS, runtime diagnostics and a terminal-free decoding smoke command.
- Replace queued graphics work with latest-frame mailboxes; reject stale results across media/layout changes and reuse one owned Kitty image ID.
- Correct tmux Sixel escaping, reserve text-caption space and reject overflowing subtitle timestamps.
- Avoid blocking cursor-position queries during fullscreen, popup and resize redraws; require an actual Unix PTY control regression in CI.
- Add six-target native CI, required LibVLC decode checks, application archives with notices/checksums and a gated tag-release workflow.
- Remove unused still-image codecs from the terminal graphics dependency tree and apply consistent Rust formatting.

## 0.5.0 (source candidate)
- VLC-inspired ANSI traffic cone and refreshed controls.
- Fullscreen video with clickable back target, clearing/restoring terminal layout.
- External SRT/WebVTT subtitle parsing, discovery and text overlay with delay control.
- Native LibVLC audio/subtitle track selection and advanced key/menu controls.
- Mouse seeking, file browsing and double-click playback, dialogs for media and subtitle paths.
- v0.4 graphics protocols, cross-platform dynamic VLC loading, native window retained.
- Tests added for subtitle timing, fullscreen state, hit testing and video layout.

## 0.4.0
- Cross-platform Windows/Linux/macOS source architecture.
- Kitty, Sixel, iTerm2 image protocols, ANSI fallback, native-window mode.
