# Build/test report (this packaging environment)

- Rust files created: `main.rs`, `app.rs`, `vlc.rs`, `renderer.rs`.
- `Cargo.toml` includes `ratatui`, `crossterm`, `libloading`, `anyhow`.
- Local source-integrity checks: passed (files present, valid UTF-8, no control characters).
- Bundled `samples/sample.mp4`: `ffprobe` **passed** — H.264 320×180 video + AAC audio, 8.0 seconds.
- `cargo test`: **NOT RUN** — `cargo` is not installed (`cargo: command not found`).
- LibVLC playback: **NOT RUN** — LibVLC shared library unavailable in this container.
- Git clone: **NOT RUN** — host DNS was unavailable (`Could not resolve host: github.com`).
- Windows/macOS/Linux user-machine tests: **NOT RUN**.

The project is therefore a source implementation with unverified Rust compilation/playback. Do not treat it as an officially tested binary. A Windows smoke script is provided at `scripts/windows-run.ps1`, and the `src/vlc.rs` tests include a mock-format video buffer callback round trip that can run without LibVLC installed (once Cargo is available).
