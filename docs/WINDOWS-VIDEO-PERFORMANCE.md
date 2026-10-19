# Windows video regression — 19 October 2026

The reported sequence was Pi MP3 streaming followed by dropping a local 1080p, 10-bit HEVC / EAC3 movie. A private modern ConPTY at 188x50 cells reproduced low frame throughput and a slow first opening. The media remains local and is not a repository fixture.

| Measurement | Previous encoder | Sampled Sixel / corrected cadence |
| --- | --- | --- |
| 480p callback, submitted frames/sec | 3.12 | 14.56 before cadence correction |
| 480p callback, recent encoding p95 | 270 ms | 13 ms |
| Default callback, submitted frames/sec | 1.71 including a 30.7-second opening delay | 20.25 |
| Default callback, recent encoding p95 | 377 ms | 18 ms |
| Default callback, recent draw p95 | 382 ms | 52 ms |
| Warm default file-drop to first video | variable | 1.07 seconds |

These are individual runs, not statistical speedup claims. The first old-default run includes opening time and is not a steady-throughput comparison. Lowering the old callback size alone restored approximately 24 callback frames/sec but left rendering near 3 FPS. The old ANSI path also had expensive writes. Buffering alone failed the performance gate and was removed. Inspection of the Ratatui image diff confirmed that unchanged image payloads were already suppressed.

The exact Pi-to-local transition subsequently passed with a real local audio clock enabled at volume zero: 40.447 measured seconds, 15.90 backend submissions/sec, 17.08 display callbacks/sec, 27 ms encoding p95, 90 ms draw p95, 245 ms request-to-backend p95, 2.23-second file-drop startup and 127 ms help response. It recorded no encoding failures and exited/restored the terminal. Some earlier development runs overlapped Rust compilation; use the reproducible probe for serial comparisons on an idle machine.

The initial performance encoder quantized a widget-bounded working image and scaled palette-index runs into the widget's physical pixel size (superseded by the quality correction below). This removes the full-resolution RGBA allocation and full-resolution quantizer input from the motion path. A cost controller reduces processing detail quickly and restores it gradually. Static artwork retains its existing full-detail path. Round-trip decoded-pixel tests cover scaling across Sixel bands, transparent padding, resize dimensions and budget recovery. Draw cadence now counts output time within the requested interval.

Validation at this checkpoint: 99 deterministic Rust tests passed, six explicit runtime/manual tests remained excluded from that normal suite, strict Clippy passed, thirteen Python tooling tests passed with one POSIX-only skip, and actionlint passed. The Windows menu-hover and artwork console checks cover the independent mouse/static-image paths.

The 30.7-second first opening has not been reproduced consistently after warming caches; later starts do not prove that cold-start issue permanently fixed. A ConPTY sink counts backend submissions rather than visible Windows Terminal frames. Muted audio-clock tests cannot certify audible lip sync. Full-movie visual acceptance remains open. The portable build and installed executable are validated separately after this source checkpoint.

See [benchmark instructions](../BENCHMARKING.md) for the native probe and metric definitions.

## Buffer ownership follow-up

An additional callback regression first failed on the former copy path, then passed with ownership transfer. Each native picture now owns a Vec allocation with an aligned pixel subregion; the complete allocation moves into the retained snapshot. No raw allocation is adopted by a differently aligned Vec, and no unsafe Send/Sync implementation is introduced. The test checks pointer identity, previous-frame isolation during another decode, and frame access on another thread after the callback context is dropped. The five real LibVLC checks pass, including actual zero copied bytes. The normal Rust suite now passes 100 tests, with the six runtime/manual checks excluded from that normal invocation.

A final serial source-player probe repeated the exact Pi-to-local transition with the local audio clock active at volume zero: **21.31 backend submissions/sec**, 23.46 callback frames/sec over 39.975 measured seconds, 24 ms encoder p95, 54 ms draw p95, 134 ms request-to-backend p95, 2.10-second dropped-file startup, 128 ms help response, zero encoding failures and clean exit/restoration. Callback copy counters remained zero. These source-player results precede the standalone installation check and retain the presentation/cold-start limitations above.

The pushed encoder checkpoint's CI found a newer Clippy array-chunk lint; the follow-up uses `as_chunks`. Packaging regression checks also require the linked performance report in the explicit documentation inventory. An artwork smoke timed out on shutdown during concurrent checks; the isolated captured rerun and independent hover smoke both passed. A single rerun does not establish that every startup/shutdown timing case is eliminated.

