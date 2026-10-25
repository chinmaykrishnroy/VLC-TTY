# Menu, timeline and subtitle regression checks

Local Windows x64 validation on October 25, 2026.

## Reproduced defects

- Switching from an open Playback menu to Audio or Video emitted a whole-screen erase. The presentation key included the selected menu identity, so every header switch reset both the terminal and the graphics cache. Those erases also happened before synchronized drawing began.
- Wheel events over the timeline reached the global volume handler instead of seeking.
- A real legacy-encoded SRT failed at UTF-8 byte offset 62,942. Its bytes decode as Windows-1252-compatible Western text; no subtitle content is included in the repository.

## Changes

Open menu header switches now use Ratatui's incremental text redraw. Required clears for pixel/text transitions and resizing are deferred into the synchronized redraw. Pixel encoders, spatial resolution and palette quality are unchanged.

Wheel-up/down over a visible timeline requests a relative seek of +10/-10 seconds. Volume adjustment elsewhere in the player and list scrolling retain their existing behavior. Open menus and dialogs consume wheel events so controls underneath are unaffected.

SRT/VTT loading tries Unicode BOMs and valid UTF-8 before a Windows-1252 fallback. Declared but malformed Unicode is rejected, and reads remain bounded to 5 MB. This is not general legacy-language encoding detection; other legacy encodings should be converted to UTF-8.

## Checks

- `cargo test --locked --all-targets`: 115 passed across the player, portable launcher and CLI; six opt-in tests excluded.
- `cargo clippy --locked --all-targets -- -D warnings` and formatting checks passed.
- Python regressions: 15 passed, one platform-specific test skipped.
- Native Windows ConPTY `menu-hover` and `menu-hover-sixel`: correct menu text after header switches, no whole-screen erase between open headers, synchronized opening/closing clears, no Sixel overwrite of menus, clean quit and terminal restoration.
- Native fullscreen smoke: startup, both resize directions, image containment, Escape, quit and terminal restoration passed.
- A private ConPTY check dropped the reported SRT into the player and verified its selection in media information. Real LibVLC playback then sought forward/backward using mouse-wheel reports on the timeline, retaining its silent volume state and restoring the terminal on exit.

The Windows menu checks run in CI on both x64 and ARM64. Their text projection accounts for incremental console writes rather than requiring complete labels in each output chunk. They verify console commands and resulting text; these checks do not measure monitor-visible FPS or physical audio/video synchronization.

## Bundled builds and remaining observation

All eight jobs passed for source commit `95e0e9b61951d6666e1dd608565486f0d8d41c20` in [CI run 40197769374](https://github.com/chinmaykrishnroy/VLC-TTY/actions/runs/40197769374), including real LibVLC file/HTTP playback and the Windows console regressions on both architectures.

The Linux ARM64 standalone passed the full portable verifier on the physical Pi 5: matching bundled runtime, concurrent launches, decoded frame/clock progress, extraction checks, SIGTERM cleanup and terminal restoration. Its 12-second interactive PTY check passed pause, help, queue, captions and quit. Both `/usr/local/bin/vlc-tty` and `/home/roy/.local/bin/vlc-tty` were atomically replaced with backups retained and SHA-256 `fd1c68521f8a1797a6781adf4b1696a8df91a21bac6fadfe9929d7b1bede1c73`. The installed command also passed a decoding smoke check.

The Windows x64 CI artifact was checked against GitHub's archive digest, its embedded clean source identity and its executable checksum. SHA-256 `4fc7a3c687a8f8d2a6eff96b2589e84ee0416e9d1ef3b6a55b124d74625c3b2d` is installed at `C:\Users\morph\AppData\Local\Programs\VLC-TTY\vlc-tty.exe`, `target\release\vlc-tty.exe` and `target\release\termvlc.exe`. Backups were retained and the user PATH entry was confirmed. Its help includes timeline wheel seeking. The actual standalone also passed ANSI/Sixel menu switching and the reported SRT/timeline check.

Some local Sixel quit checks exceeded their 10-second process-exit deadline; a subsequent standalone check exited and restored the terminal before cleanup began. The cause of those intermittent timeouts is not established or claimed fixed.

The default local Windows portable verifier did **not** pass end to end: its stalled-HTTP lifetime fixture did not receive a request within 35 seconds. A diagnostic confirmed that the launcher was still alive at that cutoff and subsequently reached the endpoint during a bounded extra wait. A full retry allowing 90 seconds for that fixture instead stopped earlier at the existing 10-second local-file decoding deadline (`Opening`, no frames), despite the earlier successful bundled decoding launch. Consequently, the clean CI result and focused interactive checks above must not be read as a clean local cold-launch reliability result. No release gate was relaxed and no production startup/shutdown fix is claimed here.

The isolated Windows Job Object check subsequently passed with the 90-second startup allowance: the endpoint was reached, forced supervisor termination stopped the extracted child, and cleanup completed. This does not change the failed end-to-end verifier result above.

The local debug HTTP suite was intermittent: isolated decoding and artwork checks passed, but some combined runs missed HTTP decoding or artwork. The same missing-artwork failure reproduced in the retained pre-fix test binary. These observations are not treated as a fix or a clean local HTTP reliability result; CI's matching-runtime checks passed. Longer remote-stream reliability remains a separate validation task.
