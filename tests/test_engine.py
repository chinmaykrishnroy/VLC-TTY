"""Real ctypes ABI smoke test against a small compiled shim, no VLC required."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from termvlc.render import halfblock_rows, progress_bar, time_label, ellipsis


class RenderTests(unittest.TestCase):
    def test_halfblock_colors(self):
        # 2x2 BGRA: first row red, green; bottom blue, white.
        pixels = bytes([0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255])
        rows, cols, lines = halfblock_rows((1, 2, 2, 8, pixels), 2, 1)
        self.assertEqual((cols, lines), (2, 1))
        self.assertIn("38;2;255;0;0", rows[0])
        self.assertIn("48;2;0;0;255", rows[0])
        self.assertIn("▀▀", "".join(c for c in rows[0] if c == "▀"))

    def test_ascii(self):
        frame = (1, 2, 2, 8, bytes([255] * 16))
        rows, _, _ = halfblock_rows(frame, 2, 1, ascii_mode=True)
        self.assertEqual(rows, ["@@"])

    def test_no_upscale_crashes_and_helpers(self):
        self.assertEqual(time_label(3600), "1:00:00")
        self.assertEqual(time_label(-10), "00:00")
        self.assertEqual(progress_bar(5, 10, 4), "━━──")
        self.assertEqual(ellipsis("abcd", 3), "abc")


@unittest.skipUnless(sys.platform.startswith("linux") and shutil.which("gcc"), "GCC/Linux only")
class TestLibVlcFFI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.dso = Path(cls.temp.name) / "libfakevlc.so"
        src = Path(__file__).with_name("fake_vlc.c")
        subprocess.run(["gcc", "-shared", "-fPIC", "-O2", str(src), "-o", str(cls.dso)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_callbacks_play_pause_seek_and_volume(self):
        from termvlc.engine import LibVLC
        with tempfile.TemporaryDirectory() as tmp:
            video = Path(tmp) / "test.mp4"
            video.write_bytes(b"fake")
            with mock.patch.dict(os.environ, {"TERMVLC_LIBVLC_PATH": str(self.dso)}):
                with LibVLC(decode_width=128) as player:
                    player.open(str(video))
                    frame = player.latest_frame()
                    self.assertIsNotNone(frame, player.callback_error)
                    self.assertEqual((frame[1], frame[2]), (128, 72))
                    self.assertEqual(len(frame[4]), frame[3] * frame[2])
                    self.assertEqual(player.state, "playing")
                    player.toggle_pause()
                    self.assertEqual(player.state, "paused")
                    player.seek(15)
                    self.assertEqual(player.position, 15)
                    player.change_volume(10)
                    self.assertEqual(player.volume, 85)
                    player.change_rate(0.25)
                    self.assertAlmostEqual(player.rate, 1.25)
                    self.assertEqual(player.cycle_subtitles(), "Subtitles: off")
                    self.assertEqual(player.cycle_audio(), "No alternate audio tracks")
                    self.assertIsNone(player.callback_error)

    def test_cli_end_to_end_in_pseudo_terminal(self):
        if not hasattr(os, "openpty"):
            self.skipTest("PTY unsupported")
        import select
        import time
        with tempfile.TemporaryDirectory() as tmp:
            video = Path(tmp) / "test.mp4"
            video.write_bytes(b"fake")
            master, slave = os.openpty()
            env = dict(os.environ, TERMVLC_LIBVLC_PATH=str(self.dso), PYTHONPATH=str(Path(__file__).parents[1]))
            process = subprocess.Popen(
                [sys.executable, "-m", "termvlc", str(video), "--fps", "5"],
                stdin=slave, stdout=slave, stderr=slave, cwd=tmp, env=env,
                start_new_session=True,
            )
            os.close(slave)
            data = bytearray()
            try:
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    readable, _, _ = select.select([master], [], [], .15)
                    if readable:
                        try:
                            data.extend(os.read(master, 65536))
                        except OSError:
                            break
                    if b"TERMVLC" in data and b"38;2;" in data:
                        break
                os.write(master, b"q")
                deadline = time.monotonic() + 4
                while process.poll() is None and time.monotonic() < deadline:
                    readable, _, _ = select.select([master], [], [], .1)
                    if readable:
                        try:
                            data.extend(os.read(master, 65536))
                        except OSError:
                            break
                if process.poll() is None:
                    process.wait(timeout=2)
                self.assertEqual(process.returncode, 0, data[-1500:])
                self.assertIn(b"TERMVLC", data)
                self.assertIn(b"38;2;", data, "No pixel frame rendered")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                os.close(master)


if __name__ == "__main__":
    unittest.main()