## Standalone, installation and ARM64 verification

The clean-source Windows x64 standalone embeds commit `89e247de2e5da2f6543f0fd18d0b52762a0ed4dc` and LibVLC 3.0.24. SHA-256: `4b801061409f73f1021269d2ed9ef88b5f0299359931beae7472696d81f53c36`.

It passed the full portable verifier: native architecture/static runtime, combined help, concurrent launches, isolated bundled LibVLC, actual decoding, invalid runtime replacement rejection, normal cleanup, explicit extraction, complete documentation and forced Windows Job Object lifetime checks. Bundled Sixel album-art and menu-hover console smokes passed independently.

The standalone repeated the Pi MP3-to-local movie transition with a muted local audio clock: **18.87 backend submissions/sec**, 22.68 callback frames/sec over 37.04 measured seconds, 22 ms encoder p95, 50 ms draw p95, 111 ms request-to-backend p95, 126 ms help response, no encoding failures and clean exit/restoration. Initial UI startup, including private extraction/SSH music setup, took 14.85 seconds. The next file-drop to first new video callback took 5.05 seconds. For all native probe results above, the `first_video_s` field observes callback availability; it does not measure a client's first visible frame.

The verified executable is installed at `C:\Users\morph\AppData\Local\Programs\VLC-TTY\vlc-tty.exe`, and that directory is on the user PATH. Both previous workspace launch names, `target/release/termvlc.exe` and `target/release/vlc-tty.exe`, contain the identical executable. Previous files were renamed to backups; existing user processes were not terminated.

The physical Ubuntu 26 Pi 5 compiled the exact source commit with the headless feature set in a separate testing directory. Its thirty-second Sixel PTY smoke passed: 397 submitted frames in the final report, nine help probes (maximum 45 ms), zero frame copies, zero encoding failures and clean terminal restoration. Peak summed RSS was 144.7 MiB; average sampled CPU was 83.5 percent of one core. This is a short synthetic server smoke, not full-movie presentation evidence; the installed Pi player was not replaced.

