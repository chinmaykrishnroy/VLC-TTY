# Changelog

## 0.0.1 (first public version, in development)

- Start public semantic versioning at 0.0.1; earlier numbers describe prototype checkpoints.
- Add portable self-extracting executables with embedded LibVLC and full help.
- Validate executable CPU architecture before native smoke tests.
- Add contributor and release documentation.
- Add UTF-8 M3U playlist import/export, shuffle, repeat, initial volume and speed options.
- Preserve duplicate playlist occurrences during navigation and shuffle restoration.
- Statically link the Windows C runtime and supervise the extracted player's lifetime.
- Preserve portable help with combined options and gate publication on verified runtime source provenance.
- Report missing media paths before starting the terminal or decoder; exercise real CLI playlist export without either dependency.
- Add opt-in server profiling and reproducible PTY benchmarks with separate callback, encoding, draw and byte counters.
- Add experimental authenticated PCM forwarding, a CPAL receiver and an OpenSSH connect helper; keep direct SSH audio limitations explicit.
- Pace audio on a network worker, invalidate old generations, and estimate receiver presentation timing with a bounded video history.
- Remove permanent fullscreen OSD/caption bands while overlays are hidden; document host terminal padding separately.
- Avoid background terminal probes stealing keyboard input after timeout, and preserve help/browser context across natural queue transitions.


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
