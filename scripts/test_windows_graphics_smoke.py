"""Regressions for interpreting incremental console output in native smokes."""
import unittest

from windows_graphics_smoke import assert_synchronized_clears, visible_text


class ConsoleProjectionTests(unittest.TestCase):
    def test_menu_label_survives_incremental_writes_and_ignores_images(self):
        data = b"\x1b[2J\x1b[2;3HPlay / Pause\x1b[2;3HCycle Audio Track"
        data += b"\x1bPqTHIS IS IMAGE DATA\x1b\\\x1b]0;window title\x07"
        data += b"\x1b[3;3HVolume Down\x1b[3;10HUp\x1b[K"
        text = visible_text(data)
        self.assertIn("Cycle Audio Track", text)
        self.assertIn("Volume Up", text)
        self.assertNotIn("Play / Pause", text)
        self.assertNotIn("IMAGE DATA", text)
        self.assertNotIn("window title", text)

    def test_erases_must_be_inside_synchronized_updates(self):
        assert_synchronized_clears(b"\x1b[?2026h\x1b[2Jtext\x1b[?2026l")
        with self.assertRaises(AssertionError):
            assert_synchronized_clears(b"\x1b[2J\x1b[?2026htext\x1b[?2026l")
