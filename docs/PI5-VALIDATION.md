# Raspberry Pi 5 validation — 17 October 2026

## Portable 0.0.1 follow-up

The same device built commit `e9a3264` into one executable embedding LibVLC 3.0.23 and its distro library/plugin closure. The file was 79,565,928 bytes, SHA256 `8437aca587ff82518d3fc3c8e99605a2b9bec284d879c8175d3b15eb565cbecb`. This locally built file uses the device's base-OS library baseline; release CI builds Linux files on Ubuntu 24.04 instead.

Two concurrent invocations loaded LibVLC from separate private extraction directories while external runtime paths were invalid. Actual silent decoding advanced the clock and produced frames. Help, identity, extraction, component hashes, documentation and normal-exit cleanup passed. Sending SIGTERM to the public launcher stopped its child, restored the terminal and removed the runtime directory.

A 30-second ANSI PTY run with the same synthetic 720p/30 FPS source passed pause/redraw, help/queue, caption, reopen, resize and exit checks. It captured 3,333,140 bytes and 112,731 half-block characters. Summed peak RSS for launcher plus player was 235.39 MiB; average CPU was 68.2% of one core. The updated driver samples the process tree: the first standalone measurement sampled only the launcher and was discarded for performance comparisons. Summed RSS can count shared pages twice; this is not unique physical memory usage or a movie-length benchmark.

## Earlier thin build: device and environment

- Raspberry Pi 5 Model B Rev 1.0, 8 GiB RAM.
- Ubuntu Server 26.04 LTS, ARM64, kernel `7.0.0-1020-raspi`.
- Distro LibVLC 3.0.23 with `libvlc5` and `vlc-plugin-base`, installed without recommended desktop packages.
- Rust 1.98.1; native release build with two compiler jobs.
- Application code at `087caf3`, unchanged in `1f2a8c1` (the latter adjusts only the PTY driver).
- No `DISPLAY` or `WAYLAND_DISPLAY`; silent SSH profile, 15 FPS limit, 720p callback cap.

The Linux ARM64 CI archive at `50a51e0` was downloaded, its artifact/archive hashes verified, transferred and extracted on the Pi. It loaded the distro runtime and decoded frames with an advancing clock. Interactive testing then exposed the cursor-query defect described below; the corrected source was built and tested natively on this device.

## Interactive regression

`scripts/pty_smoke.py` drives the real application in a Unix pseudo-terminal. A synthetic 60-second H.264 video was generated with FFmpeg (`testsrc2`, 1280×720, 30 FPS, square pixels, no audio), with a local SRT caption. No private media was used.

The 45-second run exercised fullscreen, pause, frame step, resume, seeking, help open/close, queue view, reopening the same file, terminal resizing and `q`. Terminal sizes changed from 100×32 to 120×36 to 80×24. Reported checks:

| Check | Result |
| --- | --- |
| Process exit | 0 |
| ANSI video writes | 149,256 half-block characters |
| Paused redraw | Passed |
| Help and queue views | Passed |
| Caption text | Present |
| Alternate-screen restoration | Passed |
| Captured terminal output | 4,569,024 bytes |
| Peak player RSS | 157.82 MiB |
| Average player CPU | 66.18% of one CPU core |
| Temperature after run | 46.3°C |

CPU/RSS were sampled from the player process in `/proc`; these are observations for this synthetic test, not a performance guarantee. The byte count was collected from the PTY on the Pi. It is not measured network throughput or a client display-frame-rate measurement. This short run includes pauses, menus and reopening; movie-length stability remains unverified.

## Direct SSH transport

A second run launched the actual player through `ssh -tt` into a Windows PowerShell pseudo-console using `--ssh --graphics auto`. ANSI video reached the client. Fullscreen/pause input worked, `q` returned exit code 0, and the alternate-screen restoration sequence was observed. This validates transport and control behavior in that session, not pixel-level visual quality or all PowerShell terminal hosts.

The tested executable was installed at `~/.local/bin/vlc-tty`. Its SHA256 is `8e4f4ae782580b416fe1fc870ca56ad786b29aa7c7aabff4b0323501f25ba021`. The installed executable passed diagnostics and silent decoding again with the synthetic 720p clip.

## Defect found and fixed

The original interaction run exited with `The cursor position could not be read within a normal duration`. Ratatui's `Terminal::clear()` queries/restores the cursor even in a fullscreen viewport. The PTY intentionally sends no cursor-report response, so a presentation reset blocked and consumed subsequent input before failing.

The application now resets its fullscreen viewport to the current terminal size, which clears/resets buffers without querying an invisible cursor. The same interaction sequence passes after this change. Ubuntu 26.04 ARM64 CI requires the PTY regression, including its paused-state assertion after startup notifications expire.

## Remaining acceptance

Kitty, Sixel and iTerm2 pixel display quality, tmux/client combinations, embedded-caption ordering, audible output, weak networks, higher-resolution/codec workloads and long sessions still require separate testing. The initial supported use case remains silent terminal playback. See [validation](../VALIDATION.md) and the [SSH guide](SSH.md).
