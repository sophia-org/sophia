import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("render_work", Path(__file__).resolve().parents[1] / "analyze_render_work.py")
REPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REPORT)


def record(time, full, partial, pixels, target, details=False):
    fields = dict(zip(REPORT.TOTALS, (full, partial, pixels, target)))
    if details:
        fields.update(dict.fromkeys(REPORT.DETAILS, 0))
        fields.update(damage_full_plan_count=full,
                      damage_stable_geometry_frames_count=partial,
                      damage_stable_geometry_partial_count=partial,
                      damage_stable_geometry_repaint_pixels_count=partial * 2,
                      damage_stable_geometry_target_pixels_count=partial * 100)
    return REPORT.PREFIX + f"schema=1 uptime_msec={time} " + " ".join(f"{k}={v}" for k, v in fields.items())


class RenderWorkTests(unittest.TestCase):
    def test_interval_is_not_cumulative_full_fraction(self):
        result = REPORT.analyze([record(1000, 60000, 700, 100000, 1000000, True),
                                 record(2000, 60001, 709, 100118, 1001000, True)])
        window = result["intervals"][0]
        self.assertEqual(window["full_fraction"], 0.1)
        self.assertEqual(window["repaint_area_ratio"], 0.118)
        self.assertEqual(window["stable_geometry"]["frames"], 9)
        self.assertEqual(window["stable_geometry"]["full_fraction"], 0)
        self.assertEqual(window["stable_geometry"]["repaint_area_ratio"], 0.02)

    def test_counter_reset_omits_crossing_interval_and_rebases(self):
        result = REPORT.analyze([record(1, 100, 10, 100, 1000), record(2, 1, 1, 10, 100), record(3, 2, 2, 20, 200)])
        self.assertEqual(len(result["resets"]), 1)
        self.assertEqual(len(result["intervals"]), 1)
        self.assertEqual(result["intervals"][0]["frames"], 2)

    def test_no_work_has_no_percentage_and_new_fields_rebase(self):
        result = REPORT.analyze([record(1, 0, 0, 0, 0), record(2, 0, 0, 0, 0), record(3, 0, 0, 0, 0, True)])
        self.assertIsNone(result["intervals"][0]["full_fraction"])
        self.assertIsNone(result["intervals"][0]["repaint_area_ratio"])
        self.assertEqual(result["resets"][0]["reason"], "counter_set_changed")

    def test_missing_duplicate_non_numeric_or_nonconserving_is_refused(self):
        good = record(1, 1, 1, 10, 100, True)
        for bad in [REPORT.PREFIX + "uptime_msec=1", good + " uptime_msec=1", good.replace("uptime_msec=1", "uptime_msec=-1")]:
            with self.assertRaises(ValueError):
                REPORT.analyze([bad])
        bad = record(2, 2, 2, 20, 200, True).replace("damage_full_plan_count=2", "damage_full_plan_count=3")
        with self.assertRaises(ValueError):
            REPORT.analyze([good, bad])


if __name__ == "__main__":
    unittest.main()
