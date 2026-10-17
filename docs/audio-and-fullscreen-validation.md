# Audio and fullscreen development checks — October 17, 2026

These are source-build development checks, separate from the previously verified CI standalone executables. They do not establish client-visible frame rate, audible sound quality, physical A/V offset, or movie-length acceptance.

- Windows x64: 80 deterministic Rust tests passed (75 player, 2 portable, 3 CLI), plus all four real LibVLC runtime tests. Strict Clippy passed with and without the audio-client feature. Twelve Python tooling tests and actionlint passed.
- The fullscreen regression failed with the old permanent two-row reservation, then passed with the fix. It renders matching-aspect-ratio video into 80×24, 101×33 and 24×8 TestBackends and checks all four corner cells. Host profile padding/scrollbar pixels are outside those grids.
- Pi 5 Ubuntu 26.04 ARM64: headless release build and real LibVLC PCM capture/authentication/seek-generation regression passed without CPAL or local audio devices.
- Windows default CPAL/WASAPI receiver: generated 48 kHz stereo PCM ran for five seconds, emitted 52 telemetry reports and shut down normally. The test did not record a microphone or certify audibility.
- Pi → Windows default receiver through an SSH reverse tunnel: a 30-second H.264/AAC fixture exercised pause, frame step, seek, help, queue, reopen, resize and normal quit. Server PTY exit was zero and terminal restoration passed. The final sample showed **zero callback drops**, with no new underrun callbacks from 10.6 to 29.4 seconds after the control sequence. Initial/interaction underruns remained visible (77 total); this is not glitch-free acceptance. Server summed peak RSS was 202.18 MiB and average CPU 68.13% of one core.
- The Windows `connect` helper started a Pi player through OpenSSH in a ConPTY, forwarded video bytes and local audio, and exited zero after `q`. This checks process/tunnel/input lifecycle, not a visual Windows Terminal graphics session.
- Forced Kitty and Sixel PTY-sink runs passed for 40 seconds after fixing the timed-out startup probe. A longer run then exposed help being dismissed by a natural repeat transition; its regression was reproduced and fixed. Longer post-fix runs are tracked separately and must finish before claims about the 5,000-frame threshold.

Reproduce local device checks with `python scripts/audio_receiver_smoke.py target/debug/vlc-tty.exe --output new-report.json`. For a Pi test add `--ssh-host roy@pi --remote-binary /absolute/player --remote-media /absolute/movie.mp4 --remote-driver /absolute/scripts/pty_smoke.py --duration 30`. Both machines need matching development builds. Tests create temporary private keys and remove them; no key belongs in a report.

The receiver estimates presentation time using CPAL callback/playback timestamps. The server keeps a bounded history of at most 12 pending video frames / 24 MiB plus the shown frame, clamps extra delay to 250 ms, and subtracts recent encoder time. Seeking/pause/media changes discard previous generations. These are estimates: feedback transport delay and the terminal's actual presentation delay are not independently measured. Adaptive jitter, network impairment, real-terminal lip-sync, device switching/reconnect, Linux/macOS device output and the requested 60-minute A/V gate remain open.

Public publication remains blocked by strict Windows/macOS runtime source-correspondence verification. There is no preview bypass.

## Longer pixel-encoder stress result

Two independent server PTY sinks ran concurrently on the Pi for 660 seconds using the synthetic 720p fixture and a requested limit of 15 FPS. Both restored the terminal and answered 21 help probes. Neither emitted an encoding error.

| Encoder | Encoded frames | Gate | Median encode rate before / after 5,000 frames | Median RSS before / after |
| --- | ---: | --- | --- | --- |
| Sixel | 8,996 | Passed the 6,000-frame minimum | 13.74 / 13.86 frames/s | 163.79 / 164.49 MiB |
| Kitty | 5,859 | Failed the 6,000-frame minimum | 9.31 / 9.37 frames/s | 256.06 / 217.53 MiB |

The Kitty test therefore does not receive an overall pass. Its raw protocol output was about 17.5 MB/s versus about 1.6 MB/s for Sixel, and concurrent load matters. The observed rates do not show a slowdown at 5,000 frames in these runs. They are encoded-server rates, not terminal-visible FPS or SSH-wire throughput. This remains an 11-minute synthetic test, not movie-length or client-graphics acceptance.

The locally bundled Windows x64 executable at checkpoint bf26250 passed architecture/import checks, concurrent private runtime loads, actual bundled LibVLC 3.0.24 decoding, forced-supervisor child cleanup, extraction, notice/docs inventory and temporary-runtime cleanup. Its CPAL receiver also passed the generated-PCM device smoke; the portable connect helper accepted fullscreen/pause/quit in a ConPTY and exited zero. Its installed local VLC runtime has no verified upstream archive marker, so this locally built file is not a source-provenance-cleared public release.

A subsequent 75-second Pi-to-bundled-Windows-receiver run crossed the fixture's natural repeat boundary. It processed 8.55 MB of terminal output with only a bounded 4 MiB diagnostic tail, retained all interaction assertions, restored the terminal, and exited zero on both ends. The final sample still had zero callback drops; 78 underrun callbacks were recorded in total. This adds repeat/drain and bounded-harness coverage, not a 60-minute or audible lip-sync pass.
