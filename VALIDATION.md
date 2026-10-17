# Validation — 17 October 2026

## 0.0.1 portable work

The reported Windows failure was reproduced from the exact downloaded ARM64 executable (`PE machine 0xAA64`) on Windows x64. Windows rejected the incompatible architecture; the matching x64 source executable launched and decoded video. Portable release names now use explicit `windows-x64` and `windows-arm64` labels, and packaging inspects native file headers.

The Windows x64 standalone development build passed matching `-h`/`--help`, version/bundle identity, two concurrent runtime loads, actual silent decoding, normal-exit extraction cleanup, explicit extraction and documentation checks. Its LibVLC loaded from private extraction paths while external runtime paths were deliberately invalid. A regression also requires an invalid replacement runtime to fail instead of silently falling back to installed VLC.

The follow-up passes 67 deterministic Rust tests (including real command-line invocation) and all three explicit LibVLC runtime tests on Windows x64, strict Clippy, eight packaging regressions and actionlint. Tests cover duplicate playlist occurrences, shuffle restoration, CLI validation, combined portable help, static CRT import inspection and rejection of unresolved source provenance.

The static-CRT Windows x64 standalone passed bundled decoding and actual PowerShell pseudo-terminal video, pause/fullscreen, queue, shuffle/repeat and clean alternate-screen exit. An automated forced-supervisor-termination check confirmed that the Windows Job Object stops the extracted player. This does not simulate a clean Windows installation's security prompts.

