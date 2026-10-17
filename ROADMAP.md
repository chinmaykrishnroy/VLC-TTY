# Roadmap

The release gate is a usable terminal player, including silent SSH playback. Checkboxes describe implementation status, not device certification. See [validation](VALIDATION.md) for evidence.

## Prototype SSH foundation (completed before public 0.0.1)

- [x] Separate queue state from directory browsing and playback operations from UI controls.
- [x] Fix volume, subtitle selection, delay units, overlapping captions and input handling.
- [x] Add silent mode, SSH defaults, diagnostics and a terminal-free decoding check.
- [x] Bound pending graphics work; reject stale results after media/layout changes.
- [x] Reuse Kitty image storage and fix Sixel passthrough through tmux.
- [x] Reserve space for text captions outside pixel graphics.
- [x] Pass native CI, runtime decoding and extracted-archive checks across the release matrix.
- [x] Verify Pi 5 / Ubuntu Server 26.04 headless decoding, ANSI controls and direct SSH transport.
- [ ] Complete pixel-protocol visual acceptance in the intended client terminal.
- [ ] Measure movie-length responsiveness, rendering throughput and resource usage beyond the initial 45-second synthetic check.

## Public 0.0.1

- [x] Single executable packaging with LibVLC and matching plugins for six native targets.
- [x] UTF-8 M3U import/export, shuffle, repeat and startup volume/speed flags.
- [x] Contribution/release guides, comprehensive help and architecture checks.
- [ ] Complete source correspondence for the Windows/macOS runtime before publication.
- [ ] Complete current native, launcher-lifetime and physical-device regression gates.

## Next usability milestones

- Interactive queue editing and saving the current queue.
- Saved settings, recent media and resume positions.
- Better stream buffering/error reporting and reconnect behavior.
- More subtitle formats, styling and embedded-track rendering validation.
- Terminal-specific visual acceptance and automated PTY coverage.

## Later exploration

Audio forwarding with an explicit transport and synchronization design; measured Pi acceleration; more Linux architectures. Keep portable executables as the default distribution; consider an installer only if later integration features justify one.
