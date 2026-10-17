# Development validation — 17 October 2026

This is a development build of the reliability foundation, not a declaration of VLC feature parity or production readiness.

## Verified on this Windows workspace

- Installed runtime: **LibVLC 3.0.24**, loaded dynamically from the normal VLC installation.
- **45 tests passed**, including the explicitly enabled runtime tests.
- Strict Clippy passed: `cargo clippy --offline --locked --all-targets -- -D warnings`.
- Release build passed: `cargo build --offline --locked --release`; the rebuilt executable's `--help` and `--doctor` commands both succeeded.
- Local-file and loopback-HTTP playback tests require actual frames and advancing playback time. An Opening/Playing state alone is insufficient.
- The local runtime test checks pause, frame stepping while paused, seeking, stop, and audio/subtitle delay round-trips in microseconds.
- Deterministic regressions cover volume, subtitle cycling/overlap, metadata encoding, keyboard event preservation, mouse menu effects, fullscreen double-click behavior, queue navigation/advancement/repeat, and failed backend operations.
- The original seven new regression cases were run before their fixes: **25 passed, 7 failed**. Their specific failures were corrected before running the full suite.

The old HTTP test passed in isolation during this work. Its earlier failure was not traced to a proven production defect. Its fixture now supports HEAD and byte ranges, cleans itself up even on failure, and its playback assertion requires decoded progress instead of a state number.

## Reproduce

```powershell
cargo test --locked --all-targets
cargo test --locked --all-targets -- --include-ignored --test-threads=1 --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
.\target\release\termvlc.exe --doctor
.\target\release\termvlc.exe --graphics auto .\samples\sample.mp4
```

The default unit suite explicitly reports two ignored runtime tests, so it works on machines without VLC. The second command enables them, requires LibVLC, and fails if it cannot load. The Windows CI workflow installs VLC and runs these checks separately. The updated remote workflow has not been executed from this workspace.

Runtime tests select **dummy audio output** so decoding/control verification does not depend on an audio device or produce sound. They do not establish audible synchronization or output quality.

## Manual acceptance still required

1. Test real audio output and lipsync on the intended device. Exercise audio track changes and `j`/`k` synchronization.
2. Test each supported terminal graphics protocol and ANSI fallback. Check subtitles over video, OSD/menu image ordering, resize, and fullscreen transitions.
3. Open two media files in the same folder, inspect the queue with Tab, browse another folder through View → Browse Files, and confirm Next still follows the original queue. Check repeat Off / All / One and end-of-media advancement.
4. Try pause/resume, frame step, seeking, speed, stop/replay, malformed media, invalid URLs, and files with Unicode paths and metadata.
5. Check native-window output and subtitle loading after playback begins. This remains a separate video window rather than an integrated desktop frontend.
6. Validate long sessions, large video frames, live streams, clean-machine packaging, and Linux/macOS runtime behavior.

For terminal fullscreen, `f`/F11 changes the application layout. TermVLC no longer injects global F11 keypresses; use the terminal host's own controls for OS-window fullscreen.

## Next milestone

The user confirmed the product is a terminal video player. Keep Ratatui, terminal image protocols and LibVLC as the primary architecture. Prioritize measured rendering latency/frame pacing, image/text ordering for subtitles and menus, responsive input/resizing, and real audio/video synchronization in supported terminals. Then add queue editing/import/export, shuffle, saved settings and resume positions. The existing separate native-window mode remains an optional fallback.
