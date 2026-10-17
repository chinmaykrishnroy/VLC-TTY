import struct
import tempfile
from pathlib import Path
import unittest
from binary_arch import architecture, require_architecture


class ArchitectureTests(unittest.TestCase):
    def test_reported_arm64_windows_file_is_rejected_for_x64_target(self):
        data = bytearray(256)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 0x3c, 128)
        data[128:132] = b"PE\0\0"
        struct.pack_into("<H", data, 132, 0xAA64)
        with tempfile.TemporaryDirectory() as folder:
            file = Path(folder) / "vlc-tty.exe"
            file.write_bytes(data)
            self.assertEqual(architecture(file), "aarch64")
            with self.assertRaisesRegex(ValueError, "Wrong architecture"):
                require_architecture(file, "x86_64-pc-windows-msvc")
            require_architecture(file, "aarch64-pc-windows-msvc")


if __name__ == "__main__":
    unittest.main()
