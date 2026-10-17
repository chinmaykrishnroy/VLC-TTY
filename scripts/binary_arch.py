"""Validate release CPU architecture before attempting native execution."""
import struct
from pathlib import Path

MACHINES = {"x86_64": {"pe": 0x8664, "elf": 62, "mach": 0x01000007},
            "aarch64": {"pe": 0xAA64, "elf": 183, "mach": 0x0100000C}}


def architecture(path):
    data = Path(path).read_bytes()
    if data[:2] == b"MZ":
        offset = struct.unpack_from("<I", data, 0x3C)[0]
        if data[offset:offset+4] != b"PE\0\0":
            raise ValueError("Invalid PE signature")
        kind, machine = "pe", struct.unpack_from("<H", data, offset + 4)[0]
    elif data[:4] == b"\x7fELF":
        if data[4:6] != b"\x02\x01":
            raise ValueError("Expected a little-endian 64-bit ELF")
        kind, machine = "elf", struct.unpack_from("<H", data, 18)[0]
    elif data[:4] == b"\xcf\xfa\xed\xfe":
        kind, machine = "mach", struct.unpack_from("<I", data, 4)[0]
    else:
        raise ValueError(f"Unrecognized native executable: {path}")
    for name, formats in MACHINES.items():
        if formats[kind] == machine:
            return name
    raise ValueError(f"Unsupported {kind} machine {machine:#x}")


def require_architecture(path, target):
    expected = target.split("-", 1)[0]
    actual = architecture(path)
    if actual != expected:
        raise ValueError(f"Wrong architecture: {path}: {actual}; target requires {expected}")
    return actual
