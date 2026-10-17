"""Build one self-extracting executable per OS/CPU, without an installer."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from binary_arch import require_architecture
from collect_runtime import collect
from collect_sources import linux as linux_sources
from package_release import ROOT, TARGETS, digest, package

LABELS = {"x86_64-unknown-linux-gnu": "linux-x64", "aarch64-unknown-linux-gnu": "linux-arm64",
          "x86_64-pc-windows-msvc": "windows-x64", "aarch64-pc-windows-msvc": "windows-arm64",
          "x86_64-apple-darwin": "macos-x64", "aarch64-apple-darwin": "macos-arm64"}


def build(target, runtime_home=None, with_sources=False):
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    env = dict(os.environ)
    for key in ("VLC_TTY_BUNDLE", "VLC_TTY_BUNDLE_SHA256", "VLC_TTY_BUNDLE_INFO"):
        env.pop(key, None)
    subprocess.run(["cargo", "build", "--locked", "--release", "--target", target, "--bin", "vlc-tty"], cwd=ROOT, env=env, check=True)
    suffix = ".exe" if "windows" in target else ""
    binary = ROOT / "target" / target / "release" / ("vlc-tty" + suffix)
    require_architecture(binary, target)
    out = ROOT / "dist"
    out.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="vlc-tty-bundle-") as temporary:
        temporary = Path(temporary)
        archive = package(target, binary, temporary / "thin")
        unpacked = temporary / "thin-unpacked"
        shutil.unpack_archive(archive, unpacked)
        payload = temporary / "payload"
        payload.mkdir()
        shutil.move(next(unpacked.iterdir()), payload / "app")
        previous_application = os.environ.get("VLC_TTY_COLLECT_APPLICATION")
        os.environ["VLC_TTY_COLLECT_APPLICATION"] = str(binary)
        try:
            runtime = collect(payload / "runtime", target, runtime_home or os.environ.get("VLC_HOME", ""))
        finally:
            if previous_application is None:
                os.environ.pop("VLC_TTY_COLLECT_APPLICATION", None)
            else:
                os.environ["VLC_TTY_COLLECT_APPLICATION"] = previous_application
        if with_sources and "linux" in target:
            runtime["source_companion"] = linux_sources(runtime["packages"], out / f"vlc-tty-{version}-{LABELS[target]}-runtime-sources.tar.gz")
        elif with_sources:
            runtime["source_companion"] = {"file": "vlc-3.0.24-source-snapshot.tar.gz",
                "kind": "upstream release source with contributed-dependency source snapshot, distributed alongside binaries"}
        if "windows" in target:
            require_architecture(payload / "runtime/libvlc.dll", target)
        elif "linux" in target:
            require_architecture(payload / "runtime/lib/libvlc.so.5", target)
        else:
            require_architecture(payload / "runtime/VLC.app/Contents/MacOS/lib/libvlc.dylib", target)
        inventory = {str(path.relative_to(payload)).replace("\\", "/"): digest(path)
                     for path in sorted((payload / "runtime").rglob("*")) if path.is_file() and not path.is_symlink()}
        (payload / "RUNTIME_FILES.json").write_text(json.dumps(inventory, indent=2) + "\n")
        identity = {"application": "VLC-TTY", "version": version, "target": target,
                    "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                    "runtime_included": True, "runtime": runtime, "extraction": "private temporary directory per run",
                    "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()),
                    "license": "MIT application; runtime components retain their own licenses"}
        (payload / "BUNDLE.json").write_text(json.dumps(identity, indent=2) + "\n")
        packed = temporary / "payload.tar.gz"
        with tarfile.open(packed, "w:gz") as tar:
            for path in sorted(payload.iterdir()):
                tar.add(path, arcname=path.name)
        env.update(VLC_TTY_BUNDLE=str(packed), VLC_TTY_BUNDLE_SHA256=digest(packed), VLC_TTY_BUNDLE_INFO=json.dumps(identity))
        subprocess.run(["cargo", "build", "--locked", "--release", "--target", target, "--bin", "vlc-tty-portable"], cwd=ROOT, env=env, check=True)
        executable = out / f"vlc-tty-{version}-{LABELS[target]}{suffix}"
        shutil.copy2(ROOT / "target" / target / "release" / ("vlc-tty-portable" + suffix), executable)
        executable.chmod(0o755)
        require_architecture(executable, target)
        executable.with_name(executable.name + ".sha256").write_text(f"{digest(executable)}  {executable.name}\n")
        executable.with_name(executable.name + ".json").write_text(json.dumps(identity, indent=2) + "\n")
        print(executable)
        return executable


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", choices=TARGETS)
    parser.add_argument("--runtime-home", type=Path)
    parser.add_argument("--with-sources", action="store_true")
    args = parser.parse_args()
    build(args.target, args.runtime_home, args.with_sources)
