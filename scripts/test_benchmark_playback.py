import unittest
from benchmark_playback import rates, sustained_rate_acceptance


class BenchmarkTests(unittest.TestCase):
    def test_sustained_gate_rejects_a_display_cliff_even_when_encoding_is_stable(self):
        before = dict(samples=30, counter="encoded_frames", median_encoded_frames_per_second=15,
                      median_unique_frames_drawn_per_second=14)
        after = dict(before, median_unique_frames_drawn_per_second=7)
        comparison = dict(before_5000=before, after_5000=after)
        self.assertFalse(sustained_rate_acceptance(comparison, 0.25)["passed"])
        after["median_unique_frames_drawn_per_second"] = 13
        self.assertTrue(sustained_rate_acceptance(comparison, 0.25)["passed"])
        after["samples"] = 19
        self.assertFalse(sustained_rate_acceptance(comparison, 0.25)["passed"])
        self.assertTrue(sustained_rate_acceptance(comparison, None)["passed"])

    def test_rates_use_elapsed_time_not_the_requested_fps(self):
        first = {"elapsed_ms": 1000, "vlc_callback_frames": 30, "unique_frames_drawn": 10, "encoded_frames": 12,
                 "copied_bytes": 1000, "picture_allocations": 30, "backend_bytes": 500}
        second = dict(first, elapsed_ms=3000, vlc_callback_frames=90, unique_frames_drawn=20, encoded_frames=24,
                      copied_bytes=3000, picture_allocations=90, backend_bytes=1500)
        actual = rates(first, second)
        self.assertEqual(actual["unique_frames_drawn_per_second"], 5)
        self.assertEqual(actual["vlc_callback_frames_per_second"], 30)
        self.assertEqual(actual["backend_bytes_per_second"], 500)
        self.assertNotIn("client_presented_fps", actual)
        self.assertEqual(rates(first, first), {})
