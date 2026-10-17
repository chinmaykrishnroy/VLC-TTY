"""Require six standalone files, clean build identities and runtime sources."""
import os
from pathlib import Path
import sys
import tomllib
from package_release import ROOT, TARGETS, digest
from build_portable import LABELS
import json


def manifest(directory):
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    if os.environ.get("GITHUB_REF") != f"refs/tags/v{version}":
        raise ValueError("Release tag must match Cargo.toml version")
    expected = {f"vlc-tty-{version}-{LABELS[target]}" + (".exe" if "windows" in target else "")
                for target in TARGETS}
    actual = {path.name for path in directory.iterdir() if path.is_file() and
              (path.suffix == ".exe" or (path.name.startswith(f"vlc-tty-{version}-") and not path.name.endswith((".json", ".sha256", ".tar.gz"))))}
    if expected != actual:
        raise ValueError(f"Wrong release executable set: missing={expected - actual}; extra={actual - expected}")
    lines = []
    sources = {"vlc-3.0.24-source-snapshot.tar.gz"}
    for target in TARGETS:
        name = f"vlc-tty-{version}-{LABELS[target]}" + (".exe" if "windows" in target else "")
        identity = json.loads((directory / (name + ".json")).read_text())
        if identity["target"] != target or not identity["runtime_included"] or identity.get("source_dirty", True):
            raise ValueError(f"Invalid release build identity for {name}")
        sources.add(identity["runtime"]["source_companion"]["file"])
    for name in sorted(expected | sources):
        path = directory / name
        if not path.is_file():
            raise ValueError(f"Missing corresponding source companion: {name}")
        checksum = digest(path)
        sidecar = path.with_name(name + ".sha256")
        if not sidecar.exists() or sidecar.read_text().split() != [checksum, name]:
            raise ValueError(f"Checksum mismatch for {name}")
        if name in expected and not sidecar.exists():
            raise ValueError(f"Missing executable checksum: {name}")
        lines.append(f"{checksum}  {name}\n")
    (directory / "SHA256SUMS").write_text("".join(lines))


if __name__ == "__main__":
    manifest(Path(sys.argv[1]))
