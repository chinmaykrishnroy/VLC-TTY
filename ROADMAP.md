# Roadmap

The release gate is a usable terminal player for local media and remote files, with headless server rendering available over SSH. Checkboxes describe implementation status, not device certification. See [validation](VALIDATION.md) for evidence.

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
- [x] Record an 11-minute Pi encoder stress run beyond 5,000 frames for Kitty and Sixel, with separate throughput and memory measurements.
- [ ] Resolve Kitty's missed throughput target and measure movie-length responsiveness in real client terminals; PTY sink results do not establish visible FPS.

## Public 0.0.1

- [x] Single executable packaging with LibVLC and matching plugins for six native targets.
- [x] UTF-8 M3U import/export, shuffle, repeat and startup volume/speed flags.
- [x] Contribution/release guides, comprehensive help and architecture checks.
- [ ] Complete source correspondence for the Windows/macOS runtime before publication.
- [x] Pass six native standalone lanes, source collection, launcher-lifetime checks and physical Pi bundled-runtime/PTY tests.

## Next usability milestones

- [x] Stream original remote files through SSH with local LibVLC decoding, audio and range seeking.
- [x] Load streamed MP3 tags and embedded covers through bounded background range reads; display readable URL filenames.
- [x] Implement authenticated PCM forwarding and a local receiver through the `connect` helper.
- [ ] Validate server-rendered A/V alignment, jitter handling, device loss and movie-length audio continuity.
- [ ] Measure Kitty transfer/encoding costs and compare serial benchmark baselines before selecting optimizations.
- Interactive queue editing and saving the current queue.
- Saved settings, recent media and resume positions.
- Better stream buffering/error reporting and reconnect behavior.
- More subtitle formats, styling and embedded-track rendering validation.
- Terminal-specific visual acceptance and automated PTY coverage.

## Later exploration

Adaptive audio buffering and reconnect/resume; measured Pi acceleration; more Linux architectures. Keep portable executables as the default distribution; consider an installer only if later integration features justify one.
