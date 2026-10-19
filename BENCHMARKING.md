# Playback measurements

Run `vlc-tty --metrics run.jsonl --silent movie.mp4` to record bounded, once-per-second server metrics. The output must be a new file. Recording works on Windows, Linux, and macOS; it is opt-in.

The counters distinguish LibVLC video display callbacks, picture allocations, bytes copied, encoder requests/completions, replaced pending work, rejected stale results, unique frames submitted to the terminal backend, and backend bytes written. Encoding, worker queue wait, and draw durations report the 95th percentile of the most recent 256 samples. Callback counts are not LibVLC's internal decoded-picture statistics. Backend bytes omit terminal setup, synchronization markers, SSH overhead, and some protocol cleanup writes. The final sample can precede exit by up to one second.

## Reproducible server stress test

On a Unix host with Python 3 and an existing media file:

```sh
python3 scripts/benchmark_playback.py ./target/release/vlc-tty movie.mp4 \
  --renderer kitty --duration 1800 --minimum-frames 6000 \
  --fps 15 --decode-height 720 --output benchmark-kitty
```

Repeat for `ansi`, `sixel`, and `iterm2`. Each run needs a new output directory. The driver loops the media, drains a PTY, probes help responsiveness every 30 seconds, checks shutdown/terminal restoration, and records Linux process-tree RSS and CPU. Summed RSS can double-count shared pages. It fails on missing progress, decoder errors, encoding errors, insufficient frames, or failed shutdown. Keep the binary commit, media codec/dimensions, machine model, and command alongside the results.

For a shorter control smoke, use `--duration 30 --probe-interval 3`. When the run lasts at least five seconds beyond its probe interval, acceptance requires a successful help response. CI keeps the ten-frame minimum and uses this longer window for its debug builds; a twelve-second debug Sixel run could finish ten encodes but sample only nine displayed frames, and never reached the former thirty-second keyboard probe.

To measure the negotiated Kitty zlib path, add `--kitty-compression --renderer kitty`. This makes the PTY sink explicitly emulate successful Kitty capability replies. Acceptance also requires an observed compressed image transfer. Without that option the sink sends no capability replies, exercising timeout and uncompressed compatibility. Emulation does not establish that an actual terminal supports or presents those frames correctly.

Run comparisons **serially** on the same machine, binary, media and terminal dimensions. Keep compressed and uncompressed results separate; compression changes CPU cost as well as output size. Kitty compression is negotiated automatically by the application: unsupported, rejected or unanswered probes retain raw inline transfer. Use the player option `--no-kitty-compression` to prefer lower CPU cost on a fast connection. It also works after the file argument in `stream` or `connect`.

For a standalone encoder microbenchmark (generated gradient and noise, no decoder or terminal), run:

```sh
cargo test --locked --release --bin vlc-tty kitty_encoding_benchmark -- --ignored --nocapture
```

It reports mean encoding time and protocol bytes for raw and compressed transfers. This isolates encoder costs; use playback runs for pipeline behavior.

## Longer runs in Actions

The manual **Terminal pipeline stress (server metrics)** workflow accepts a 30, 60 or 120 minute duration, x64/ARM64 Linux runner, renderer and H.264/H.265/AV1 codec. It builds the headless release player, generates a ten-second synthetic 720p clip and loops it. Artifacts include the commit, fixture hash and codec details, per-second metrics, resource samples, diagnostic tail and summary. These cloud runs complement the physical Pi results.

The workflow requires at least 6,000 frames, progress throughout the run, help responses and clean shutdown. `--maximum-rate-drop 0.25` also rejects a median encoding or submitted-frame rate decrease of more than 25 percent after 5,000 frames. At least twenty samples are required in each phase; insufficient evidence fails that gate. This is a relative regression limit, not a promise of 15 FPS. Memory usage remains a reported measurement rather than an inferred leak verdict.

Use the same options on a physical device. Synthetic loops exercise repeat transitions but do not replace diverse full-length media, a real terminal, or externally measured A/V alignment.

Results include application metrics, per-second resource samples, a summary, and a bounded diagnostic tail. The summary compares median rates and latency before and after 5,000 frames when enough samples exist. It does not infer success from the requested FPS. A short smoke run cannot establish movie-length stability.

## What this does not measure

A drained PTY is not a terminal graphics client. These tests exercise decoding, conversion, encoding, and server writes, including forced pixel protocols. They do not measure client-visible FPS, image quality, SSH wire throughput, display latency, or audible A/V synchronization.

For visual acceptance, run the same media in a real supported terminal over SSH for 30–120 minutes, including pause, seek, resize, fullscreen, and track changes. Record the terminal/version, connection, actual visible frame pacing, and external A/V offset measurements. Keep server stress results and client presentation evidence separate.
