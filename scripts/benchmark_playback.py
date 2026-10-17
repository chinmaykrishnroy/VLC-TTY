"""Measure server rendering in a Unix PTY sink; never claim client-visible FPS."""
import argparse
import json
import os
from pathlib import Path
import platform
import select
import signal
import struct
import statistics
import subprocess
import time


def resources(pid):
    """Linux process-tree samples. RSS is summed, not unique physical memory."""
    rss, ticks = 0, 0
    pending = [pid]
    while pending:
        current = pending.pop()
        try:
            pending.extend(map(int, Path(f"/proc/{current}/task/{current}/children").read_text().split()))
            status = Path(f"/proc/{current}/status").read_text()
            rss += int(next(line.split()[1] for line in status.splitlines() if line.startswith("VmRSS:")))
            fields = Path(f"/proc/{current}/stat").read_text().rsplit(")", 1)[1].split()
            ticks += int(fields[11]) + int(fields[12])
        except (FileNotFoundError, ProcessLookupError, StopIteration):
            pass
    return (rss, ticks) if rss else (None, None)


def rates(previous, current):
    seconds = (current["elapsed_ms"] - previous["elapsed_ms"]) / 1000
    if seconds <= 0:
        return {}
    return {name + "_per_second": (current[name] - previous[name]) / seconds
            for name in ("vlc_callback_frames", "unique_frames_drawn", "encoded_frames", "copied_bytes", "picture_allocations", "backend_bytes")}


def phases(path, renderer):
    counter = "unique_frames_drawn" if renderer == "ansi" else "encoded_frames"
    groups = {"before_5000": [], "after_5000": []}
    with path.open() as stream:
        for line in stream:
            sample = json.loads(line)
            if sample[counter] >= 1000:
                groups["after_5000" if sample[counter] >= 5000 else "before_5000"].append(sample)
    result = {}
    for name, samples in groups.items():
        fields = ["unique_frames_drawn_per_second", "encoded_frames_per_second", "encode_recent_p95_us", "draw_recent_p95_us", "backend_bytes_per_second", "summed_rss_kib"]
        result[name] = {"samples": len(samples), "counter": counter}
        for field in fields:
            values = [sample[field] for sample in samples if sample.get(field) is not None]
            result[name]["median_" + field] = statistics.median(values) if values else None
    return result


