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