[Portable CI run 39029478162](https://github.com/chinmaykrishnroy/VLC-TTY/actions/runs/39029478162), at `e9a3264`, passed **all eight jobs**: all six native build/runtime/standalone lanes, Ubuntu 26.04 ARM64 headless/PTY checks, and the common source collector. Both Windows builds passed CRT-import checks. Unix native lanes passed actual launcher SIGTERM cleanup, and Linux standalone files decoded inside Ubuntu containers with no VLC packages. The additional Windows forced-termination regression passed locally and is required in subsequent native CI runs.

An earlier common-source job failed in an upstream Tremor recipe that stored an abbreviated pin but required 40 digits during verification. The collector now expands that same verified commit and records the adjustment; the complete source job passes. This source snapshot still does not establish the unresolved Windows/macOS binary-to-contrib correspondence described below.

Windows/macOS exact binary-to-contrib-source correspondence remains unverified, and publication refuses that unresolved identity. The physical Pi now passes the standalone bundled-runtime checks, SIGTERM child/terminal/runtime cleanup, and a 30-second 720p PTY interaction/caption test. Launcher-plus-player peak summed RSS was 235.39 MiB and average CPU 68.2% of one core in that synthetic test; see [Pi details](docs/PI5-VALIDATION.md) for measurement limits and the discarded launcher-only sample. See also [checkpoint review](docs/portable-review.md).

## Earlier thin-application milestone

## Verified locally on Windows x64

- LibVLC 3.0.24 loads from the installed VideoLAN runtime.
- 59 Rust tests cover controls, queue state, input, captions, renderer layout, frame mailboxes and native decoding. The complete suite explicitly enables three runtime tests.
- File and loopback HTTP tests require decoded frames and an advancing playback clock. Pause, seek, frame stepping, stop and delay round-trips are exercised against LibVLC.
- Silent playback decodes frames with audio disabled and a 90p callback cap, without requiring an audio device. The SSH profile chooses silent audio, 15 FPS and 720p by default.
- Regression tests reject old encoded results across invalidation, verify tmux ESC doubling, require inline Kitty transfer with a reused image ID, bound encoding pixel budgets and reject overflowing subtitle timestamps.
- Fullscreen OSD rows and optional caption rows are outside the pixel rectangle. Terminal-specific visual behavior remains a manual gate.
- The release executable and its extracted Windows archive pass version/help/doctor and an actual silent headless decode check. Diagnostics report no TTY on stdin/stdout during this check.
- Five Python packaging tests cover Unix executable permissions, pre-1980 ZIP timestamps, architecture-specific runtime manifests, matching release tags, complete archive sets, corruption detection and packaged documentation links.
- Workflow syntax passes actionlint 1.7.12. Its runner inventory predates Ubuntu 26.04 ARM64; the documented hosted label is explicitly listed in the configuration.

Earlier regression runs reproduced volume/subtitle/input/queue defects before repair. The new stale-frame, tmux and timestamp cases also failed before their fixes. A passed fake-backend test is not counted as native playback evidence.

## Automated remote gates

The CI workflow requires six native targets: Linux, macOS and Windows on x64 and ARM64. Each target must pass unit tests, strict Clippy, a release build, real LibVLC decoding tests, package creation and a decode check from the extracted archive. An additional Ubuntu 26.04 ARM64 job installs only the headless runtime packages and runs without display variables.

[CI run 39025755417](https://github.com/chinmaykrishnroy/VLC-TTY/actions/runs/39025755417), at commit `1f2a8c1`, passed **all seven jobs**: six native build/runtime/package lanes and the Ubuntu 26.04 ARM64 headless/PTY lane. Each native target produced an archive and passed decoding from its extracted executable. The PTY lane verified video writes, captions, paused redraw, help/queue views and terminal restoration.

Earlier runs exposed newer Clippy lints, unsigned `c_char` on Linux ARM64 and macOS's core-library preload requirement. Those were fixed and the corresponding native lanes passed. Real Pi testing additionally reproduced an unanswered cursor-query crash during presentation resets; the application now resets its viewport without that query, and the failing interaction sequence passes.

## Verified on a physical Pi 5

A Raspberry Pi 5 running Ubuntu Server 26.04 ARM64 loaded distro LibVLC 3.0.23 and decoded video with no display/session or sound-device requirement. A 45-second ANSI PTY run with a synthetic 720p/30 FPS source passed caption, pause, frame-step/seek, help, queue, reopen, resize and clean-exit checks. Peak player RSS was 157.82 MiB and sampled average CPU was 66.18% of one core for this test. A separate direct SSH run delivered ANSI video to the Windows client and exited cleanly after keyboard input.

Full environment, measurements, limits and the reproduced/fixed defect are recorded in [Pi 5 validation](docs/PI5-VALIDATION.md). These checks do not certify pixel-protocol visual quality or long-session performance.

## Reproduce

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo test --locked --all-targets -- --ignored --test-threads=1 --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
python -m unittest discover -s scripts -p 'test_*.py' -v
vlc-tty --ssh --doctor
vlc-tty --ssh --smoke-test samples/sample.mp4
```

Unit tests report runtime tests as ignored by default, so machines without VLC can still run the deterministic suite. The explicit ignored-test command must load LibVLC and produce frames; it does not silently skip a missing runtime. Tests use dummy/disabled audio and establish no audible synchronization result.

## Required manual acceptance

- Longer sessions on the Pi 5 through the intended terminal, with real media and recorded network/resource conditions. Basic headless decoding, ANSI controls and direct SSH transport have passed.
- Kitty, Sixel and iTerm2: inspect caption/OSD visibility, menus, resize, fullscreen transitions, repeated media opens, renderer toggling, image cleanup and tmux passthrough. ANSI fallback needs a real terminal check too.
- Sustained playback and input responsiveness on low-bandwidth connections, large files and live streams.
- Unicode paths, invalid media/URLs, track switching and embedded subtitles across platforms.
- Optional local audio/lipsync and native-window behavior. Audio forwarding to the SSH client is not implemented.
- Clean-machine installation, platform security prompts, signing/notarization if future distribution claims require them.

Thin application builds require installed matching LibVLC libraries and plugins; portable builds embed their runtime. Automated and short device checks do not establish movie-length performance, audible output, terminal pixel quality, platform signing or VLC feature parity.
