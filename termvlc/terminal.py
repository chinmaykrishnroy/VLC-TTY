"""Cross-platform raw terminal input without curses or external packages."""
import os
import select
import sys


class Terminal:
    def __init__(self):
        self._old = None
        self._fd = None
        self._win_old = None

    def __enter__(self):
        if not sys.stdin.isatty() or not sys.stdout.isatty():
            raise RuntimeError("TermVLC must run in an interactive terminal, not a redirected pipe.")
        if os.name == "nt":
            import ctypes
            kernel32 = ctypes.windll.kernel32
            handle = kernel32.GetStdHandle(-11)
            mode = ctypes.c_uint()
            if kernel32.GetConsoleMode(handle, ctypes.byref(mode)):
                self._win_old = (handle, mode.value)
                kernel32.SetConsoleMode(handle, mode.value | 0x0004)  # ENABLE_VIRTUAL_TERMINAL_PROCESSING
        else:
            import termios
            import tty
            self._fd = sys.stdin.fileno()
            self._old = termios.tcgetattr(self._fd)
            tty.setraw(self._fd)
        sys.stdout.write("\x1b[?1049h\x1b[?25l\x1b[0m\x1b[2J")
        sys.stdout.flush()
        return self

    def __exit__(self, *args):
        sys.stdout.write("\x1b[0m\x1b[?25h\x1b[?1049l")
        sys.stdout.flush()
        if self._old is not None:
            import termios
            termios.tcsetattr(self._fd, termios.TCSADRAIN, self._old)
        if self._win_old is not None:
            import ctypes
            ctypes.windll.kernel32.SetConsoleMode(*self._win_old)

    def poll_key(self):
        if os.name == "nt":
            import msvcrt
            if not msvcrt.kbhit():
                return None
            key = msvcrt.getwch()
            if key in ("\x00", "\xe0"):
                arrow = msvcrt.getwch()
                return {"H": "up", "P": "down", "K": "left", "M": "right"}.get(arrow)
            return key
        if not select.select([self._fd], [], [], 0)[0]:
            return None
        raw = os.read(self._fd, 32).decode("utf-8", errors="ignore")
        if raw.startswith("\x1b["):
            return {"A": "up", "B": "down", "C": "right", "D": "left"}.get(raw[2:3], "esc")
        return raw[:1] if raw else None
