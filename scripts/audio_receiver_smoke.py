"""Exercise a real CPAL device, optionally with Pi video/PCM through SSH.

Plays a quiet 500 Hz fixture in local mode. Does not certify audibility or lip-sync.
No microphone capture and no recorded audio. Not a headless CI test.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shlex
import socket
import struct
import subprocess
import tempfile
import threading
import time


def exact(connection, length):
    result = bytearray()
    while len(result) < length:
        chunk = connection.recv(length-len(result))
        if not chunk:
            raise EOFError("Audio connection closed")
        result.extend(chunk)
    return bytes(result)


def pcm(port, key, duration):
    feedback = []
    with socket.create_connection(("127.0.0.1", port), timeout=3) as connection:
        connection.sendall(b"VTTYPCM1" + key)
        assert exact(connection, 8) == b"VTTYPCM1"

        def send(payload):
            connection.sendall(struct.pack("<I", len(payload)) + payload)

        def read():
            try:
                while True:
                    length = struct.unpack("<I", exact(connection, 4))[0]
                    if not 1 <= length <= 4096:
                        raise ValueError("Invalid feedback length")
                    data = exact(connection, length)
                    if data[0] == 5:
                        feedback.append(struct.unpack("<BQIQq", data))
            except (OSError, EOFError):
                pass

        send(struct.pack("<BIH", 1, 48000, 2))
        send(struct.pack("<BQB", 3, 1, 0))
        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        started = time.monotonic()
        for block in range(int(duration*50)):
            target = started+block*.02
            if target > time.monotonic():
                time.sleep(target-time.monotonic())
            samples = [int(math.sin(2*math.pi*500*(block*960+i)/48000)*1500)
                       for i in range(960) for _ in range(2)]
            send(struct.pack("<BQqq", 2, 1, int((block*.02+1)*1e6), 0)
                 + struct.pack("<1920h", *samples))
        send(struct.pack("<BQB", 3, 2, 1))
        time.sleep(.2)
        send(struct.pack("<BQB", 3, 3, 0))
        time.sleep(.1)
        send(bytes([4]))
        reader.join(timeout=4)
    if len(feedback) < 10 or not any(item[4] > 0 for item in feedback):
        raise RuntimeError("No device presentation feedback")
    return {"telemetry_messages": len(feedback), "maximum_queued_frames": max(x[2] for x in feedback),
            "underrun_callback_count": max(x[3] for x in feedback), "source": "generated PCM"}


def run(args):
    binary = args.binary.resolve()
    if not binary.is_file() or args.output.exists():
        raise ValueError("Binary must exist and report path must be new")
    with tempfile.TemporaryDirectory(prefix="vlc-tty-audio-smoke-") as temp:
        key = os.urandom(32)
        keyfile = Path(temp) / "session.key"
        keyfile.write_text(key.hex())
        keyfile.chmod(0o600)
        with socket.socket() as reserve:
            reserve.bind(("127.0.0.1", 0))
            port = reserve.getsockname()[1]
        receiver = subprocess.Popen([str(binary), "audio-receive", "--listen", f"127.0.0.1:{port}", "--token-file", str(keyfile)],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        remote_key = None
        try:
            ready = []
            ready_thread = threading.Thread(target=lambda: ready.append(receiver.stderr.readline()), daemon=True)
            ready_thread.start()
            ready_thread.join(timeout=15)
            if not ready or b"listening on" not in ready[0]:
                raise RuntimeError("Local audio device did not initialize: " + (ready[0].decode(errors="replace") if ready else "timeout"))
            if not args.ssh_host:
                report = pcm(port, key, args.duration)
            else:
                remote_key = f"/tmp/vlc-tty-audio-smoke.{os.urandom(8).hex()}"
                subprocess.run(["ssh", "-o", "BatchMode=yes", "--", args.ssh_host,
                                f"umask 077; set -C; cat > {shlex.quote(remote_key)}"],
                               input=key.hex().encode(), check=True, timeout=25, stdout=subprocess.DEVNULL)
                remote_port = 49152+int.from_bytes(os.urandom(2), "little") % 16383
                profile = str(Path(args.remote_driver).parent.parent.as_posix()) + f"/audio-smoke-{os.urandom(6).hex()}.jsonl"
                command = shlex.join(["python3", args.remote_driver, args.remote_binary, args.remote_media,
                                      "--duration", str(args.duration), "--repeat", "all", "--audio-endpoint", f"127.0.0.1:{remote_port}",
                                      "--audio-token-file", remote_key, "--metrics", profile])
                result = subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes", "-R",
                                         f"127.0.0.1:{remote_port}:127.0.0.1:{port}", "--", args.ssh_host, command],
                                        capture_output=True, text=True, encoding="utf-8", timeout=args.duration+45)
                if result.returncode:
                    raise RuntimeError("Remote PTY test failed: " + result.stdout[-2000:] + result.stderr[-2000:])
                report = {"source": "remote LibVLC over SSH reverse tunnel", "remote_pty": json.loads(result.stdout), "remote_profile": profile}
                sample = report["remote_pty"].get("last_application_sample") or {}
                if not sample.get("remote_audio") or sample.get("audio_estimated_receiver_pts", 0) <= 0:
                    raise RuntimeError("Remote decoder did not receive device feedback")
            _, errors = receiver.communicate(timeout=8)
            if receiver.returncode:
                raise RuntimeError("Receiver failed: " + errors.decode(errors="replace"))
            report.update(passed=True, device="default CPAL output opened", duration_s=args.duration,
                          audible_confirmation=False, av_sync_measured=False)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
            print(json.dumps(report, indent=2))
            return report
        finally:
            if receiver.poll() is None:
                receiver.kill()
                receiver.wait(timeout=5)
            if remote_key:
                subprocess.run(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=3", "--", args.ssh_host,
                                "rm -f -- " + shlex.quote(remote_key)], stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--duration", type=float, default=5)
    parser.add_argument("--ssh-host")
    parser.add_argument("--remote-binary")
    parser.add_argument("--remote-media")
    parser.add_argument("--remote-driver", help="absolute server path to scripts/pty_smoke.py")
    args = parser.parse_args()
    if not 3 <= args.duration <= 3600:
        parser.error("duration must be 3-3600 seconds")
    if args.ssh_host and not all([args.remote_binary, args.remote_media, args.remote_driver]):
        parser.error("SSH smoke needs all three remote paths")
    run(args)