[CI at the executable's source commit](https://github.com/chinmaykrishnroy/VLC-TTY/actions/runs/39237309140) passed all eight jobs: six native OS/architecture lanes, Ubuntu 26 ARM64 headless checks, and runtime-source preservation. Public publication remains subject to the existing strict runtime-source correspondence gate.


## Fullscreen quality correction — October 19, 2026

The follow-up screenshot showed coarse blocks and damaged lettering. A decoder-frame extraction from the user's local movie remained sharp. The old adaptive motion path could reduce its spatial working image to 320×180 and enlarge those indices to a fullscreen raster. A deterministic decoded-Sixel regression reproduced the defect: only 2,880 of 5,184 fine black/white pixels survived. The correction preserves those strokes and periodic RGB strokes, including palette-sampling alias cases and source upscaling.

Only colour-palette training is sampled now: its ceiling starts at 57,600 pixels, can decrease to 14,400 and recover to 230,400. Stable stratified sample positions avoid a fixed grid missing periodic textures. Every fitted picture pixel comes from the decoder; when the widget is larger than the decoded source, native source indices are expanded without discarding any additional source resolution. A frame-local 4 MiB colour cache bounds repeated matching work. Palette matching uses representable Sixel colours, and light ordered dithering reduces banding. Static artwork and other protocols retain their existing paths.

A release-mode round trip of one 1920×1080 opening studio frame from the private movie measured **27.19 dB PSNR** for the old minimum spatial budget and **40.56 dB** for the new minimum palette budget. The source, raster size and decoder were unchanged. These are pixel-error measurements of one frame, not a perceptual quality score for the movie. The new palette-budget cases measured 39–41 ms median encoding over three repetitions and approximately 0.56–0.68 MB output per frame; stronger dithering was rejected because it roughly doubled output on this frame. Preview PNGs and the isolated harness stay in ignored local diagnostics; private movie images are not committed or uploaded.

Two short Windows ConPTY probes used the same movie, 188×50 cells, a muted active local audio clock and 40 seconds after dropping the file:

| Measurement | Previous spatial downsampling | Preserved picture detail |
| --- | --- | --- |
| Backend submissions/sec | 23.92 | 19.67 |
| Video callbacks/sec | 23.98 | 23.95 |
| Encode p95 | 8.40 ms | 36.10 ms |
| Backend draw p95 | 27.68 ms | 70.27 ms |
| Request-to-backend p95 | 58.28 ms | 149.20 ms |
| Help response | 118 ms | 107 ms |
| Captured terminal output | 114.5 MB | 337.2 MB |
| Encoding failures | 0 | 0 |
| Clean quit and terminal restoration | Passed | Passed |

The throughput windows exclude initial opening (31.48 and 33.91 measured seconds respectively). Captured output totals include UI/protocol traffic throughout the probe. Quality requires more bandwidth and work; the gate was retained at 12 backend submissions/sec and 500 ms maximum help latency. These measurements establish responsive backend playback at the improved detail level, not visible Windows Terminal FPS, speaker alignment or movie-length stability. SSH connections and terminal clients can impose additional limits.

The normal Windows Rust suite passes 111 deterministic tests; the five real-runtime checks and manual encoder benchmark remain separately selected. Strict Clippy, packaging regressions and the fullscreen Sixel resize/Escape/quit smoke passed locally. Final packaged validation and CI results are recorded after building this checkpoint.


### Packaged quality checkpoint and installation

The final executable source is `09871bab6e7cc5bb932af876c875ccd00396a28f`; production rendering matches the measured `aa4cb15` checkpoint, with a subsequent fixed-array test-only cleanup for newer Clippy. [All eight CI jobs passed](https://github.com/chinmaykrishnroy/VLC-TTY/actions/runs/39298653161). Both downloaded artifact archives were checked against GitHub's SHA-256 digest, their embedded clean-source/architecture manifests and the executable checksum companions.

The Windows x64 standalone passed the complete local portable verifier, including concurrent diagnostics, isolated bundled LibVLC, decoded progress, extraction/document inventory, cleanup and forced supervisor termination. It then repeated **Pi MP3 streaming → dropped local movie in fullscreen**, with an active local audio clock muted at volume zero: **20.16 backend submissions/sec**, 23.92 callbacks/sec over 35.87 measured seconds, 43.27 ms encoder p95, 59.73 ms draw p95, 120.74 ms request-to-backend p95 and 111 ms help response. Encoding failures were zero, and quit/terminal restoration passed. UI setup, private extraction and SSH music startup took 24.68 seconds; the movie drop reached its first new callback after 2.03 seconds. These remain backend/clock checks, not visible FPS or audible lip-sync measurements.

A separate round trip of the Paramount scene at 30 seconds in the same file measured 23.83 dB PSNR with the old minimum spatial budget and 41.00 dB with the new minimum palette-training budget. The corrected output retains the fine mountain detail and script lettering. These local previews are decoded protocol output rather than screenshots of a physical terminal.

The Pi 5 compiled source `aa4cb15` headlessly and passed a 30-second Sixel probe: 396 submitted frames in the final 29.04-second sample, nine help probes (maximum 69 ms), 144.22 MiB peak summed RSS, 91.89 percent average CPU of one core, zero copies/errors and clean restoration. The actual Linux ARM64 standalone then decoded headlessly with bundled LibVLC 3.0.20 and passed a 20-second Sixel probe: 235 submissions in the final 17.25-second sample, six help probes (maximum 67 ms), 204.76 MiB peak summed RSS, zero copies/errors and clean restoration. The standalone's process-tree memory includes its supervisor and may double-count shared pages. No full-movie or post-5,000-frame claim follows from these short tests.

Verified installed executables, with previous versions retained as backups and running user processes preserved:

- Windows PATH: `C:\Users\morph\AppData\Local\Programs\VLC-TTY\vlc-tty.exe`, plus both workspace `target/release` launch names. SHA-256: `376a347eedc891c8d39dbaf75a5cdac1876b07bd1b18a1c383cff1d4a786a951`.
- Pi: `/usr/local/bin/vlc-tty` and `/home/roy/.local/bin/vlc-tty`, replaced atomically after packaged decoding/interaction checks. SHA-256: `98a5d0b15ffa926b8f24f644136a7da476291dc501f136417db3a90620532926`.

Restart an existing player process to use the corrected renderer. The fix is automatic; the 256-colour Sixel palette still imposes a colour/bandwidth tradeoff. Cold startup and physical presentation remain separate acceptance concerns.
