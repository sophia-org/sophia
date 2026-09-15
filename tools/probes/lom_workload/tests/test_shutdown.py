"""Synthetic final-accounting refusals; no backend or device execution."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from records import HOST, SHUTDOWN_INVENTORY, InvalidEvidence, decode
from verify import verify
from fixture import transcript, encode, limits


class ShutdownEvidenceTests(unittest.TestCase):
    def test_missing_and_repeated_shutdown_are_not_success(self):
        for count in (0, 2):
            host, client = transcript()
            host["sophia_shell_content_shutdown"] *= count
            with self.assertRaisesRegex(InvalidEvidence, "final shell shutdown"):
                verify(host, client, limits())

    def test_each_owned_inventory_is_checked_despite_quiescent_label(self):
        for field in SHUTDOWN_INVENTORY.split():
            with self.subTest(field=field):
                host, client = transcript()
                host["sophia_shell_content_shutdown"][0][field] = 1
                with self.assertRaisesRegex(InvalidEvidence, "ownership or credit"):
                    verify(host, client, limits())

    def test_exact_grant_join_and_order_are_required(self):
        for field, value in (("connection_epoch", 3), ("content_grant_epoch", 4),
                             ("workers_joined", "0"), ("status", "retained"),
                             ("monotonic_usec", 71_500_000)):
            with self.subTest(field=field):
                host, client = transcript()
                host["sophia_shell_content_shutdown"][0][field] = value
                # The early snapshot is AFTER the declared 71s window but
                # BEFORE the actual final 72s native completions.
                with self.assertRaises(InvalidEvidence):
                    verify(host, client, limits())

    def test_shutdown_decoding_never_defaults_missing_or_malformed_fields(self):
        host, _ = transcript()
        name = "sophia_shell_content_shutdown"
        row = host[name][0]
        good = encode({name: [row]}).strip()
        self.assertEqual(decode(good, HOST), (name, row))
        for field in row:
            with self.subTest(field=field):
                missing = encode({name: [{k: v for k, v in row.items() if k != field}]}).strip()
                for invalid in (missing, good + f" {field}={row[field]}"):
                    with self.assertRaises(InvalidEvidence):
                        decode(invalid, HOST)
        for value in (-1, 1 << 64, "1x"):
            with self.assertRaises(InvalidEvidence):
                decode(encode({name: [{**row, "resident_bytes": value}]}).strip(), HOST)
