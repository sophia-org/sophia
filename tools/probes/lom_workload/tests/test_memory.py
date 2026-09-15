"""Strict workload inventory transcript controls; no live sampling."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from records import HOST, InvalidEvidence, decode
from verify import verify
from fixture import transcript, encode, limits


class MemoryEvidenceTests(unittest.TestCase):
    def test_positive_has_numeric_bounds_and_all_samples(self):
        result = verify(*transcript(), limits())["memory"]
        self.assertEqual(result["slot_bound"], 4)
        self.assertEqual(result["pixel_byte_bound"], 38400)
        self.assertEqual(result["warmed_resource_ids"], 4)
        self.assertEqual(result["samples"], 17)
        self.assertEqual(result["high_water"]["resident_bytes"], 19200)

    def test_missing_brackets_gaps_and_duplicate_samples_fail(self):
        for mode in ("empty", "start", "end", "gap", "duplicate"):
            with self.subTest(mode=mode):
                host, client = transcript()
                rows = host["sophia_shell_content_sample"]
                if mode == "empty":
                    rows.clear()
                elif mode == "start":
                    del rows[:2]
                elif mode == "end":
                    del rows[14:]
                elif mode == "gap":
                    del rows[5]
                else:
                    rows.insert(3, rows[3].copy())
                with self.assertRaises(InvalidEvidence):
                    verify(host, client, limits())

    def test_identity_and_every_owned_bound_fail(self):
        for field, value in (("connection_epoch", 2), ("content_grant_epoch", 3),
                             ("active_epochs", 0), ("retired_epochs", 1),
                             ("resources", 5), ("resource_ids", 5), ("transfers", 5),
                             ("resident_bytes", 38401), ("backing_bytes", 38401),
                             ("reserved_resident_bytes", 16777216),
                             ("reserved_bytes", 41943041), ("reserved_backing_bytes", 33554433),
                             ("allocations", 3), ("candidates", 5), ("permits", 3), ("demands", 3),
                             ("response_records", 129), ("response_bytes", 262145),
                             ("input_bytes", 131073), ("input_records", 5462)):
            with self.subTest(field=field):
                host, client = transcript()
                host["sophia_shell_content_sample"][6][field] = value
                with self.assertRaises(InvalidEvidence):
                    verify(host, client, limits())

    def test_warmed_ids_cannot_grow_even_below_the_slot_cap(self):
        host, client = transcript()
        for row in host["sophia_shell_content_sample"][:2]:
            row["resource_ids"] = 3
        with self.assertRaisesRegex(InvalidEvidence, "grew after warmup"):
            verify(host, client, limits())

    def test_actual_negotiated_budgets_and_strict_decode(self):
        host, client = transcript()
        name = "sophia_shell_content_budget"
        cap = host[name][0]
        host[name].append({**cap, "limits_generation": 2})
        with self.assertRaisesRegex(InvalidEvidence, "changed content budgets"):
            verify(host, client, limits())
        host[name].pop()
        cap["max_live_resources"] = 1
        with self.assertRaisesRegex(InvalidEvidence, "slot or ID bound"):
            verify(host, client, limits())
        for name in ("sophia_shell_content_sample", "sophia_shell_content_budget"):
            row = host[name][0]
            good = encode({name: [row]}).strip()
            self.assertEqual(decode(good, HOST), (name, row))
            for field in row:
                with self.subTest(name=name, field=field):
                    missing = encode({name: [{k: v for k, v in row.items() if k != field}]}).strip()
                    for invalid in (missing, good + f" {field}={row[field]}"):
                        with self.assertRaises(InvalidEvidence):
                            decode(invalid, HOST)
