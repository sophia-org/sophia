import unittest

from owner_wait import FIELDS, attribute


def records():
    rows = []
    for stamp, count in ((1_000_000, 0), (6_000_000, 1)):
        clock = {"owner_tid": 12, "owner_waits": count * 9,
                 "owner_wait_deadlines": count * 5, "completions": count * 2}
        waits = dict.fromkeys(FIELDS, 0)
        waits.update(owner_tid=12, selected_maintenance=3 * count,
                     selected_frames=6 * count, expired_maintenance=2 * count,
                     expired_frames=3 * count, pending_frames=8 * count,
                     pending_lifecycle=8 * count)
        for prefix, fields in (("sophia_present_clock_service", clock),
                               ("sophia_owner_wait", waits)):
            rows.append(prefix + f" schema=1 observed_monotonic_usec={stamp} " +
                        " ".join(f"{k}={v}" for k, v in sorted(fields.items())))
    return "\n".join(rows)


class OwnerWaitTests(unittest.TestCase):
    def test_partition_and_overlapping_pending_use_interval_completions(self):
        result = attribute(records(), 0, 10_000_000)
        self.assertEqual(result["completions"], 2)
        self.assertEqual(result["per_complete"]["expired"]["frames"], 1.5)
        self.assertEqual(result["per_complete"]["pending"]["lifecycle"], 4)

    def test_inconsistent_evidence_is_refused(self):
        changes = (("selected_frames=6", "selected_frames=7"),
                   ("expired_frames=3", "expired_frames=4"),
                   ("pending_frames=8", "pending_frames=10"),
                   ("selected_maintenance=3", "selected_maintenance=1"),
                   ("completions=2", "completions=0"),
                   ("owner_tid=12", "owner_tid=13"),
                   ("observed_monotonic_usec=6000000", "observed_monotonic_usec=6000001"),
                   ("selected_frames=6", "missing_frames=6"))
        for old, new in changes:
            with self.subTest(old=old), self.assertRaises(ValueError):
                attribute(records().replace(old, new, 1), 0, 10_000_000)

    def test_expiries_cannot_exceed_the_selected_reason(self):
        text = records().replace("expired_frames=3", "expired_frames=1").replace(
            "expired_maintenance=2", "expired_maintenance=4")
        with self.assertRaisesRegex(ValueError, "cohort"):
            attribute(text, 0, 10_000_000)

    def test_missing_records_are_refused(self):
        with self.assertRaisesRegex(ValueError, "two records"):
            attribute("", 0, 10_000_000)
