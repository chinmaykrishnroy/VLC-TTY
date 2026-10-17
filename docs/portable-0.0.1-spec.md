# VLC-TTY 0.0.1 implementation brief

Source: the user's 17 October 2026 request in this chat. Baseline: commit `a4d37cf`.

## Required delivery

- Reproduce the reported Windows launch failure using the exact downloaded file and matching host architecture. Smoke-test the correct Windows executable.
- Deliver one portable native executable per supported OS/CPU, embedding LibVLC and required plugins. Prefer no installer. Dynamic runtime extraction is acceptable and must be described honestly.
- Keep six targets: Windows, Linux and macOS on x64 and ARM64. Match CPU architecture for application and runtime; prevent system VLC from masking a broken bundle. Preserve library replacement and source/license records.
- Run native decoding and terminal tests, including the user's headless Pi 5 / Ubuntu 26.04 / SSH use case with silent audio initially.
- Start public versioning at 0.0.1. Provide complete `-h` / `--help`, contribution guidance and release documentation.
- Diagnose/fix concrete bugs, make incremental feature checkpoints, commit meaningful milestones and continue autonomously while the user is away.

## Planned next feature slice

Playlist import/export, explicit queue ordering, repeat/shuffle and useful startup playback options. Keep these changes independently testable. This is an agile scope choice; it is not a promise to implement every desktop VLC capability in one milestone.

## Evidence rules

Native launch/decoded frames are required for runtime claims. A unit test cannot prove terminal visual quality, audio sync, long-session stability or device performance. Mark source snapshots and upstream-binary reproducibility separately. Do not publish a tag/release as a side effect of implementing the pipeline.
