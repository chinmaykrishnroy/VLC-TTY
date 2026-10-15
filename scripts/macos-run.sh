#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if ! command -v cargo >/dev/null; then
  echo 'Cargo not found: install Rust via https://rustup.rs' >&2
  exit 1
fi
# VLC.app (https://www.videolan.org/vlc/download-macosx.html)
# Can override with VLC_HOME=/Applications/VLC.app or a custom prefix.
export VLC_HOME="${VLC_HOME:-/Applications/VLC.app}"
cargo build --release
cargo test
"./target/release/termvlc" --doctor
"./target/release/termvlc" --graphics auto "${1:-./samples/sample.mp4}"
