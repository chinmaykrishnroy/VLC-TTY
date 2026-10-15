# Run from the project root in PowerShell.
# Requires 64-bit VLC 3.x and stable Rust/Cargo.
$ErrorActionPreference = "Stop"
if (-not $env:VLC_HOME) { $env:VLC_HOME = "C:\Program Files\VideoLAN\VLC" }
$env:PATH = "$env:VLC_HOME;$env:PATH"
if (-not (Test-Path "$env:VLC_HOME\libvlc.dll")) {
    throw "Cannot find libvlc.dll in VLC_HOME=$env:VLC_HOME. Install VLC 3.x (64-bit)."
}
cargo test
if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }
cargo run --release -- --doctor
if ($LASTEXITCODE -ne 0) { throw "LibVLC diagnostics failed" }
cargo run --release -- .\samples\sample.mp4
