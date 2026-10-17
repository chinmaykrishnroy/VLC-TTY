"""Validate release CPU architecture before attempting native execution."""
import struct
from pathlib import Path

MACHINES = {"x86_64": {"pe": 0x8664, "elf": 62, "mach": 0x01000007},
            "aarch64": {"pe": 0xAA64, "elf": 183, "mach": 0x0100000C}}


def pe_imports(path):
    """Inspect the import table without relying on a developer SDK on the host."""
    data = Path(path).read_bytes()
    if data[:2] != b"MZ":
        raise ValueError("Expected PE executable")
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    sections = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    if struct.unpack_from("<H", data, optional)[0] != 0x20b:
        raise ValueError("Expected PE32+ executable")
    import_rva = struct.unpack_from("<I", data, optional + 120)[0]
    table = optional + optional_size

    def offset(rva):
        for index in range(sections):
            section = table + index * 40
            virtual_size, address, raw_size, raw = struct.unpack_from("<IIII", data, section + 8)
            if address <= rva < address + max(virtual_size, raw_size):
                return raw + rva - address
        raise ValueError(f"Unmapped PE RVA {rva:#x}")

    names = []
    if not import_rva:
        return names
    descriptor = offset(import_rva)
    while any(data[descriptor:descriptor + 20]):
        name_rva = struct.unpack_from("<I", data, descriptor + 12)[0]
        start = offset(name_rva)
        names.append(data[start:data.index(b"\0", start)].decode("ascii"))
        descriptor += 20
    return names


def require_static_crt(path):
    imports = pe_imports(path)
    external = [name for name in imports if name.lower().startswith(("vcruntime", "msvcp", "concrt"))]
    if external:
        raise ValueError(f"Standalone executable requires an installed VC runtime: {path}: {external}")


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
