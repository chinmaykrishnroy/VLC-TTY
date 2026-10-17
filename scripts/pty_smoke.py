"""Exercise the real ANSI TUI in a Unix PTY. This is a byte/interaction check, not visual acceptance."""
import argparse
import json
import os
from pathlib import Path
import select
import struct
import subprocess
import time


def smoke(binary, media, duration=12, expect_caption=None):
    import fcntl
    import pty
    import termios

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 100, 0, 0))

    def child_setup():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    env = {key: value for key, value in os.environ.items() if key not in ("DISPLAY", "WAYLAND_DISPLAY")}
    env["TERM"] = "xterm-256color"
    process = subprocess.Popen([str(binary.resolve()), "--ssh", "--graphics", "ansi", str(media.resolve())],
                               stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=child_setup)
    os.close(slave)
    captured = bytearray()
    started = time.monotonic()
    peak_rss_kib = 0
    cpu_ticks = 0

    def collect(seconds):
        nonlocal peak_rss_kib, cpu_ticks
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if select.select([master], [], [], min(0.05, max(0, until - time.monotonic())))[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                captured.extend(data)
                if len(captured) > 64 * 1024 * 1024:
                    raise RuntimeError("PTY capture exceeded 64 MiB")
            try:
                status = Path(f"/proc/{process.pid}/status").read_text()
                peak_rss_kib = max(peak_rss_kib, int(next(line.split()[1] for line in status.splitlines() if line.startswith("VmRSS:"))))
                fields = Path(f"/proc/{process.pid}/stat").read_text().rsplit(")", 1)[1].split()
                cpu_ticks = int(fields[11]) + int(fields[12])
            except (FileNotFoundError, StopIteration):
                pass

    def send(keys, delay=0.35):
        os.write(master, keys)
        collect(delay)
        if process.poll() is not None:
            raise RuntimeError(f"Player exited during controls: {process.returncode}")

    try:
        collect(1.5)
        send(b"f")                         # Fullscreen.
        send(b" ")                         # Pause.
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
        collect(0.3)                        # Force a complete paused redraw.
        paused_was_visible = b"PAUSED" in captured
        send(b"e")                         # Step while paused.
        send(b" ")                         # Resume.
        send(b"\x1b[C")                    # Seek forward.
        send(b"?")                         # Help overlay.
        help_was_visible = b"PLAYBACK CONTROLS" in captured
        send(b"\x1b")                      # Close overlay.
        send(b"f")                         # Return to normal layout.
        send(b"\t")                        # Queue view.
        queue_was_visible = b"PLAYBACK QUEUE" in captured
        send(b"\t")
        send(b"o")                         # Terminal file input, including over SSH.
        send(str(media.resolve()).encode() + b"\r", 1)
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        collect(max(0.5, duration - (time.monotonic() - started)))
        elapsed = time.monotonic() - started
        os.write(master, b"q")
        collect(2)
        code = process.wait(timeout=5)
        result = {
            "exit_code": code, "seconds": round(elapsed, 2), "terminal_bytes": len(captured),
            "ansi_half_blocks": captured.count("▀".encode()), "paused_redraw": paused_was_visible,
            "help_overlay": help_was_visible, "queue_view": queue_was_visible,
            "alternate_screen_restored": b"\x1b[?1049l" in captured,
            "peak_rss_mib": round(peak_rss_kib / 1024, 2) if peak_rss_kib else None,
            "average_cpu_one_core_percent": round(cpu_ticks / os.sysconf("SC_CLK_TCK") / elapsed * 100, 2) if cpu_ticks else None,
        }
        if expect_caption:
            result["caption_present"] = expect_caption.encode() in captured
        print(json.dumps(result, indent=2))
        checks = [code == 0, result["ansi_half_blocks"] > 0, paused_was_visible,
                  help_was_visible, queue_was_visible, result["alternate_screen_restored"]]
        if expect_caption:
            checks.append(result["caption_present"])
        if not all(checks):
            raise RuntimeError("Interactive PTY acceptance failed; inspect the reported checks")
        return result
    except Exception:
        tail = bytes(captured[-4000:]).decode("utf-8", errors="replace")
        error_at = tail.rfind("Error:")
        print("Player error output:", repr(tail[error_at:] if error_at >= 0 else tail[-1000:]))
        raise
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
        os.close(master)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("media", type=Path)
    parser.add_argument("--duration", type=float, default=12)
    parser.add_argument("--expect-caption")
    args = parser.parse_args()
    smoke(args.binary, args.media, args.duration, args.expect_caption)
