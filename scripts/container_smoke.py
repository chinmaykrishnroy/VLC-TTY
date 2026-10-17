"""Decode with the Linux single file on a base image without system LibVLC."""
from pathlib import Path
import subprocess
import sys
import tomllib
from build_portable import LABELS
from package_release import ROOT

target = sys.argv[1]
version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
name = f"vlc-tty-{version}-{LABELS[target]}"
subprocess.run(["docker", "run", "--rm", "--network=none", "-v", f"{ROOT / 'dist'}:/dist:ro",
                "-v", f"{ROOT / 'samples'}:/samples:ro", "ubuntu:24.04", f"/dist/{name}",
                "--ssh", "--smoke-test", "/samples/sample.mp4"], check=True)
