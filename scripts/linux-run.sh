#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if ! command -v cargo >/dev/null; then
  echo 'Cargo not found: install Rust via https://rustup.rs' >&2
  exit 1
fi
# Ubuntu/Debian: sudo apt install --no-install-recommends libvlc5 vlc-plugin-base
# Fedora: sudo dnf install vlc libvlc
# Arch: sudo pacman -S vlc
cargo build --release
cargo test
"./target/release/vlc-tty" --doctor
"./target/release/vlc-tty" --graphics auto "${1:-./samples/sample.mp4}"
