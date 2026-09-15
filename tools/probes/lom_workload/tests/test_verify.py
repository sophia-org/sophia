import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from records import HOST, InvalidEvidence, decode
from verify import verify
from fixture import transcript, encode, limits


class CausalLatencyTests(unittest.TestCase):
    def test_complete_transcript_retains_every_sample_and_exact_chain(self):
        result = verify(*transcript(), limits())
        self.assertEqual(result["status"], "pass")
        self.assertEqual(result["scope"], "causal_action_latency")
        for output in result["outputs"].values():
            self.assertEqual(output["count"], 20)
            self.assertEqual(output["ack_p95_usec"], 10_000)
            self.assertEqual(output["native_p95_usec"], 50_000)
            self.assertEqual(len(output["samples"]), 20)

    def test_missing_rejected_or_ambiguous_outcome_never_disappears_from_statistics(self):
        for name in ["sophia_shell_action_receipt", "sophia_shell_action_cause",
                     "sophia_shell_action_policy", "sophia_shell_native_binding",
                     "sophia_shell_native_completion", "sophia_shell_indicator_state"]:
            for mode in ["missing", "conflict"]:
                with self.subTest(name=name, mode=mode):
                    host, client = transcript()
                    if mode == "missing":
                        host[name].clear()
                    else:
                        row = copy.deepcopy(host[name][0])
                        field = next(k for k in row if k in {
                            "monotonic_usec", "action", "state_bits", "target_generation"})
                        row[field] += 1
                        host[name].append(row)
                    with self.assertRaises(InvalidEvidence):
                        verify(host, client, limits())

    def test_exact_identity_and_causal_negatives(self):
        changes = [
            ("sophia_shell_action_receipt", 1, "target_id", 999),
            ("sophia_shell_action_receipt", 1, "presentation_epoch", 999),
            ("sophia_shell_action_receipt", 1, "disposition", "2"),
            ("sophia_shell_action_receipt", 1, "monotonic_usec", 1),
            ("sophia_shell_action_cause", 0, "admission", "RejectedCapacity"),
            ("sophia_shell_action_cause", 0, "activation_serial", 999),
            ("sophia_shell_action_cause", 0, "action", 999),
            ("sophia_shell_action_policy", 0, "outcome", "RejectedStale"),
            ("sophia_shell_action_policy", 0, "indicator_generation", 999),
            ("sophia_shell_native_binding", 0, "native_frame", 999),
            ("sophia_shell_native_binding", 0, "content_grant_epoch", 999),
            ("sophia_shell_native_binding", 0, "heads", 2),
            ("sophia_shell_native_binding", 2, "target_generation", 999),
            ("sophia_shell_native_completion", 2, "timestamp_source", "observation_fallback"),
            ("sophia_shell_native_completion", 2, "missing_kernel_timestamp", "1"),
            ("sophia_shell_indicator_state", 1, "state_bits", 1),  # Requested target already active.
        ]
        for name, at, field, value in changes:
            with self.subTest(name=name, field=field):
                host, client = transcript()
                host[name][at][field] = value
                with self.assertRaises(InvalidEvidence):
                    verify(host, client, limits())

    def test_later_unrelated_candidate_cannot_replace_missing_origin_revision(self):
        host, client = transcript()
        for candidate in client["lom_panel_candidate"]:
            if candidate["indicator_generation"] == 2:
                candidate["indicator_generation"] = 1
        with self.assertRaisesRegex(InvalidEvidence, "originating committed"):
            verify(host, client, limits())

    def test_unconverged_unrelated_candidate_is_reported_without_becoming_a_sample(self):
        host, client = transcript()
        candidate = {**client["lom_panel_candidate"][-2], "candidate_generation": 90,
                     "presentation_epoch": 190}
        binding = {**host["sophia_shell_native_binding"][-2], "candidate_generation": 90,
                   "native_frame": 90}
        client["lom_panel_candidate"].append(candidate)
        host["sophia_shell_native_binding"].append(binding)
        result = verify(host, client, limits())
        self.assertEqual(result["unqualified_candidates"], 1)
        self.assertEqual(sum(v["count"] for v in result["outputs"].values()), 40)

    def test_fixed_window_warmup_and_latency_limits(self):
        for field, value in [("warmup_usec", 11_000_000), ("ack_p95_usec", 9_999),
                             ("ack_max_usec", 9_999), ("native_p95_usec", 49_999),
                             ("native_max_usec", 49_999)]:
            with self.subTest(field=field):
                values = limits()
                values[field] = value
                with self.assertRaises(InvalidEvidence):
                    verify(*transcript(), values)
        host, client = transcript()
        host["sophia_shell_native_completion"][-1]["monotonic_usec"] = 70_999_999
        with self.assertRaisesRegex(InvalidEvidence, "full workload duration"):
            verify(host, client, limits())

    def test_strict_record_schema(self):
        row = encode({"sophia_shell_native_completion": transcript()[0]["sophia_shell_native_completion"][:1]}).strip()
        self.assertIsNotNone(decode(row, HOST))
        for invalid in [row + " native_frame=1", row.replace("native_owner=1", ""),
                        row.replace("native_frame=1", "native_frame=-1"),
                        row.replace("native_frame=1", "native_frame=18446744073709551616"),
                        row.replace("schema=1", "schema=2"), row.replace("heads=1", "heads=0"),
                        row.replace("timestamp_source=kernel", "timestamp_source=unknown")]:
            with self.subTest(invalid=invalid):
                with self.assertRaises(InvalidEvidence):
                    decode(invalid, HOST)

    def test_nearest_rank_and_maximum_are_independent(self):
        host, client = transcript()
        issue_time = host["sophia_shell_action_receipt"][0]["monotonic_usec"]
        host["sophia_shell_action_receipt"][1]["monotonic_usec"] = issue_time + 80_000
        host["sophia_shell_native_completion"][2]["monotonic_usec"] = issue_time + 250_000
        result = verify(host, client, limits())["outputs"]["1"]
        self.assertEqual(result["ack_p95_usec"], 10_000)
        self.assertEqual(result["ack_max_usec"], 80_000)
        self.assertEqual(result["native_p95_usec"], 50_000)
        self.assertEqual(result["native_max_usec"], 250_000)
        for field, value in [("ack_max_usec", 70_000), ("native_max_usec", 200_000)]:
            with self.subTest(field=field):
                workload = {**limits(), field: value}
                with self.assertRaisesRegex(InvalidEvidence, "latency budget exceeded"):
                    verify(host, client, workload)

    def test_real_cli_pass_and_fail_keep_sources_separate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            host, client = transcript()
            (root / "host").write_text(encode(host))
            (root / "client").write_text(encode(client))
            (root / "budgets").write_text(json.dumps(limits()))
            command = [sys.executable, "-B", str(Path(__file__).resolve().parents[1] / "verify.py"),
                       "--host", str(root / "host"), "--client", str(root / "client"),
                       "--budgets", str(root / "budgets")]
            positive = subprocess.run(command, text=True, capture_output=True, check=False)
            self.assertEqual(positive.returncode, 0, positive.stdout + positive.stderr)
            self.assertEqual(json.loads(positive.stdout)["status"], "pass")
            # A claimed host completion in untrusted client output cannot repair missing host evidence.
            (root / "host").write_text(encode({**host, "sophia_shell_native_completion": []}))
            (root / "client").write_text(encode(client) + encode(host))
            negative = subprocess.run(command, text=True, capture_output=True, check=False)
            self.assertEqual(negative.returncode, 1, negative.stdout + negative.stderr)
            self.assertEqual(json.loads(negative.stdout)["status"], "fail")


if __name__ == "__main__":
    unittest.main()
