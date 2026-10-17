import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from release_manifest import verify_upstream_sources, VLC_URL


class SourceReleaseTests(unittest.TestCase):
    def fixture(self, path, corrupt=False, empty=False):
        original = b"original release source archive"
        dependency = b"preserved contributed source"
        checksum = hashlib.sha256(original).hexdigest()
        record = {"origin": VLC_URL, "sha256": checksum, "original_file": "UPSTREAM-vlc-3.0.24.tar.xz",
                  "contrib_sources": [] if empty else [{"file": "contrib/tarballs/codec.tar.xz", "sha256": hashlib.sha256(dependency).hexdigest()}]}
        files = {"SOURCE_MANIFEST.json": json.dumps(record).encode(), "UPSTREAM-vlc-3.0.24.tar.xz": original,
                 "contrib/tarballs/codec.tar.xz": b"corrupt" if corrupt else dependency,
                 "COPYING": b"GPL", "COPYING.LIB": b"LGPL", "configure.ac": b"build", "contrib/src/main.mak": b"recipes"}
        with tarfile.open(path, "w:gz") as archive:
            for name, data in files.items():
                member = tarfile.TarInfo("vlc-3.0.24-source-snapshot/" + name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
        return checksum

    def test_publication_requires_original_archive_and_intact_contributed_sources(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "source.tar.gz"
            checksum = self.fixture(path)
            with patch("release_manifest.VLC_SHA", checksum):
                verify_upstream_sources(path)
            self.fixture(path, corrupt=True)
            with patch("release_manifest.VLC_SHA", checksum), self.assertRaisesRegex(ValueError, "Contributed source checksum mismatch"):
                verify_upstream_sources(path)
            self.fixture(path, empty=True)
            with patch("release_manifest.VLC_SHA", checksum), self.assertRaisesRegex(ValueError, "Missing contributed"):
                verify_upstream_sources(path)
            self.fixture(path)
            with patch("release_manifest.VLC_SHA", "0" * 64), self.assertRaisesRegex(ValueError, "Wrong upstream source release identity"):
                verify_upstream_sources(path)
