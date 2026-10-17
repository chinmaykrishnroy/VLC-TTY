"""Exercise the real ANSI TUI in a Unix PTY. This is a byte/interaction check, not visual acceptance."""
import argparse
import json
import os
from pathlib import Path
import select
import struct
import subprocess
import time


def smoke(binary, media, duration=12, expect_caption=None, audio_endpoint=None, audio_token_file=None, metrics=None, repeat="off", steady_audio=False):
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
    audio = (["--audio-output", "remote", "--audio-endpoint", audio_endpoint,
              "--audio-token-file", str(audio_token_file)] if audio_endpoint and audio_token_file else ["--silent"])
    profile = ["--metrics", str(metrics)] if metrics else []
    process = subprocess.Popen([str(binary.resolve()), "--ssh", *audio, *profile, "--graphics", "ansi", "--repeat", repeat, str(media.resolve())],
                               stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=child_setup)
    os.close(slave)
    captured = bytearray()
    total_bytes = half_blocks = 0
    carry = b""
    restored = caption_seen = False
    started = time.monotonic()
    peak_rss_kib = 0
    cpu_ticks = 0

    def process_tree(pid):
        result = [pid]
        try:
            children = Path(f"/proc/{pid}/task/{pid}/children").read_text().split()
            for child in children:
                result.extend(process_tree(int(child)))
        except FileNotFoundError:
            pass
        return result

    def collect(seconds):
        nonlocal peak_rss_kib, cpu_ticks, total_bytes, half_blocks, carry, restored, caption_seen
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if select.select([master], [], [], min(0.05, max(0, until - time.monotonic())))[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                total_bytes += len(data)
                combined = carry + data
                half_blocks += combined.count("▀".encode()) - carry.count("▀".encode())
                restored |= b"\x1b[?1049l" in combined
                if expect_caption:
                    caption_seen |= expect_caption.encode() in combined
                carry = combined[-max(256, len(expect_caption.encode()) if expect_caption else 0):]
                captured.extend(data)
                del captured[:-4 * 1024 * 1024]
            rss, ticks = 0, 0
            for pid in process_tree(process.pid):
                try:
                    status = Path(f"/proc/{pid}/status").read_text()
                    rss += int(next(line.split()[1] for line in status.splitlines() if line.startswith("VmRSS:")))
                    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
                    ticks += int(fields[11]) + int(fields[12])
                except (FileNotFoundError, StopIteration):
                    pass
            peak_rss_kib = max(peak_rss_kib, rss)
            cpu_ticks = max(cpu_ticks, ticks)

    def send(keys, delay=0.35):
        os.write(master, keys)
        collect(delay)
        if process.poll() is not None:
            raise RuntimeError(f"Player exited during controls: {process.returncode}")

    try:
        # Startup notifications occupy the OSD for four seconds. Let them expire
        # before asserting the paused-state label rather than the notification.
        paused_was_visible = help_was_visible = queue_was_visible = False
        if not steady_audio:
            collect(4.3)
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
            "exit_code": code, "seconds": round(elapsed, 2), "terminal_bytes": total_bytes,
            "ansi_half_blocks": half_blocks, "paused_redraw": paused_was_visible,
            "help_overlay": help_was_visible, "queue_view": queue_was_visible,
            "alternate_screen_restored": restored,
            "peak_rss_mib": round(peak_rss_kib / 1024, 2) if peak_rss_kib else None,
            "average_cpu_one_core_percent": round(cpu_ticks / os.sysconf("SC_CLK_TCK") / elapsed * 100, 2) if cpu_ticks else None,
            "resource_scope": "launcher and descendants; summed RSS may count shared pages twice" if peak_rss_kib else None,
        }
        if metrics and metrics.exists():
            lines = metrics.read_text().splitlines()
            result["last_application_sample"] = json.loads(lines[-1]) if lines else None
            if steady_audio:
                samples = [json.loads(line) for line in lines]
                steady = [s for s in samples if s["elapsed_ms"] >= 4000]
                if len(steady) < 5:
                    raise RuntimeError("Not enough steady audio measurements")
                result["steady_underrun_callbacks"] = steady[-1]["audio_underrun_callbacks"] - steady[0]["audio_underrun_callbacks"]
                result["steady_playback_progress_ms"] = steady[-1]["playback_ms"] - steady[0]["playback_ms"]
                result["continuity_scope"] = "Receiver starvation/gaps after 4s startup; not microphone or speaker capture"
        if expect_caption:
            result["caption_present"] = caption_seen
        print(json.dumps(result, indent=2))
        checks = [code == 0, result["ansi_half_blocks"] > 0, paused_was_visible,
                  help_was_visible, queue_was_visible, result["alternate_screen_restored"]]
        if steady_audio:
            checks = [code == 0, restored, result.get("steady_underrun_callbacks") == 0,
                      result.get("steady_playback_progress_ms", 0) > 4000,
                      result.get("last_application_sample", {}).get("remote_audio") is True]
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
    parser.add_argument("--repeat", choices=["off", "all", "one"], default="off")
    parser.add_argument("--metrics", type=Path)
    parser.add_argument("--audio-endpoint")
    parser.add_argument("--audio-token-file", type=Path)
    parser.add_argument("--steady-audio", action="store_true", help="Check uninterrupted audio starvation after startup, without control changes")
    args = parser.parse_args()
    if bool(args.audio_endpoint) != bool(args.audio_token_file):
        parser.error("audio endpoint and token file are required together")
    smoke(args.binary, args.media, args.duration, args.expect_caption, args.audio_endpoint, args.audio_token_file, args.metrics, args.repeat, args.steady_audio)
