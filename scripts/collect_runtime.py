"""Collect a coherent runtime and record copied native components."""
import hashlib
import json
import os
import platform
from pathlib import Path
import re
import shutil
import subprocess


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def owner(path):
    candidates = [str(path)]
    if str(path).startswith("/usr/lib/"):
        candidates.append(str(path)[4:])
    elif str(path).startswith("/lib/"):
        candidates.append("/usr" + str(path))
    for name in candidates:
        result = subprocess.run(["dpkg-query", "-S", name], text=True, capture_output=True)
        if result.returncode == 0:
            return result.stdout.split(": ", 1)[0].splitlines()[0]
    raise RuntimeError(f"Cannot identify the distro package owning {path}")


def package_identity(package):
    fields = output("dpkg-query", "-W", "-f=${binary:Package}\t${source:Package}\t${source:Version}\t${Version}", package).split("\t")
    return dict(zip(("binary_package", "source_package", "source_version", "binary_version"), fields))


def collect_linux(destination):
    listing = output("dpkg", "-L", "libvlc5").splitlines()
    libvlc = next(Path(name) for name in listing if name.endswith("/libvlc.so.5"))
    plugin_root = libvlc.parent / "vlc/plugins"
    if not plugin_root.is_dir():
        raise RuntimeError("LibVLC base plugins are missing")
    libraries = {libvlc}
    plugins = list(plugin_root.rglob("*.so"))
    packages = {"libvlc5", "libvlccore9", "vlc-plugin-base", "vlc-data"}
    for binary in [libvlc, *plugins]:
        linked = subprocess.run(["ldd", str(binary)], text=True, capture_output=True, check=True).stdout
        if "not found" in linked:
            raise RuntimeError(f"Unresolved runtime dependencies for {binary}: {linked}")
        libraries.update(Path(match) for match in re.findall(r"=>\s+(/\S+)", linked))
    library_dir = destination / "lib"
    library_dir.mkdir(parents=True)
    for library in sorted(libraries):
        package = owner(library)
        identity = package_identity(package)
        # The ELF interpreter and glibc remain base-OS requirements. Copying them
        # without their loader would produce incompatible mixed glibc installs.
        if identity["source_package"] == "glibc":
            continue
        target = library_dir / library.name
        if target.exists() and digest(target) != digest(library):
            raise RuntimeError(f"Conflicting runtime SONAME: {library.name}")
        shutil.copy2(library.resolve(), target)
        packages.add(package)
    shutil.copytree(plugin_root, library_dir / "vlc/plugins", symlinks=True)
    shutil.copytree("/usr/share/vlc", destination / "share/vlc", symlinks=True)
    # Keep licenses for copied packages, including the shared license texts that
    # distro copyright files reference. Source packages are collected separately.
    identities = []
    for package in sorted(packages):
        identity = package_identity(package)
        identities.append(identity)
        name = package.split(":", 1)[0]
        notice = Path("/usr/share/doc") / name / "copyright"
        if not notice.exists():
            raise RuntimeError(f"Missing runtime copyright notice: {package}")
        target = destination / "licenses" / name / "copyright"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(notice, target)
    shutil.copytree("/usr/share/common-licenses", destination / "licenses/common", symlinks=False)
    return {"origin": "Ubuntu distribution packages", "glibc_baseline": platform.libc_ver()[1], "packages": identities}


def collect(destination, target, runtime_home):
    if "linux" in target:
        return collect_linux(destination)
    runtime_home = Path(runtime_home)
    if "windows" in target:
        if not (runtime_home / "libvlc.dll").is_file():
            raise RuntimeError("Expected a complete Windows VLC runtime")
        shutil.copytree(runtime_home, destination, symlinks=True)
    else:
        if not (runtime_home / "Contents/MacOS/lib/libvlc.dylib").is_file():
            raise RuntimeError("Expected a complete VLC 3 macOS app bundle")
        destination.mkdir()
        shutil.copytree(runtime_home, destination / "VLC.app", symlinks=True)
    spec = json.loads(Path(__file__).with_name("runtime-manifest.json").read_text())[target]
    return {"origin": spec["url"], "upstream_sha256": spec["sha256"], "runtime": "VLC 3.0.24"}
