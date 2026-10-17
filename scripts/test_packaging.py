import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile
import tarfile
from package_release import create_archive, digest, package, ROOT, TARGETS
from release_manifest import manifest
from verify_package import verify_documentation


class PackagingTests(unittest.TestCase):
    def test_packaged_documentation_links_resolve_inside_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "README.md").write_text("[Guide](docs/SSH.md#setup) [Website](https://example.com)\n")
            with self.assertRaisesRegex(RuntimeError, "Broken packaged documentation link"):
                verify_documentation(root, ["README.md"])
            (root / "docs").mkdir()
            (root / "docs/SSH.md").write_text("[Home](../README.md)\n")
            verify_documentation(root, ["README.md", "docs/SSH.md"])

    def test_archive_preserves_files_permissions_and_checksum(self):
        for windows in (True, False):
            with self.subTest(windows=windows), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                stage = directory / "vlc-tty-test"
                stage.mkdir()
                binary = stage / "vlc-tty"
                binary.write_bytes(b"test executable")
                binary.chmod(0o755)
                # Cargo license files can carry the Unix epoch, before ZIP's 1980 minimum.
                import os
                os.utime(binary, (0, 0))
                (stage / "BUILD.json").write_text(json.dumps({"runtime_included": False}))
                output = directory / ("build.zip" if windows else "build.tar.gz")
                create_archive(stage, output, windows)
                self.assertEqual(output.with_name(output.name + ".sha256").read_text().split()[0], digest(output))
                if windows:
                    with zipfile.ZipFile(output) as archive:
                        self.assertEqual(archive.read("vlc-tty-test/vlc-tty"), binary.read_bytes())
                else:
                    with tarfile.open(output) as archive:
                        self.assertEqual(archive.getmember("vlc-tty-test/vlc-tty").mode & 0o111, 0o111)

    def test_release_tag_must_match_package_version(self):
        with patch.dict("os.environ", {"GITHUB_REF": "refs/tags/v999.0.0"}):
            with self.assertRaisesRegex(ValueError, "Tag must match"):
                package("x86_64-unknown-linux-gnu")

    def test_runtime_hashes_and_urls_are_architecture_specific(self):
        spec = json.loads((ROOT / "scripts/runtime-manifest.json").read_text())
        self.assertEqual(len(spec), 4)
        self.assertEqual(len({item["sha256"] for item in spec.values()}), 4)
        for item in spec.values():
            self.assertEqual(len(bytes.fromhex(item["sha256"])), 32)
            self.assertTrue(item["url"].startswith("https://download.videolan.org/"))

    def test_publication_requires_complete_uncorrupted_archive_set(self):
        import tomllib
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        with tempfile.TemporaryDirectory() as temporary, patch.dict("os.environ", {"GITHUB_REF": f"refs/tags/v{version}"}):
            directory = Path(temporary)
            with self.assertRaisesRegex(ValueError, "Wrong release archive set"):
                manifest(directory)
            for target in TARGETS:
                path = directory / (f"vlc-tty-{version}-{target}" + (".zip" if "windows" in target else ".tar.gz"))
                path.write_bytes(target.encode())
                path.with_name(path.name + ".sha256").write_text(f"{digest(path)}  {path.name}\n")
            manifest(directory)
            self.assertEqual(len((directory / "SHA256SUMS").read_text().splitlines()), 6)
            path.write_bytes(b"corrupted archive")
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                manifest(directory)


if __name__ == "__main__":
    unittest.main()
