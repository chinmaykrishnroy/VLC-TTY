# Roadmap

The release gate is a usable terminal player, including silent SSH playback. Checkboxes describe implementation status, not device certification. See [validation](VALIDATION.md) for evidence.

## 0.6 — SSH foundation

- [x] Separate queue state from directory browsing and playback operations from UI controls.
- [x] Fix volume, subtitle selection, delay units, overlapping captions and input handling.
- [x] Add silent mode, SSH defaults, diagnostics and a terminal-free decoding check.
- [x] Bound pending graphics work; reject stale results after media/layout changes.
- [x] Reuse Kitty image storage and fix Sixel passthrough through tmux.
- [x] Reserve space for text captions outside pixel graphics.
- [ ] Pass native CI, runtime decoding and extracted-archive checks across the release matrix.
- [ ] Validate Pi 5 / Ubuntu Server 26.04 / actual SSH client terminal.
- [ ] Measure sustained responsiveness, rendering throughput and resource usage.

## Next usability milestones

- Queue editing, shuffle and playlist import/export.
- Saved settings, recent media and resume positions.
- Better stream buffering/error reporting and reconnect behavior.
- More subtitle formats, styling and embedded-track rendering validation.
- Terminal-specific visual acceptance and automated PTY coverage.

## Later exploration

Audio forwarding with an explicit transport and synchronization design; measured Pi acceleration; runtime-inclusive installers after component/license inventory; more Linux architectures. These depend on evidence and demand.
