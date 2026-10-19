"""Measure real Windows ConPTY playback with bounded output capture.

Counts backend submissions, not visible FPS or physical A/V synchronization.
The child is silent unless --audio-clock enables muted local audio decoding.
Supply a private local file; no media is copied/uploaded.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import sys
import time

from windows_graphics_smoke import Console, prepare_host


class BoundedConsole(Console):
    def read(self):
        buffer = C.create_string_buffer(65536)
        size = W.DWORD()
        self.total_bytes = 0
        while self.k.ReadFile(self.output, buffer, len(buffer), C.byref(size), None) and size.value:
            with self._data_lock:
                self.total_bytes += size.value
                self._data.extend(buffer.raw[:size.value])
                if len(self._data) > 2 * 1024 * 1024:
                    del self._data[:-1024 * 1024]


def latest_sample(path):
    try:
        return json.loads(path.read_text().splitlines()[-1])
    except (OSError, ValueError, IndexError):
        return None


def run(args):
    args.output.mkdir(parents=True, exist_ok=False)
    metrics = (args.output / "metrics.jsonl").resolve()
    for name in ["NO_COLOR", "TERM", "TERM_PROGRAM", "KITTY_WINDOW_ID"]:
        os.environ.pop(name, None)
    if args.runtime_home:
        os.environ["VLC_HOME"] = str(args.runtime_home.resolve())
    initial = ["stream", *args.initial_stream] if args.initial_stream else []
    player_options = ["--metrics", str(metrics)]
    if args.decode_height is not None:
        player_options += ["--decode-height", str(args.decode_height)]
    bootstrap = Path(__file__).with_name("windows_graphics_smoke.py")
    child_mode = "--console-child-audio" if args.audio_clock else "--console-child"
    command = [sys.executable, bootstrap, child_mode, args.binary.resolve(), *initial, *player_options]
    started = time.monotonic()
    terminal = BoundedConsole(command, prepare_host(args.host_directory), "sixel", args.columns, args.rows)
    result = {"scope": "native console backend; not visible FPS or audible A/V sync", "audio_clock": args.audio_clock}
    try:
        terminal.wait_for(lambda data: b"Sixel pixel graphics" in data, args.startup_timeout)
        if initial:
            # Wait for LibVLC's playback clock, rather than assuming that an
            # artwork filename/tag exists in this particular remote file.
            terminal.wait_for(lambda _: (latest_sample(metrics) or {}).get("playback_ms", 0) >= 1000,
                              args.startup_timeout)
        result["ui_startup_s"] = time.monotonic() - started
        initial_frames = (latest_sample(metrics) or {}).get("vlc_callback_frames", 0)
        dropped = time.monotonic()
        terminal.send(('"' + str(args.media.resolve()) + '"').encode())
        samples = []
        probe = None
        response = None
        first = None
        while time.monotonic() - dropped < args.duration:
            time.sleep(.05)
            sample = latest_sample(metrics)
            if sample and (not samples or samples[-1]["elapsed_ms"] != sample["elapsed_ms"]):
                samples.append(sample)
            if sample and first is None and sample["vlc_callback_frames"] > initial_frames:
                first = time.monotonic()
                result["first_video_s"] = first - dropped
            if probe is None and first is not None and time.monotonic() - first >= 10:
                probe = time.monotonic()
                terminal.send(b"?")
            if probe is not None and response is None and b"KEYBOARD CONTROLS" in terminal.data:
                response = time.monotonic() - probe
                terminal.send(b"\x1b")
        terminal.send(b"q")
        result["quit_responsive"] = terminal.k.WaitForSingleObject(terminal.process, 10000) == 0
        code = W.DWORD()
        terminal.k.GetExitCodeProcess(terminal.process, C.byref(code))
        result.update(exit_code=code.value, terminal_restored=b"\x1b[?1049l" in terminal.data,
                      help_latency_s=response, captured_bytes=terminal.total_bytes)
        if len(samples) >= 8:
            # Exclude the opening interval from steady playback throughput.
            moving = [row for row in samples if row["vlc_callback_frames"] > initial_frames]
            if len(moving) >= 6:
                a, b = moving[2], moving[-1]
                seconds = (b["elapsed_ms"] - a["elapsed_ms"]) / 1000
                result.update(measured_playback_s=seconds,
                              submitted_fps=(b["unique_frames_drawn"] - a["unique_frames_drawn"]) / seconds,
                              decode_fps=(b["vlc_callback_frames"] - a["vlc_callback_frames"]) / seconds,
                              encode_p95_ms=b["encode_recent_p95_us"] / 1000,
                              draw_p95_ms=b["draw_recent_p95_us"] / 1000,
                              encode_failures=b["encode_failures"],
                              request_to_backend_p95_ms=b.get("request_to_backend_recent_p95_us", 0) / 1000)
        result["passed"] = (result.get("submitted_fps", 0) >= args.minimum_fps
                            and response is not None and response <= args.maximum_input_latency
                            and result["quit_responsive"] and code.value == 0
                            and result["terminal_restored"] and result.get("encode_failures", 1) == 0)
        (args.output / "summary.json").write_text(json.dumps(result, indent=2))
        print(json.dumps(result), flush=True)
        return 0 if result["passed"] else 1
    finally:
        terminal.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("media", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="New result directory")
    parser.add_argument("--initial-stream", nargs=2, metavar=("HOST", "REMOTE_FILE"))
    parser.add_argument("--runtime-home", type=Path)
    parser.add_argument("--host-directory", type=Path, default=Path("target/terminal-test-host"))
    parser.add_argument("--columns", type=int, default=188)
    parser.add_argument("--rows", type=int, default=50)
    parser.add_argument("--duration", type=float, default=45)
    parser.add_argument("--startup-timeout", type=float, default=120)
    parser.add_argument("--decode-height", type=int)
    parser.add_argument("--audio-clock", action="store_true", help="Enable local audio decoding at volume zero")
    parser.add_argument("--minimum-fps", type=float, default=12)
    parser.add_argument("--maximum-input-latency", type=float, default=.5)
    options = parser.parse_args()
    if not options.media.is_file() or options.duration < 15 or not (24 <= options.columns <= 500 and 8 <= options.rows <= 200):
        parser.error("Use an existing media file, duration >=15 seconds, and a terminal size within 24x8–500x200")
    raise SystemExit(run(options))
