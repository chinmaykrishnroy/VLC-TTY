"""Smoke-test the downloaded single file with hostile external-runtime paths."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from binary_arch import require_architecture
from package_release import ROOT, DOCS, digest
from verify_package import verify_documentation


def verify(executable, target):
    executable = executable.resolve()
    require_architecture(executable, target)
    sidecar = executable.with_name(executable.name + ".sha256")
    if sidecar.read_text().split() != [digest(executable), executable.name]:
        raise RuntimeError("Portable executable checksum mismatch")
    with tempfile.TemporaryDirectory(prefix="vlc-tty-portable-test-") as temporary:
        directory = Path(temporary)
        alone = directory / "folder with spaces" / executable.name
        alone.parent.mkdir()
        shutil.copy2(executable, alone)
        env = dict(os.environ)
        env.update(VLC_HOME=str(directory / "missing-vlc"), VLC_PLUGIN_PATH=str(directory / "missing-plugins"),
                   LD_LIBRARY_PATH=str(directory / "missing-libraries"), TEMP=str(directory), TMP=str(directory), TMPDIR=str(directory))
        env.pop("VLC_TTY_RUNTIME", None)
        env.pop("VLC_TTY_STRICT_RUNTIME", None)
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)

        def run(*args):
            result = subprocess.run([str(alone), *args], cwd=directory, env=env, capture_output=True, text=True, encoding="utf-8", timeout=120)
            if result.returncode != 0:
                raise RuntimeError(f"Portable launch failed {args}: {result.stdout}\n{result.stderr}")
            return result.stdout

        if run("--help") != run("-h"):
            raise RuntimeError("-h and --help must display the same options")
        print(run("--version").strip())
        identity = json.loads(run("--bundle-info"))
        if identity["target"] != target or not identity["runtime_included"]:
            raise RuntimeError("Wrong embedded runtime identity")
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            doctors = list(pool.map(lambda _: run("--ssh", "--doctor"), range(2)))
        for doctor in doctors:
            loaded = next(line for line in doctor.splitlines() if " loaded: " in line)
            if "vlc-tty-" not in loaded or "runtime" not in loaded:
                raise RuntimeError(f"Used an external VLC installation: {loaded}")
            print(loaded)
        decoded = run("--ssh", "--smoke-test", str(ROOT / "samples/sample.mp4"))
        if not re.search(r"Decoded \d+ frames", decoded):
            raise RuntimeError("No decoded frames reported")
        print(decoded.strip())
        overridden = dict(env, VLC_TTY_RUNTIME=str(directory / "intentionally missing runtime"))
        rejected = subprocess.run([str(alone), "--doctor"], cwd=directory, env=overridden, capture_output=True, text=True, timeout=120)
        if rejected.returncode == 0:
            raise RuntimeError("An invalid replacement runtime silently fell back to system VLC")
        leftovers = [path for path in directory.iterdir() if path.name.startswith("vlc-tty-")]
        if leftovers:
            raise RuntimeError(f"Normal exit did not remove extraction directories: {leftovers}")
        extracted = directory / "manual extraction"
        run("--extract-to", str(extracted))
        verify_documentation(extracted / "app", DOCS)
        require_architecture(extracted / "app" / ("vlc-tty.exe" if "windows" in target else "vlc-tty"), target)
        if not (extracted / "BUNDLE.json").is_file():
            raise RuntimeError("Missing embedded build manifest")
        print("Portable smoke passed: architecture, help, concurrent launches, bundled runtime, decoding, cleanup, extraction and docs")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path, nargs="?")
    parser.add_argument("target", nargs="?")
    parser.add_argument("--target", dest="target_option")
    args = parser.parse_args()
    target = args.target_option or args.target
    if not target: parser.error("a target is required")
    if args.executable is None:
        from build_portable import LABELS
        import tomllib
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        args.executable = ROOT / "dist" / (f"vlc-tty-{version}-{LABELS[target]}" + (".exe" if "windows" in target else ""))
    verify(args.executable, target)
