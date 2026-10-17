"""Create application-only archives, with build identity and dependency notices."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc",
)
DOCS = ("README.md", "LICENSE", "BUILDING.md", "RELEASING.md", "OBJECTIVE.md", "CHANGELOG.md",
        "ROADMAP.md", "ARCHITECTURE.md", "VALIDATION.md", "docs/SSH.md", "docs/PI5-VALIDATION.md",
        "docs/release-platform-research.md", "assets/vlc-tty.svg")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def dependency_notices(destination, target):
    supplements = json.loads((ROOT / "third_party/notice-sources.json").read_text())
    metadata = json.loads(command("cargo", "metadata", "--locked", "--format-version=1",
                                  "--filter-platform", target))
    dependencies = []
    for package in metadata["packages"]:
        if package["source"] is None:
            continue
        source = Path(package["manifest_path"]).parent
        key = f'{package["name"]}-{package["version"]}'
        licenses = sorted({p for p in source.iterdir() if p.is_file()
                           and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))})
        if package.get("license_file"):
            licenses = sorted(set(licenses + [source / package["license_file"]]))
        for entry in supplements.get(key, []):
            notice = ROOT / "third_party/notices" / key / entry["file"]
            if digest(notice) != entry["sha256"]:
                raise RuntimeError(f"Supplemental notice checksum mismatch for {key}")
            licenses.append(notice)
        if not licenses:
            raise RuntimeError(f"No packaged license text found for {key}; inspect before release")
        for path in licenses:
            output = destination / "licenses" / key / path.name
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, output)
        dependencies.append({"name": package["name"], "version": package["version"],
                             "license": package["license"], "repository": package["repository"]})
    (destination / "THIRD_PARTY.json").write_text(json.dumps(dependencies, indent=2) + "\n")
    shutil.copy2(ROOT / "third_party/notice-sources.json", destination / "NOTICE_SOURCES.json")


def create_archive(stage, output, windows):
    if windows:
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED, strict_timestamps=False) as archive:
            for path in sorted(stage.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(stage.parent))
    else:
        def portable_permissions(info):
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            info.mode = 0o755 if info.isdir() or info.name.endswith("/vlc-tty") else 0o644
            return info
        with tarfile.open(output, "w:gz") as archive:
            archive.add(stage, arcname=stage.name, filter=portable_permissions)
    output.with_name(output.name + ".sha256").write_text(f"{digest(output)}  {output.name}\n")


def package(target, binary=None, out_dir=None):
    if target not in TARGETS:
        raise ValueError(f"Unsupported release target: {target}")
    config = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    version = config["package"]["version"]
    tag = os.environ.get("GITHUB_REF", "")
    if tag.startswith("refs/tags/") and tag != f"refs/tags/v{version}":
        raise ValueError(f"Tag must match Cargo.toml: v{version}")
    windows = "windows" in target
    name = "vlc-tty.exe" if windows else "vlc-tty"
    binary = binary or ROOT / "target" / target / "release" / name
    out_dir = out_dir or ROOT / "dist"
    out_dir.mkdir(parents=True, exist_ok=True)
    stem = f"vlc-tty-{version}-{target}"
    with tempfile.TemporaryDirectory(prefix="vlc-tty-package-") as temporary:
        stage = Path(temporary) / stem
        stage.mkdir()
        shutil.copy2(binary, stage / name)
        (stage / name).chmod(0o755)
        for relative in DOCS:
            output = stage / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, output)
        dependency_notices(stage, target)
        identity = {"version": version, "target": target, "commit": command("git", "rev-parse", "HEAD"),
                    "compiler": command("rustc", "--version"), "binary_sha256": digest(stage / name),
                    "runtime_included": False, "runtime_requires": "LibVLC 3.x with matching plugins"}
        (stage / "BUILD.json").write_text(json.dumps(identity, indent=2) + "\n")
        output = out_dir / (stem + (".zip" if windows else ".tar.gz"))
        create_archive(stage, output, windows)
    print(output)
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", choices=TARGETS)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--out-dir", type=Path)
    args = parser.parse_args()
    package(args.target, args.binary, args.out_dir)
