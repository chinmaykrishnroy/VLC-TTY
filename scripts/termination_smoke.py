"""Stop the public Unix launcher PID and require child exit and runtime cleanup."""
import os
from pathlib import Path
import select
import signal
import subprocess
import tempfile
import time


def smoke(binary, media):
    import pty
    import fcntl
    import termios
    import struct
    with tempfile.TemporaryDirectory(prefix="vlc-tty-termination-test-") as temporary:
        directory = Path(temporary)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        env = dict(os.environ, TMPDIR=str(directory), TERM="xterm-256color")
        def child_setup():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        process = subprocess.Popen([str(binary.resolve()), "--ssh", "--graphics", "ansi", "--repeat", "all", str(media.resolve())],
                                   stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=child_setup)
        os.close(slave)
        captured = bytearray()

        def collect(seconds):
            until = time.monotonic() + seconds
            while time.monotonic() < until:
                if select.select([master], [], [], 0.05)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError:
                        return
                    if not data:
                        return
                    captured.extend(data)

        child = None
        try:
            deadline = time.monotonic() + 40
            while "▀".encode() not in captured and process.poll() is None and time.monotonic() < deadline:
                collect(0.1)
            if "▀".encode() not in captured:
                raise RuntimeError("Player did not render before termination")
            rows = subprocess.check_output(["ps", "-axo", "pid=,ppid="], text=True)
            children = [int(row.split()[0]) for row in rows.splitlines() if int(row.split()[1]) == process.pid]
            if len(children) != 1:
                raise RuntimeError(f"Expected one extracted child, got {children}")
            child = children[0]
            process.send_signal(signal.SIGTERM)
            collect(8)
            if process.wait(timeout=2) != 0:
                raise RuntimeError("Handled SIGTERM did not exit cleanly")
            try:
                os.kill(child, 0)
            except ProcessLookupError:
                pass
            else:
                raise RuntimeError("Extracted player survived launcher termination")
            if b"\x1b[?1049l" not in captured:
                raise RuntimeError("Termination did not restore the terminal")
            if list(directory.iterdir()):
                raise RuntimeError("Termination left an extracted runtime behind")
            print("SIGTERM smoke passed: extracted child exited, terminal restored, runtime removed")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            if child:
                try:
                    os.kill(child, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            os.close(master)
