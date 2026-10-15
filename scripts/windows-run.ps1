# Run inside the project root (with Cargo.toml) using PowerShell.
$ErrorActionPreference = "Stop"
if (-not $env:VLC_HOME) { $env:VLC_HOME = "C:\Program Files\VideoLAN\VLC" }
$env:PATH = "$env:VLC_HOME;$env:PATH"
if (-not (Test-Path "$env:VLC_HOME\libvlc.dll")) {
    throw "LibVLC not found at $env:VLC_HOME. Install 64-bit VLC 3.x and/or set VLC_HOME."
}
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Rust build failed" }
cargo test
if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
cargo run --release -- --doctor
if ($LASTEXITCODE -ne 0) { throw "VLC diagnostics failed" }
cargo run --release -- --graphics auto .\samples\sample.mp4
