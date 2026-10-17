# Building VLC-TTY

Use Rust stable, Cargo and the native platform linker (Visual Studio C++ Build Tools on Windows, Xcode Command Line Tools on macOS, a C toolchain on Linux). No VLC SDK, headers or source checkout are needed at build time.

```sh
cargo build --locked --release
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

The executable is `target/release/vlc-tty` or `target/release/vlc-tty.exe`. The application loads **LibVLC 3.x** dynamically when run. Install matching CPU-architecture libraries, core and plugins. LibVLC 4 is not supported by this adapter.

## Runtime

- Ubuntu/Debian: `sudo apt-get install --no-install-recommends libvlc5 vlc-plugin-base`.
- Windows: install matching-architecture VLC 3.x; the usual `Program Files/VideoLAN/VLC` installation is detected. Set `VLC_HOME` to its directory if needed. Keep `libvlccore.dll` and plugins alongside `libvlc.dll`.
- macOS: install VLC 3.x at `/Applications/VLC.app` or set `VLC_HOME` to the app bundle. Preserve its libraries, plugins and bundle structure.

`VLC_PLUGIN_PATH` can explicitly override plugin discovery. Avoid mixing libraries and plugins from different installations. `TERMVLC_DECODE_MAX` remains a legacy environment override for callback height; prefer `--decode-height` for new scripts.

```sh
vlc-tty --doctor
vlc-tty --ssh --smoke-test samples/sample.mp4
```

The default unit suite reports the runtime tests as ignored. With LibVLC installed, **enable them explicitly**:

```sh
cargo test --locked --all-targets -- --ignored --test-threads=1 --nocapture
```

These tests require decoded video frames and advancing playback time from a file and a loopback HTTP server. They check pause, frame stepping, seeking, stop, timing controls and silent playback without a sound device. They fail when LibVLC cannot load. Run them serially because native runtime initialization uses process-wide environment configuration.

## Packaging

Python 3.11+ is needed for release tooling. Build with an explicit target, then package and verify:

```sh
cargo build --locked --release --target aarch64-unknown-linux-gnu
python3 scripts/package_release.py aarch64-unknown-linux-gnu
python3 scripts/verify_package.py dist/vlc-tty-0.6.0-aarch64-unknown-linux-gnu.tar.gz aarch64-unknown-linux-gnu
python3 -m unittest discover -s scripts -p 'test_*.py' -v
```

The verification command runs the extracted executable, so use a host capable of executing that target. Packaging records Cargo metadata and copies dependency notices from the local Cargo registry. It fails if a dependency has no discoverable license text.

See [release process](RELEASING.md) for the complete native target matrix and [SSH guide](docs/SSH.md) for headless setup. Real rendering, network performance and physical-device behavior require terminal/device checks beyond the automated suite.
