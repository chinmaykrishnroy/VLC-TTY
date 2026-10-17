"""Require six standalone files, clean build identities and runtime sources."""
import os
from pathlib import Path
import tomllib
from package_release import ROOT, TARGETS, digest
from build_portable import LABELS
import json
import argparse
import hashlib
import tarfile
from collect_sources import VLC_URL, VLC_SHA


def verify_upstream_sources(path):
    """Verify preserved source bytes; do not turn a source superset into a reproducibility claim."""
    root = "vlc-3.0.24-source-snapshot/"
    with tarfile.open(path, "r:gz") as archive:
        record = json.load(archive.extractfile(root + "SOURCE_MANIFEST.json"))
        if record.get("origin") != VLC_URL or record.get("sha256") != VLC_SHA:
            raise ValueError("Wrong upstream source release identity")
        original = record.get("original_file")
        if not original or "/" in original or "\\" in original:
            raise ValueError("Missing original upstream source archive")
        with archive.extractfile(root + original) as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != VLC_SHA:
                raise ValueError("Original upstream source checksum mismatch")
        records = record.get("contrib_sources", [])
        if not records:
            raise ValueError("Missing contributed dependency source records")
        for item in records:
            name = item["file"]
            if not name.startswith("contrib/tarballs/") or ".." in name.split("/"):
                raise ValueError("Invalid contributed source path")
            with archive.extractfile(root + name) as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != item["sha256"]:
                    raise ValueError(f"Contributed source checksum mismatch: {name}")
        for name in ["COPYING", "COPYING.LIB", "configure.ac", "contrib/src/main.mak"]:
            if not archive.getmember(root + name).isfile():
                raise ValueError(f"Missing source/build/license file: {name}")


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
    upstream = json.loads((ROOT / "scripts/runtime-manifest.json").read_text())
    for target in TARGETS:
        name = f"vlc-tty-{version}-{LABELS[target]}" + (".exe" if "windows" in target else "")
        identity = json.loads((directory / (name + ".json")).read_text())
        if identity["target"] != target or identity.get("version") != version or not identity["runtime_included"] or identity.get("source_dirty", True):
            raise ValueError(f"Invalid release build identity for {name}")
        if os.environ.get("GITHUB_SHA") and identity.get("commit") != os.environ["GITHUB_SHA"]:
            raise ValueError(f"Release executable came from a different commit: {name}")
        if not identity["runtime"].get("source_correspondence", {}).get("verified", False):
            raise ValueError(f"Unresolved runtime source correspondence blocks publication: {name}")
        if "linux" not in target and not identity["runtime"].get("archive_verified", False):
            raise ValueError(f"Unverified upstream runtime archive blocks publication: {name}")
        if "linux" not in target and (identity["runtime"].get("origin") != upstream[target]["url"] or identity["runtime"].get("upstream_sha256") != upstream[target]["sha256"]):
            raise ValueError(f"Upstream runtime identity mismatch: {name}")
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
    for name in sorted(expected):
        identity_path = directory / (name + ".json")
        lines.append(f"{digest(identity_path)}  {identity_path.name}\n")
    verify_upstream_sources(directory / "vlc-3.0.24-source-snapshot.tar.gz")
    (directory / "SHA256SUMS").write_text("".join(lines))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    manifest(args.directory)
