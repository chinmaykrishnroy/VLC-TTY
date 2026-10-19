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

The new encoder quantizes a widget-bounded working image and scales palette-index runs into the widget's physical pixel size. This removes the full-resolution RGBA allocation and full-resolution quantizer input from the motion path. A cost controller reduces processing detail quickly and restores it gradually. Static artwork retains its existing full-detail path. Round-trip decoded-pixel tests cover scaling across Sixel bands, transparent padding, resize dimensions and budget recovery. Draw cadence now counts output time within the requested interval.

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
