import struct
import tempfile
from pathlib import Path
import unittest
from binary_arch import architecture, require_architecture, require_static_crt


class ArchitectureTests(unittest.TestCase):
    def test_import_gate_detects_a_missing_vc_redistributable_dependency(self):
        data = bytearray(1024)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 0x3c, 128)
        data[128:132] = b"PE\0\0"
        struct.pack_into("<H", data, 134, 1)
        struct.pack_into("<H", data, 148, 240)
        struct.pack_into("<H", data, 152, 0x20b)
        struct.pack_into("<I", data, 272, 0x1000)
        struct.pack_into("<IIII", data, 400, 512, 0x1000, 512, 512)
        struct.pack_into("<I", data, 524, 0x1040)
        name = b"VCRUNTIME140.dll\0"
        data[576:576+len(name)] = name
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "launcher.exe"
            path.write_bytes(data)
            with self.assertRaisesRegex(ValueError, "installed VC runtime"):
                require_static_crt(path)
            data[576:576+len(name)] = b"KERNEL32.dll\0".ljust(len(name), b"\0")
            path.write_bytes(data)
            require_static_crt(path)
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
