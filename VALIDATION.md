# v0.5.0 validation report

Status: **source release candidate, NOT compile-verified or real-playback-verified**.

Checks completed in the packaging container:
- Reused **v0.4.0** full pixel-graphics and cross-platform source, not a regression to v0.2.
- Added LibVLC 3.x subtitle/audio track functions and UTF-8 sidecar parser.
- Checked Rust source tokenization, balanced delimiters, project manifest, expected source paths, ZIP integrity and FFmpeg sample metadata.
- Verified original sample MP4 with `ffprobe` and sample subtitle timestamps by inspection.

Unavailable in this environment:
- `cargo test` / `cargo build`: **not run**, no Rust toolchain and no access to download one.
- Actual LibVLC playback and graphical terminal protocol output: **not run**, LibVLC not installed.
- Win11/Linux/macOS binaries or runtime verification: **not run**. CI workflow can test builds after pushing the files to GitHub.

Windows first smoke test:
1. `cargo build --release`
2. `cargo test`
3. `./target/release/termvlc.exe --doctor`
4. `./target/release/termvlc.exe --graphics auto ./samples/sample.mp4`
5. Press F, Esc and click the Back control; check subtitles at 1s and 4s, press S and A, click and drag the timeline, press G for ANSI fallback.

If compilation fails, capture the **first** Rust compiler error including file and line number. If the runtime fails, include `--doctor` output and terminal emulator/version.
