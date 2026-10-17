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

Results include application metrics, per-second resource samples, a summary, and a bounded diagnostic tail. The summary compares median rates and latency before and after 5,000 frames when enough samples exist. It does not infer success from the requested FPS. A short smoke run cannot establish movie-length stability.

## What this does not measure

A drained PTY is not a terminal graphics client. These tests exercise decoding, conversion, encoding, and server writes, including forced pixel protocols. They do not measure client-visible FPS, image quality, SSH wire throughput, display latency, or audible A/V synchronization.

For visual acceptance, run the same media in a real supported terminal over SSH for 30–120 minutes, including pause, seek, resize, fullscreen, and track changes. Record the terminal/version, connection, actual visible frame pacing, and external A/V offset measurements. Keep server stress results and client presentation evidence separate.
