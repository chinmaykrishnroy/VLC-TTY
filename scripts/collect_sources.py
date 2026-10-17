"""Preserve runtime source snapshots and the provenance used to obtain them."""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request

VLC_URL = "https://download.videolan.org/pub/videolan/vlc/3.0.24/vlc-3.0.24.tar.xz"
VLC_SHA = "e7cab503d1d7d5849b89d2cf0e1ee60d0ef6d012407791b644b9cfc0cc225fdf"


def download(url, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=120) as response, path.open("wb") as output:
                shutil.copyfileobj(response, output)
            return
        except Exception:
            if attempt == 2:
                raise
            time.sleep(2)


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def json_url(url):
    with urllib.request.urlopen(url, timeout=90) as response:
        return json.load(response)


def linux(packages, output):
    unique = {(item["source_package"], item["source_version"]) for item in packages}
    with tempfile.TemporaryDirectory(prefix="vlc-tty-linux-sources-") as temporary:
        root = Path(temporary)

        def fetch(item):
            name, version = item
            query = urllib.parse.urlencode({"ws.op": "getPublishedSources", "source_name": name,
                                           "version": version, "exact_match": "true", "ws.size": "100"})
            entries = json_url("https://api.launchpad.net/1.0/ubuntu/+archive/primary?" + query)["entries"]
            entry = next((entry for entry in entries if entry["source_package_name"] == name
                          and entry["source_package_version"] == version), None)
            if entry is None:
                raise RuntimeError(f"Exact Ubuntu source not found: {name} {version}")
            urls = json_url(entry["self_link"] + "?ws.op=sourceFileUrls")
            directory = root / (name + "-" + version.replace(":", "_"))
            records = []
            for url in urls:
                path = directory / urllib.parse.unquote(urllib.parse.urlsplit(url).path.rsplit("/", 1)[1])
                download(url, path)
                records.append({"file": str(path.relative_to(root)), "url": url, "sha256": sha(path)})
            if not any(record["file"].endswith(".dsc") for record in records):
                raise RuntimeError(f"Missing signed source description for {name} {version}")
            print(f"Preserved exact Ubuntu source: {name} {version}", flush=True)
            return {"package": name, "version": version, "launchpad": entry["self_link"], "files": records}

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            records = list(pool.map(fetch, sorted(unique)))
        (root / "SOURCE_MANIFEST.json").write_text(json.dumps(records, indent=2) + "\n")
        output.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(output, "w:gz") as archive:
            archive.add(root, arcname="runtime-sources")
    output.with_name(output.name + ".sha256").write_text(f"{sha(output)}  {output.name}\n")
    return {"file": output.name, "sha256": sha(output), "kind": "exact Ubuntu source package versions"}


def upstream(output):
    with tempfile.TemporaryDirectory(prefix="vlc-tty-upstream-sources-") as temporary:
        root = Path(temporary)
        asset = root / "vlc-3.0.24.tar.xz"
        download(VLC_URL, asset)
        if sha(asset) != VLC_SHA:
            raise RuntimeError("VLC source archive checksum mismatch")
        with tarfile.open(asset) as archive:
            archive.extractall(root, filter="data")
        source = root / "vlc-3.0.24"
        fetch = source / "contrib/source-fetch"
        fetch.mkdir()
        # The release recipe writes this abbreviated pin into a .githash file,
        # then its generic checker insists on 40 hex digits. Expand only the
        # verified same commit; retain the adjustment in the source manifest.
        tremor_pin = "b56ffce0c0773ec5ca04c466bc00b1bbcaf65aef"
        recipe = source / "contrib/src/tremor/rules.mak"
        original = recipe.read_text()
        if "b56ffce0" not in original:
            raise RuntimeError("Unexpected Tremor source recipe; review the pin adjustment")
        recipe.write_text(original.replace("b56ffce0", tremor_pin))
        subprocess.run(["../bootstrap", "--build=x86_64-linux-gnu", "--host=x86_64-w64-mingw32"], cwd=fetch, check=True)
        # Fetch source only. Native compilers answer configuration probes; no
        # target library is compiled. fetch-all deliberately preserves a superset.
        subprocess.run(["make", "-j4", "fetch-all", "CC=cc", "CXX=c++", "MINGW_W64_VERSION=14",
                        "CONTRIB_VIDEOLAN=https://download.videolan.org/pub/contrib",
                        "VIDEOLAN=https://download.videolan.org/pub/videolan", "GNU=https://ftpmirror.gnu.org/gnu"], cwd=fetch, check=True)
        records = [{"file": str(path.relative_to(source)), "sha256": sha(path)}
                   for path in sorted((source / "contrib/tarballs").rglob("*")) if path.is_file()]
        if not records:
            raise RuntimeError("No contributed dependency sources collected")
        original_file = "UPSTREAM-vlc-3.0.24.tar.xz"
        shutil.copy2(asset, source / original_file)
        manifest = {"origin": VLC_URL, "sha256": VLC_SHA, "contrib_sources": records,
                    "original_file": original_file,
                    "recipe_adjustments": {"contrib/src/tremor/rules.mak": {"original_pin": "b56ffce0", "expanded_pin": tremor_pin}},
                    "scope": "VLC release source plus checksum/git-pinned contrib recipe source superset; not a claim of bit-reproducible upstream binaries"}
        (source / "SOURCE_MANIFEST.json").write_text(json.dumps(manifest, indent=2) + "\n")
        shutil.rmtree(fetch)
        output.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(output, "w:gz") as archive:
            archive.add(source, arcname="vlc-3.0.24-source-snapshot")
    output.with_name(output.name + ".sha256").write_text(f"{sha(output)}  {output.name}\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, required=True)
    args = parser.parse_args()
    upstream(args.upstream)
