"""Install coherent runtimes for CI only; never copy them into release artifacts."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile


def install(target):
    if "linux" in target:
        subprocess.run(["sudo", "apt-get", "update"], check=True)
        subprocess.run(["sudo", "apt-get", "install", "-y", "--no-install-recommends",
                        "libvlc5", "vlc-plugin-base"], check=True)
        return
    spec = json.loads(Path(__file__).with_name("runtime-manifest.json").read_text())[target]
    root = Path(tempfile.mkdtemp(prefix="vlc-tty-runtime-", dir=os.environ.get("RUNNER_TEMP")))
    asset = root / spec["url"].rsplit("/", 1)[1]
    with urllib.request.urlopen(spec["url"], timeout=120) as response, asset.open("wb") as output:
        shutil.copyfileobj(response, output)
    with asset.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != spec["sha256"]:
        raise RuntimeError(f"Runtime checksum mismatch: {actual}")
    if "windows" in target:
        with zipfile.ZipFile(asset) as archive:
            archive.extractall(root)
        matches = list(root.glob("*/libvlc.dll"))
        if len(matches) != 1:
            raise RuntimeError("Expected one complete upstream VLC directory")
        home = matches[0].parent
    else:
        mount = root / "mount"
        mount.mkdir()
        subprocess.run(["hdiutil", "attach", str(asset), "-readonly", "-nobrowse",
                        "-mountpoint", str(mount)], check=True)
        try:
            home = root / "VLC.app"
            subprocess.run(["ditto", str(mount / "VLC.app"), str(home)], check=True)
        finally:
            subprocess.run(["hdiutil", "detach", str(mount)], check=True)
    env_file = os.environ["GITHUB_ENV"]
    with open(env_file, "a", encoding="utf-8") as output:
        output.write(f"VLC_HOME={home}\n")
    print(f"Verified upstream runtime: {asset.name} SHA256={actual}; VLC_HOME={home}")


if __name__ == "__main__":
    install(sys.argv[1])
