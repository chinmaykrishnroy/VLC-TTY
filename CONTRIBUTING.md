# Contributing to VLC-TTY

VLC-TTY is a terminal-first player. Prioritize responsive keyboard controls, headless SSH playback, clear errors and repeatable tests over desktop UI parity.

## Development

Install Rust stable and native build tools, clone the repository, then run:

```sh
cargo build --locked --bin vlc-tty
cargo run --locked --bin vlc-tty -- --help
```

Source builds load an installed LibVLC 3 runtime. Portable release files embed that runtime and launch an extracted player. See [BUILDING.md](BUILDING.md) and [RELEASE.md](RELEASE.md).

## Before submitting

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
python -m unittest discover -s scripts -p 'test_*.py' -v
```

For playback changes, install LibVLC and enable the real runtime tests:

```sh
cargo test --locked --all-targets -- --ignored --test-threads=1 --nocapture
```

On Unix, build the player and run `python3 scripts/pty_smoke.py target/debug/vlc-tty samples/sample.mp4 --expect-caption TermVLC`. Native decode tests and PTY tests establish different things: keep both.

## Bugs and pull requests

Report the exact downloaded filename, OS/CPU architecture, terminal/version, `--doctor` output and reproduction steps. A PowerShell shell can run inside different terminal hosts. Redact media credentials and private URLs.

For a confirmed bug, add a regression that fails on the reported path before fixing it. Keep unrelated changes separate. Describe the user-visible change, evidence, and remaining device/terminal limits. A real device result is stronger than assuming behavior from a unit test.

## Checkpoints and scope

Work in small milestones: diagnose, implement, validate, commit. Keep playback changes separate from release tooling where practical. Update help, documentation and tests together when adding a CLI option. Do not call a feature supported until its required runtime path is exercised.

## Licenses

Application sources use MIT. Bundled LibVLC, plugins and codecs retain their own licenses; the portable distribution is not MIT-only. Keep component inventories, notices and corresponding-source records synchronized with runtime changes. Preserve the external-runtime override so users can replace the media engine.
