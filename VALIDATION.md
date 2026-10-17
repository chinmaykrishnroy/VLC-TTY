# Validation — 17 October 2026

## Verified locally on Windows x64

- LibVLC 3.0.24 loads from the installed VideoLAN runtime.
- 59 Rust tests cover controls, queue state, input, captions, renderer layout, frame mailboxes and native decoding. The complete suite explicitly enables three runtime tests.
- File and loopback HTTP tests require decoded frames and an advancing playback clock. Pause, seek, frame stepping, stop and delay round-trips are exercised against LibVLC.
- Silent playback decodes frames with audio disabled and a 90p callback cap, without requiring an audio device. The SSH profile chooses silent audio, 15 FPS and 720p by default.
- Regression tests reject old encoded results across invalidation, verify tmux ESC doubling, require inline Kitty transfer with a reused image ID, bound encoding pixel budgets and reject overflowing subtitle timestamps.
- Fullscreen OSD rows and optional caption rows are outside the pixel rectangle. Terminal-specific visual behavior remains a manual gate.
- The release executable and its extracted Windows archive pass version/help/doctor and an actual silent headless decode check. Diagnostics report no TTY on stdin/stdout during this check.
- Four Python packaging tests cover Unix executable permissions, pre-1980 ZIP timestamps, architecture-specific runtime manifests, matching release tags, complete archive sets and corruption detection.
- Workflow syntax passes actionlint 1.7.12. Its runner inventory predates Ubuntu 26.04 ARM64; the documented hosted label is explicitly listed in the configuration.

Earlier regression runs reproduced volume/subtitle/input/queue defects before repair. The new stale-frame, tmux and timestamp cases also failed before their fixes. A passed fake-backend test is not counted as native playback evidence.

## Automated remote gates

The CI workflow requires six native targets: Linux, macOS and Windows on x64 and ARM64. Each target must pass unit tests, strict Clippy, a release build, real LibVLC decoding tests, package creation and a decode check from the extracted archive. An additional Ubuntu 26.04 ARM64 job installs only the headless runtime packages and runs without display variables.

At the time this milestone was prepared, these new workflows were pending their first run. Update this record with run results rather than treating a workflow definition as proof. The initial baseline workflow passed its unit suites but failed Clippy on newer Rust lints; those reported source patterns have been corrected.

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

- Actual Pi 5 / Ubuntu Server 26.04 over SSH, using the intended terminal. Record codec/resolution, renderer, terminal size, FPS/cap, CPU/memory/temperature, connection and session duration.
- Kitty, Sixel and iTerm2: inspect caption/OSD visibility, menus, resize, fullscreen transitions, repeated media opens, renderer toggling, image cleanup and tmux passthrough. ANSI fallback needs a real terminal check too.
- Sustained playback and input responsiveness on low-bandwidth connections, large files and live streams.
- Unicode paths, invalid media/URLs, track switching and embedded subtitles across platforms.
- Optional local audio/lipsync and native-window behavior. Audio forwarding to the SSH client is not implemented.
- Clean-machine installation, platform security prompts, signing/notarization if future distribution claims require them.

Application archives require installed matching LibVLC libraries and plugins. No Pi/device performance, audible output, terminal visual acceptance, signed installers or VLC feature parity is claimed by these local checks.
