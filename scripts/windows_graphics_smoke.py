"""Exercise real Windows console detection and dropped audio artwork in ConPTY.

The small terminal responder advertises Sixel, not a Windows Terminal env hint.
No display, third-party Python packages, or sound device is required.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import hashlib
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import sys
import tempfile
import threading
import time
import wave
import urllib.request
import zipfile
import zlib


def prepare_host(directory):
    """Pinned Microsoft test dependency; never included in player releases."""
    version = "1.24.260710001"
    checksum = "175640566a3b59c4b132070ee96c2c77e5ab7edd2e92732a5eb3610bbf63d90e"
    directory.mkdir(parents=True, exist_ok=True)
    package = directory / "conpty.nupkg"
    if not package.is_file():
        url = f"https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/{version}/microsoft.windows.console.conpty.{version}.nupkg"
        with urllib.request.urlopen(url, timeout=60) as response:
            package.write_bytes(response.read())
    assert hashlib.sha256(package.read_bytes()).hexdigest() == checksum, "Microsoft ConPTY package checksum mismatch"
    arch = {"amd64": "x64", "x86_64": "x64", "arm64": "arm64", "aarch64": "arm64"}[platform.machine().lower()]
    with zipfile.ZipFile(package) as archive:
        for source, destination in [(f"runtimes/win-{arch}/native/conpty.dll", "conpty.dll"),
                                    (f"build/native/runtimes/{arch}/OpenConsole.exe", "OpenConsole.exe")]:
            (directory / destination).write_bytes(archive.read(source))
    return directory / "conpty.dll"


class Console:
    def __init__(self, command, library=None, mode="sixel", columns=100, rows=32):
        k = self.k = C.WinDLL("kernel32", use_last_error=True)
        self.host = C.WinDLL(str(library.resolve()), use_last_error=True) if library else k
        class Coord(C.Structure):
            _fields_ = [("x", C.c_short), ("y", C.c_short)]
        class Startup(C.Structure):
            _fields_ = [("size", W.DWORD), ("reserved", W.LPWSTR), ("desktop", W.LPWSTR),
                        ("title", W.LPWSTR), ("x", W.DWORD), ("y", W.DWORD),
                        ("width", W.DWORD), ("height", W.DWORD), ("cols", W.DWORD),
                        ("rows", W.DWORD), ("fill", W.DWORD), ("flags", W.DWORD),
                        ("show", W.WORD), ("count", W.WORD), ("bytes", C.c_void_p),
                        ("stdin", W.HANDLE), ("stdout", W.HANDLE), ("stderr", W.HANDLE)]
        class StartupEx(C.Structure):
            _fields_ = [("startup", Startup), ("attributes", C.c_void_p)]
        class Process(C.Structure):
            _fields_ = [("process", W.HANDLE), ("thread", W.HANDLE),
                        ("pid", W.DWORD), ("tid", W.DWORD)]
        k.CreatePipe.argtypes = [C.POINTER(W.HANDLE), C.POINTER(W.HANDLE), C.c_void_p, W.DWORD]
        self.create_console = self.host.ConptyCreatePseudoConsole if library else k.CreatePseudoConsole
        self.create_console.argtypes = [Coord, W.HANDLE, W.HANDLE, W.DWORD, C.POINTER(W.HANDLE)]
        self.create_console.restype = C.c_long
        self.close_console = self.host.ConptyClosePseudoConsole if library else k.ClosePseudoConsole
        self.close_console.argtypes = [W.HANDLE]
        k.InitializeProcThreadAttributeList.argtypes = [C.c_void_p, W.DWORD, W.DWORD, C.POINTER(C.c_size_t)]
        k.UpdateProcThreadAttribute.argtypes = [C.c_void_p, W.DWORD, C.c_size_t, C.c_void_p, C.c_size_t,
                                               C.c_void_p, C.c_void_p]
        k.CreateProcessW.argtypes = [W.LPCWSTR, W.LPWSTR, C.c_void_p, C.c_void_p, W.BOOL,
                                    W.DWORD, C.c_void_p, W.LPCWSTR, C.POINTER(StartupEx), C.POINTER(Process)]
        k.ReadFile.argtypes = [W.HANDLE, C.c_void_p, W.DWORD, C.POINTER(W.DWORD), C.c_void_p]
        k.WriteFile.argtypes = k.ReadFile.argtypes
        k.WaitForSingleObject.argtypes = [W.HANDLE, W.DWORD]
        k.GetExitCodeProcess.argtypes = [W.HANDLE, C.POINTER(W.DWORD)]
        k.TerminateProcess.argtypes = [W.HANDLE, W.UINT]
        k.CloseHandle.argtypes = [W.HANDLE]
        k.DeleteProcThreadAttributeList.argtypes = [C.c_void_p]
        input_read, self.input = W.HANDLE(), W.HANDLE()
        self.output, output_write = W.HANDLE(), W.HANDLE()
        for read, write in [(input_read, self.input), (self.output, output_write)]:
            if not k.CreatePipe(C.byref(read), C.byref(write), None, 0):
                raise C.WinError(C.get_last_error())
        self.console = W.HANDLE()
        if self.create_console(Coord(columns, rows), input_read, output_write, 0, C.byref(self.console)) != 0:
            raise RuntimeError("CreatePseudoConsole failed")
        size = C.c_size_t()
        k.InitializeProcThreadAttributeList(None, 1, 0, C.byref(size))
        attrs = C.create_string_buffer(size.value)
        if not k.InitializeProcThreadAttributeList(attrs, 1, 0, C.byref(size)):
            raise C.WinError(C.get_last_error())
        try:
            if not k.UpdateProcThreadAttribute(attrs, 0, 0x20016, self.console, C.sizeof(W.HANDLE), None, None):
                raise C.WinError(C.get_last_error())
            startup = StartupEx()
            startup.startup.size = C.sizeof(startup)
            startup.attributes = C.cast(attrs, C.c_void_p)
            process = Process()
            line = C.create_unicode_buffer(command if isinstance(command, str) else
                                           subprocess.list2cmdline([str(arg) for arg in command]))
            if not k.CreateProcessW(None, line, None, None, False, 0x80000, None, None,
                                    C.byref(startup), C.byref(process)):
                raise C.WinError(C.get_last_error())
            self.process = process.process
            k.CloseHandle(process.thread)
            if library:
                self.host.ConptyReleasePseudoConsole.argtypes = [W.HANDLE]
                self.host.ConptyReleasePseudoConsole(self.console)
        finally:
            k.DeleteProcThreadAttributeList(attrs)
            k.CloseHandle(input_read)
            k.CloseHandle(output_write)
        self._data = bytearray()
        self._data_lock = threading.Lock()
        self.cursor_queries = 0
        self.mode = mode
        self.answered = {}
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        buffer = C.create_string_buffer(65536)
        size = W.DWORD()
        while self.k.ReadFile(self.output, buffer, len(buffer), C.byref(size), None) and size.value:
            with self._data_lock:
                self._data.extend(buffer.raw[:size.value])

    @property
    def data(self):
        # Readers receive a stable snapshot while the pipe thread appends.
        # Python may release the GIL while copying/scanning large buffers.
        with self._data_lock:
            return bytes(self._data)

    def send(self, data):
        size = W.DWORD()
        if not self.k.WriteFile(self.input, data, len(data), C.byref(size), None):
            raise C.WinError(C.get_last_error())

    def wait_for(self, predicate, timeout=20):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            data = bytes(self.data)
            # ConPTY itself can ask for the initial cursor position.
            queries = data.count(b"\x1b[6n")
            if queries > self.cursor_queries:
                self.send(b"\x1b[1;1R")
                self.cursor_queries = queries
            if self.mode != "timeout":
                for query, reply in [(b"\x1b[c", b"\x1b[?62;4c" if self.mode == "sixel" else b"\x1b[?1;2c"),
                                     (b"\x1b[16t", b"\x1b[6;20;10t"), (b"\x1b[5n", b"\x1b[0n")]:
                    count = data.count(query)
                    if count > self.answered.get(query, 0):
                        self.send(reply)
                        self.answered[query] = count
            if predicate(data):
                return data
            if self.k.WaitForSingleObject(self.process, 0) == 0:
                time.sleep(0.2)
                raise AssertionError(f"Player exited before expected output: {bytes(self.data)[-1600:]!r}")
            time.sleep(0.01)
        raise AssertionError("Timed out waiting for terminal output")

    def close(self):
        if self.k.WaitForSingleObject(self.process, 0) != 0:
            self.k.TerminateProcess(self.process, 1)
            self.k.WaitForSingleObject(self.process, 5000)
        self.close_console(self.console)
        self.reader.join(timeout=5)
        for handle in [self.input, self.output, self.process]:
            self.k.CloseHandle(handle)


def fixture(folder):
    """Silent WAV and deterministic full-resolution folder artwork."""
    path = folder / "dropped audio.wav"
    with wave.open(str(path), "wb") as audio:
        audio.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
        audio.writeframes(bytes(8000 * 2 * 20))
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    pixels = b"".join(b"\0" + bytes(c for x in range(64) for c in (x * 4, y * 4, 128)) for y in range(64))
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 64, 64, 8, 2, 0, 0, 0))
    (folder / "cover.png").write_bytes(png + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b""))
    return path


def smoke(binary, mode="sixel", host=None, startup_timeout=20):
    for name in ["TERM_PROGRAM", "TERM", "KITTY_WINDOW_ID", "NO_COLOR"]:
        os.environ.pop(name, None)
    with tempfile.TemporaryDirectory(prefix="vlc-tty-art-") as temporary:
        path = fixture(Path(temporary))
        # CI redirects Python's standard handles. The child bootstrap replaces
        # those with read/write handles to this new console before launching VLC.
        terminal = Console([sys.executable, Path(__file__).resolve(), "--console-child", binary.resolve()], host, mode)
        try:
            data = terminal.wait_for(lambda data: b"\x1b[16t" in data or b"ANSI half-block" in data,
                                     timeout=startup_timeout)
            assert b"\x1b[16t" in data, "Auto graphics never queried Windows console capabilities (ANSI regression)"
            if mode == "sixel":
                terminal.wait_for(lambda data: b"Sixel pixel graphics" in data)
                # Windows Terminal drops a quoted path as ordinary key events.
                start = len(terminal.data)
                terminal.send(('"' + str(path) + '"').encode())
                terminal.wait_for(lambda data: re.search(rb"\x1bP[0-9;]*q", data[start:]))
            elif mode == "no-sixel":
                terminal.wait_for(lambda data: b"ANSI half-block" in data)
            elif mode == "timeout":
                # An early shortcut must survive the unanswered capability probe.
                terminal.send(b"?")
                terminal.wait_for(lambda data: b"KEYBOARD CONTROLS" in data)
                assert b"ANSI half-block" in terminal.data, "Unanswered query did not retain ANSI fallback"
                terminal.send(b"\x1b")
            elif mode == "menu-hover":
                terminal.wait_for(lambda data: b"ANSI half-block" in data)
                start = len(terminal.data)
                terminal.send(b"\x1b[<35;12;1M")
                terminal.wait_for(lambda data: b"48;2;48;54;62" in data[start:])
                assert b"Play / Pause" not in terminal.data[start:], "Closed menu opened on hover"
                terminal.send(b"\x1b[<0;12;1M\x1b[<0;12;1m")
                terminal.wait_for(lambda data: b"Play / Pause" in data[start:])
                start = len(terminal.data)
                terminal.send(b"\x1b[<35;12;4M")
                terminal.wait_for(lambda data: b"48;2;74;203;216" in data[start:] and b"Stop" in data[start:])
                start = len(terminal.data)
                terminal.send(b"\x1b[<35;22;1M")
                terminal.wait_for(lambda data: b"Cycle Audio Track" in data[start:] and b"Volume Up" in data[start:])
                terminal.send(b"\x1b")
            terminal.send(b"q")
            assert terminal.k.WaitForSingleObject(terminal.process, 10000) == 0, "Quit key was lost after probing"
            code = W.DWORD()
            terminal.k.GetExitCodeProcess(terminal.process, C.byref(code))
            assert code.value == 0, f"Player failed with exit code {code.value}"
            assert b"\x1b[?1049l" in terminal.data, "Alternate screen was not restored"
            checked = (
                "menu hover and header switching" if mode == "menu-hover"
                else "dropped Sixel artwork" if mode == "sixel"
                else "fallback and startup input"
            )
            print(f"Windows console graphics smoke passed: {mode}; {checked}, quit and terminal restoration")
        finally:
            terminal.close()


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] in {"--console-child", "--console-child-audio"}:
        # Open read/write console handles: cmd's write-only CONOUT$ handle
        # fails GetConsoleMode and Rust's IsTerminal check.
        incoming = os.open("CONIN$", os.O_RDWR | os.O_BINARY)
        outgoing = os.open("CONOUT$", os.O_RDWR | os.O_BINARY)
        try:
            audio = ["--audio", "--volume", "0"] if sys.argv[1] == "--console-child-audio" else ["--silent"]
            raise SystemExit(subprocess.call([sys.argv[2], *sys.argv[3:], *audio], stdin=incoming,
                                              stdout=outgoing, stderr=outgoing))
        finally:
            os.close(incoming)
            os.close(outgoing)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--mode", choices=["sixel", "no-sixel", "timeout", "menu-hover"], default="sixel")
    parser.add_argument("--host-directory", type=Path,
                        help="Download checksum-pinned modern Microsoft ConPTY into this test directory")
    parser.add_argument("--startup-timeout", type=float, default=20,
                        help="Allow extra startup time for a cold portable extraction")
    args = parser.parse_args()
    smoke(args.binary, args.mode, prepare_host(args.host_directory) if args.host_directory else None,
          args.startup_timeout)
