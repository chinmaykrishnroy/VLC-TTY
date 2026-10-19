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
