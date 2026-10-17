"""Kill a headless Windows supervisor and require its Job Object to stop the child."""
import ctypes
from ctypes import wintypes
import http.server
import os
from pathlib import Path
import subprocess
import tempfile
import threading


def smoke(binary):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)

    class ProcessEntry(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD), ("pid", wintypes.DWORD),
                    ("heap", ctypes.c_size_t), ("module", wintypes.DWORD), ("threads", wintypes.DWORD),
                    ("parent", wintypes.DWORD), ("priority", wintypes.LONG), ("flags", wintypes.DWORD),
                    ("exe", wintypes.WCHAR * 260)]

    kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]

    def child_handle(parent):
        snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
        if snapshot == wintypes.HANDLE(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            entry = ProcessEntry()
            entry.size = ctypes.sizeof(entry)
            more = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
            while more:
                if entry.parent == parent:
                    handle = kernel.OpenProcess(0x100000, False, entry.pid)
                    if not handle:
                        raise ctypes.WinError(ctypes.get_last_error())
                    return handle
                more = kernel.Process32NextW(snapshot, ctypes.byref(entry))
        finally:
            kernel.CloseHandle(snapshot)
        raise RuntimeError("Extracted child was not found")

    requested, release = threading.Event(), threading.Event()

    class Stall(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            requested.set()
            release.wait(40)
        def log_message(self, *_args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Stall)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="vlc-tty-job-test-") as temporary:
            env = dict(os.environ, TEMP=temporary, TMP=temporary)
            env.pop("VLC_TTY_RUNTIME", None)
            process = subprocess.Popen([str(binary.resolve()), "--ssh", "--smoke-test", f"http://127.0.0.1:{server.server_port}/stall.mp4"],
                                       env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            handle = None
            try:
                if not requested.wait(35):
                    raise RuntimeError("Extracted player did not reach the loopback server")
                handle = child_handle(process.pid)
                process.kill()
                process.wait(timeout=5)
                if kernel.WaitForSingleObject(handle, 10000) != 0:
                    raise RuntimeError("Extracted player survived forced supervisor termination")
                # TerminateProcess cannot run destructors. TemporaryDirectory
                # removes this test's known private extraction after child exit.
                print("Windows Job Object smoke passed: forced supervisor exit stopped the extracted child")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                if handle:
                    kernel.CloseHandle(handle)
                release.set()
    finally:
        release.set()
        server.shutdown()
        server.server_close()