def benchmark(args):
    import fcntl
    import pty
    import termios
    binary, media = args.binary.resolve(), args.media.resolve()
    if not binary.is_file() or not media.is_file():
        raise ValueError("Binary and local media must exist")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    app_metrics = output / "application.jsonl"
    metadata = {"schema": 1, "renderer": args.renderer, "duration_requested_s": args.duration,
                "fps_limit": args.fps, "decode_height": args.decode_height, "terminal_cells": [args.columns, args.rows],
                "os": platform.platform(), "architecture": platform.machine(), "client_presented_fps": None,
                "scope": "server pipeline and PTY drain; no real graphics client, SSH wire throughput or end-user FPS measurement",
                "resource_scope": "Linux launcher and descendants, sampled once per second; summed RSS can double-count shared pages"}
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", args.rows, args.columns, 0, 0))

    def setup():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    env = {key: value for key, value in os.environ.items() if key not in ("DISPLAY", "WAYLAND_DISPLAY", "TMUX")}
    env["TERM"] = "xterm-256color"
    process = subprocess.Popen([str(binary), "--ssh", "--silent", "--graphics", args.renderer, "--fps", str(args.fps),
                                "--decode-height", str(args.decode_height), "--repeat", "all", "--metrics", str(app_metrics), str(media)],
                               stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=setup)
    os.close(slave)
    started = time.monotonic()
    next_sample, next_probe = started + 1, started + 30
    pending_probe = None
    probe_tail = bytearray()
    tail = bytearray()
    total_bytes = peak_rss = maximum_ticks = 0
    previous = latest = None
    metrics_stream = None
    partial = ""
    samples = probe_count = 0
    probe_latency_max_ms = 0
    terminal_restored = False
    last_decode_progress = last_draw_progress = started

    def drain(wait=0.05):
        nonlocal total_bytes, pending_probe, probe_count, probe_latency_max_ms, terminal_restored, next_probe
        if not select.select([master], [], [], wait)[0]:
            return
        try:
            data = os.read(master, 65536)
        except OSError:
            return
        total_bytes += len(data)
        tail.extend(data)
        del tail[:-65536]
        terminal_restored = terminal_restored or b"\x1b[?1049l" in tail
        if pending_probe is not None:
            probe_tail.extend(data)
            del probe_tail[:-65536]
            if b"PLAYBACK CONTROLS" in probe_tail:
                latency = (time.monotonic() - pending_probe) * 1000
                probe_latency_max_ms = max(probe_latency_max_ms, latency)
                probe_count += 1
                pending_probe = None
                next_probe = time.monotonic() + 30
                os.write(master, b"\x1b")

    try:
        with (output / "samples.jsonl").open("w") as report:
            while time.monotonic() - started < args.duration:
                drain()
                now = time.monotonic()
                if process.poll() is not None:
                    raise RuntimeError(f"Player exited early with code {process.returncode}")
                if now >= next_probe and pending_probe is None and latest is not None:
                    probe_tail.clear()
                    pending_probe = now
                    os.write(master, b"?")
                if pending_probe is not None and now - pending_probe > 5:
                    raise RuntimeError("No help response at PTY sink within five seconds")
                if now < next_sample:
                    continue
                next_sample = now + 1
                if metrics_stream is None and app_metrics.exists():
                    metrics_stream = app_metrics.open()
                if metrics_stream is not None:
                    partial += metrics_stream.read()
                    lines = partial.split("\n")
                    partial = lines.pop()
                    for line in lines:
                        if not line:
                            continue
                        latest = json.loads(line)
                        if latest["state"] == 7:
                            raise RuntimeError("LibVLC entered its error state")
                        if previous is None or latest["vlc_callback_frames"] > previous["vlc_callback_frames"]:
                            last_decode_progress = now
                        if previous is None or latest["unique_frames_drawn"] > previous["unique_frames_drawn"]:
                            last_draw_progress = now
                        if latest["state"] == 3 and (now - last_decode_progress > 10 or now - last_draw_progress > 10):
                            raise RuntimeError("No decoding or server rendering progress for ten seconds")
                        rss, ticks = resources(process.pid)
                        peak_rss = max(peak_rss, rss or 0)
                        maximum_ticks = max(maximum_ticks, ticks or 0)
                        sample = dict(latest, wall_elapsed_s=round(now - started, 3), pty_bytes=total_bytes,
                                      summed_rss_kib=rss, cpu_ticks=ticks, help_probes=probe_count,
                                      help_response_max_ms=round(probe_latency_max_ms, 3))
                        if previous is not None:
                            sample.update(rates(previous, latest))
                        report.write(json.dumps(sample) + "\n")
                        report.flush()
                        previous = latest
                        samples += 1
                if now - started > 30 and latest is None:
                    raise RuntimeError("No profiling samples within thirty seconds")
            if pending_probe is not None:
                os.write(master, b"\x1b")
            os.write(master, b"q")
            deadline = time.monotonic() + 15
            while process.poll() is None and time.monotonic() < deadline:
                drain()
            drain(0)
            code = process.wait(timeout=2)
            elapsed = time.monotonic() - started
            valid = (code == 0 and terminal_restored and latest is not None and latest["vlc_callback_frames"] >= 3
                     and latest["unique_frames_drawn"] >= args.minimum_frames and latest["encode_failures"] == 0)
            if args.renderer != "ansi":
                valid = valid and latest is not None and latest["encoded_frames"] >= args.minimum_frames
            summary = dict(metadata, passed=bool(valid), exit_code=code, wall_elapsed_s=round(elapsed, 3),
                           samples=samples, pty_bytes=total_bytes, terminal_restored=terminal_restored,
                           help_probes=probe_count, help_response_max_ms=round(probe_latency_max_ms, 3),
                           peak_summed_rss_mib=peak_rss / 1024 if peak_rss else None,
                           average_cpu_one_core_percent=maximum_ticks / os.sysconf("SC_CLK_TCK") / elapsed * 100 if maximum_ticks else None,
                           last_application_sample=latest)
            summary["frame_threshold_comparison"] = phases(output / "samples.jsonl", args.renderer)
            (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
            print(json.dumps(summary, indent=2), flush=True)
            if not valid:
                raise RuntimeError("Benchmark acceptance failed; inspect summary.json")
            return summary
    except BaseException as error:
        if not (output / "summary.json").exists():
            (output / "summary.json").write_text(json.dumps(dict(metadata, passed=False, error=str(error), last_application_sample=latest), indent=2) + "\n")
        raise
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            deadline = time.monotonic() + 5
            while process.poll() is None and time.monotonic() < deadline:
                drain()
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
        (output / "diagnostic-tail.txt").write_text(bytes(tail).decode("utf-8", errors="replace"))
        if metrics_stream is not None:
            metrics_stream.close()
        os.close(master)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("media", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="new results directory; never overwritten")
    parser.add_argument("--renderer", choices=["ansi", "kitty", "sixel", "iterm2"], default="ansi")
    parser.add_argument("--duration", type=float, default=1800)
    parser.add_argument("--minimum-frames", type=int, default=10)
    parser.add_argument("--fps", type=int, default=15)
    parser.add_argument("--decode-height", type=int, default=720)
    parser.add_argument("--columns", type=int, default=100)
    parser.add_argument("--rows", type=int, default=32)
    args = parser.parse_args()
    if os.name == "nt": parser.error("Use a Unix host/SSH server for the PTY benchmark; --metrics itself supports Windows")
    if not 2 <= args.duration <= 7200: parser.error("duration must be between 2 and 7200 seconds")
    if not 20 <= args.columns <= 500 or not 10 <= args.rows <= 200: parser.error("terminal dimensions are out of range")
    if not 1 <= args.fps <= 120 or not 0 <= args.decode_height <= 4320 or args.minimum_frames < 1: parser.error("invalid frame/decode limits")
    benchmark(args)
