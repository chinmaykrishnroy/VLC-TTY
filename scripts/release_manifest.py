"""Fail closed unless exactly the six expected application archives are present."""
import os
from pathlib import Path
import sys
import tomllib
from package_release import ROOT, TARGETS, digest


def manifest(directory):
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    if os.environ.get("GITHUB_REF") != f"refs/tags/v{version}":
        raise ValueError("Release tag must match Cargo.toml version")
    expected = {f"vlc-tty-{version}-{target}" + (".zip" if "windows" in target else ".tar.gz")
                for target in TARGETS}
    actual = {path.name for path in directory.iterdir() if path.name.endswith((".zip", ".tar.gz"))}
    if expected != actual:
        raise ValueError(f"Wrong release archive set: missing={expected - actual}; extra={actual - expected}")
    lines = []
    for name in sorted(expected):
        path = directory / name
        checksum = digest(path)
        if path.with_name(name + ".sha256").read_text().split() != [checksum, name]:
            raise ValueError(f"Checksum mismatch for {name}")
        lines.append(f"{checksum}  {name}\n")
    (directory / "SHA256SUMS").write_text("".join(lines))


if __name__ == "__main__":
    manifest(Path(sys.argv[1]))
