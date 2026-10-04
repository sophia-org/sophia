import unittest

from settings import comparison_settings, verify_settings


class SettingsTests(unittest.TestCase):
    def setUp(self):
        self.settings = {"present_evidence": "full", "renderer_worker": "private"}
        self.text = ("sophia_present_evidence schema=1 mode=full\n"
                     "sophia_live_native_resources schema=12 status=complete renderer_workers=2 "
                     "worker_failures=0 worker_hard_stalls=0 worker_result_misroutes=0 "
                     "worker_release_enqueue_failures=0 frame_slots_leased=0\n")

    def test_one_explicit_setting_and_unchanged_control(self):
        comparison_settings(self.settings, self.settings)
        for key, value in (("present_evidence", "aggregate"), ("renderer_worker", "shared")):
            changed = {**self.settings, key: value}
            comparison_settings(self.settings, changed, key)
            with self.assertRaises(ValueError):
                comparison_settings(self.settings, changed)
        with self.assertRaises(ValueError):
            comparison_settings(self.settings, {"present_evidence": "aggregate",
                                               "renderer_worker": "shared"}, "renderer_worker")
        with self.assertRaises(ValueError):
            comparison_settings(self.settings, self.settings, "present_evidence")

    def test_actual_session_mode_and_worker_count(self):
        verify_settings(self.text, self.settings)
        verify_settings(self.text.replace("mode=full", "mode=aggregate").replace(
            "renderer_workers=2", "renderer_workers=1"),
            {"present_evidence": "aggregate", "renderer_worker": "shared"})

    def test_missing_unknown_or_contradictory_settings_fail(self):
        for text in ("", self.text + self.text,
                     self.text.replace("mode=full", "mode=aggregate"),
                     self.text.replace("renderer_workers=2", "renderer_workers=1"),
                     self.text.replace("worker_failures=0", "worker_failures=1"),
                     self.text.replace("frame_slots_leased=0", "frame_slots_leased=1")):
            with self.subTest(text=text), self.assertRaises(ValueError):
                verify_settings(text, self.settings)
        for settings in ({}, {**self.settings, "renderer_worker": "other"}):
            with self.assertRaises(ValueError):
                verify_settings(self.text, settings)
