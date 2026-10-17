"""Check archive identity/checksum and decode with the extracted executable."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from urllib.parse import unquote, urlsplit
from package_release import ROOT, DOCS, digest


def verify_documentation(root, documents):
    for relative in documents:
        path = root / relative
        if path.suffix != ".md":
            continue
        for link in re.findall(r"\]\(([^)]+)\)", path.read_text(encoding="utf-8-sig")):
            url = urlsplit(link)
            if not url.scheme and url.path and not (path.parent / unquote(url.path)).exists():
                raise RuntimeError(f"Broken packaged documentation link in {relative}: {link}")


def verify(archive, target):
    expected = archive.with_name(archive.name + ".sha256").read_text().split()[0]
    if digest(archive) != expected:
        raise RuntimeError("Application archive checksum mismatch")
    with tempfile.TemporaryDirectory(prefix="vlc-tty-extracted-") as temporary:
        directory = Path(temporary)
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as source:
                source.extractall(directory)
        else:
            with tarfile.open(archive) as source:
                source.extractall(directory, filter="data")
        roots = list(directory.iterdir())
        if len(roots) != 1 or not roots[0].is_dir():
            raise RuntimeError("Expected exactly one archive root")
        root = roots[0]
        identity = json.loads((root / "BUILD.json").read_text())
        binary = root / ("vlc-tty.exe" if "windows" in target else "vlc-tty")
        if identity["target"] != target or identity["runtime_included"] or digest(binary) != identity["binary_sha256"]:
            raise RuntimeError("Packaged build identity does not match")
        if not (root / "THIRD_PARTY.json").is_file() or not (root / "licenses").is_dir():
            raise RuntimeError("Dependency notices missing")
        verify_documentation(root, DOCS)
        env = {key: value for key, value in os.environ.items() if key not in ("DISPLAY", "WAYLAND_DISPLAY")}
        for flags in [["--version"], ["--help"], ["--ssh", "--doctor"],
                      ["--ssh", "--smoke-test", str(ROOT / "samples/sample.mp4")]]:
            subprocess.run([str(binary), *flags], cwd=directory, env=env, check=True, timeout=30)


if __name__ == "__main__":
    verify(Path(sys.argv[1]).resolve(), sys.argv[2])
