import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import runner
import prepare
import analyze
import build
import re

FAKE = r'''
fn main() {
    let _trace = t082_trace::start();
    let mode = std::env::args().nth(1).unwrap();
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    assert!(input.starts_with("SETDESC T082 DIAGNOSTIC ONLY."));
    assert!(input.ends_with("\nGETPIN\nBYE\n"));
    println!("OK ready\nOK");
    t082_trace::mark("window_created", 1234);
    t082_trace::mark("run_native_enter", 0);
    t082_trace::mark("submit_enter", 0);
    t082_trace::mark("swap_enter", 0);
    if mode == "stall" { std::thread::sleep(std::time::Duration::from_secs(30)); }
    if mode == "flood" {
        for _ in 0..100_000 { t082_trace::mark("app_update", 0); }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if mode == "connections" {
        for pointer in 100..117 { t082_trace::connection(pointer); }
    }
    if mode == "threads" {
        std::thread::scope(|s| {
            for _ in 0..2 { s.spawn(|| { for _ in 0..10 { t082_trace::mark("app_update", 0); } }); }
        });
    }
    t082_trace::mark("swap_return", 1);
    t082_trace::mark("run_native_return", 0);
    if mode == "wrong" { println!("D PRIVATE_CANARY_NEVER_LOG\nOK\nOK closing"); }
    else { println!("D test\nOK\nOK closing"); }
    eprintln!("PRIVATE_STDERR_NEVER_LOG");
}
'''

class ProbeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = tempfile.TemporaryDirectory(prefix="t082-tests-")
        cls.root = Path(cls.scratch.name)
        here = Path(__file__).resolve().parent
        subprocess.run(["rustc", "--edition=2021", "--crate-type=lib", "--crate-name=t082_trace", str(here / "trace.rs"), "--out-dir", str(cls.root)], check=True)
        (cls.root / "fake.rs").write_text(FAKE)
        cls.binary = cls.root / "fake"
        subprocess.run(["rustc", "--edition=2021", str(cls.root / "fake.rs"), "--extern", f"t082_trace={cls.root}/libt082_trace.rlib", "-o", str(cls.binary)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.scratch.cleanup()

    def run_probe(self, mode, **kwargs):
        path = self.root / self.id().split(".")[-1]
        result = runner.run_case([str(self.binary), mode], path, "enter", True, **kwargs)
        return result, path

    def test_real_trace_pipe_and_protocol_complete(self):
        result, path = self.run_probe("complete")
        self.assertEqual(result["result"], "completed")
        self.assertTrue(result["trace_complete"])
        self.assertTrue(result["dummy_matches"])
        self.assertEqual(result["window_ids"], [1234])
        self.assertIsNone(result["last_observed_open_span"])
        self.assertNotIn("PRIVATE_STDERR_NEVER_LOG", (path / "stages.jsonl").read_text())

    def test_connection_identity_is_opaque_bounded_and_overflow_is_inconclusive(self):
        result, path = self.run_probe("connections")
        records = [json.loads(line) for line in (path / "stages.jsonl").read_text().splitlines()]
        ids = [r["value"] for r in records if r.get("stage") == "xcb_connection"]
        self.assertEqual(ids, list(range(1, 17)) + [0])
        self.assertTrue(result["trace_loss"])
        self.assertEqual(result["result"], "inconclusive")

    def test_wrong_value_never_enters_logs_or_counts_as_success(self):
        result, path = self.run_probe("wrong")
        self.assertNotEqual(result["result"], "completed")
        self.assertFalse(result["dummy_matches"])
        for file in path.iterdir():
            self.assertNotIn("PRIVATE_CANARY_NEVER_LOG", file.read_text())
            self.assertNotIn("PRIVATE_STDERR_NEVER_LOG", file.read_text())

    def test_submission_watchdog_terminates_only_its_child(self):
        healthy = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        try:
            result, path = self.run_probe("stall", lifetime=2, submitted_timeout=0.25)
            self.assertEqual(result["harness_termination"], "submission_deadline")
            self.assertTrue(result["heartbeat_fresh_at_cleanup"])
            self.assertEqual(result["last_observed_open_span"], "swap_enter")
            self.assertFalse(result["trace_loss"])
            self.assertIsNone(healthy.poll())
            with self.assertRaises(ProcessLookupError):
                os.kill(result["pid"], 0)
            self.assertIn('"event":"harness_termination"', (path / "stages.jsonl").read_text())
        finally:
            healthy.terminate()
            healthy.wait(timeout=2)

    def test_total_lifetime_is_independent_of_heartbeats(self):
        result, _ = self.run_probe("stall", lifetime=0.25, submitted_timeout=2)
        self.assertEqual(result["harness_termination"], "case_deadline")

    def test_overflow_is_inconclusive_not_a_fake_native_stall(self):
        result, _ = self.run_probe("flood")
        self.assertTrue(result["trace_loss"])
        self.assertEqual(result["result"], "inconclusive")

    def test_concurrent_writers_do_not_interleave_pipe_records(self):
        result, _ = self.run_probe("threads")
        self.assertEqual(result["result"], "completed")
        self.assertFalse(result["trace_loss"])

    def test_trace_parser_rejects_wrong_identity_and_arbitrary_data(self):
        for data in (b"secret", b"t082 0 123 ThreadId(1) 1 1 password 0 0", b"t082 0 999 ThreadId(1) 1 1 app_update 0 0", b"t082 0 123 ThreadId(1) 1 1 app_update 123 0"):
            with self.assertRaises((ValueError, UnicodeError)):
                runner.parse_trace(data, 123)

    def test_cancel_protocol_has_no_data_response(self):
        protocol = runner.Protocol()
        for line in (b"OK ready", b"OK", b"ERR 83886179 Operation cancelled", b"OK closing"):
            protocol.feed(line)
        self.assertEqual(protocol.phase, "done")
        self.assertEqual(protocol.terminal, "cancelled")
        self.assertFalse(protocol.invalid)
        protocol.feed(b"D PRIVATE_CANARY_NEVER_LOG")
        self.assertTrue(protocol.invalid)

    def test_build_jobs_default_to_every_cpu_and_honor_the_caller(self):
        env = build.build_env({"PATH": "/usr/bin", "SOPHIA_X": "1", "HAGIA_Y": "2"})
        self.assertEqual(env, {"PATH": "/usr/bin", "CARGO_BUILD_JOBS": str(os.cpu_count() or 1)})
        self.assertEqual(build.build_env({"CARGO_BUILD_JOBS": "3"})["CARGO_BUILD_JOBS"], "3")

    def test_every_instrumented_marker_is_accepted(self):
        source = (Path(__file__).parent / "build.py").read_text()
        stages = set(re.findall(r'(?:mark\(|t082_trace::mark\()"([a-z_]+)"', source))
        self.assertIn("close_processed", stages)
        self.assertFalse(stages - runner.STAGES)
        for stage in stages:
            runner.parse_trace(f"t082 0 123 ThreadId(1) 1 1 {stage} 0 0".encode(), 123)

    def test_analysis_distinguishes_loss_swap_and_teardown(self):
        summary = dict(trace_loss=False, instrumented=True, result="stalled_or_failed",
                       trace_complete=False, heartbeat_fresh_at_cleanup=True,
                       last_observed_open_span="swap_enter")
        self.assertIn("swap_enter", analyze.boundary(summary, []))
        summary["trace_loss"] = True
        self.assertIn("Inconclusive", analyze.boundary(summary, []))
        summary["trace_loss"] = False
        summary["last_observed_open_span"] = "run_native_enter"
        self.assertIn("including teardown", analyze.boundary(summary, [dict(seq=1, stage="event_loop_return")]))
        self.assertIn("not evidence of a blocked swap", analyze.boundary(summary, [dict(seq=1, stage="run_native_error")]))

    def test_span_correlation_preserves_other_threads_connections_and_sequences(self):
        def event(seq, stage, value=0, thread="ThreadId(1)"):
            return dict(seq=seq, stage=stage, value=value, thread=thread)
        events = [event(0, "destroy_enter"), event(1, "xcb_connection", 1),
                  event(2, "reply_wait_enter", 872),
                  event(3, "reply_wait_return", 872, "ThreadId(2)"),
                  event(4, "xcb_connection", 2), event(5, "reply_wait_return", 872),
                  event(6, "xcb_connection", 1), event(7, "reply_wait_return", 873)]
        self.assertEqual([s["stage"] for s in runner.open_spans(events)],
                         ["destroy_enter", "reply_wait_enter"])
        events.append(event(8, "reply_wait_return", 872))
        self.assertEqual(runner.last_open_span(events), "destroy_enter")
        events.append(event(9, "destroy_return"))
        self.assertEqual(runner.open_spans(events), [])

    def test_new_metadata_is_bounded_and_case_selection_is_explicit(self):
        for stage, value in (("window_event_kind", 3), ("xcb_connection", 16),
                             ("reply_wait_enter", 872), ("window_drop_enter", 8388611)):
            runner.parse_trace(f"t082 0 123 ThreadId(1) 1 1 {stage} {value} 0".encode(), 123)
        for stage, value in (("window_event_kind", 4), ("xcb_connection", 17),
                             ("window_drop_enter", 2**32)):
            with self.assertRaises(ValueError):
                runner.parse_trace(f"t082 0 123 ThreadId(1) 1 1 {stage} {value} 0".encode(), 123)
        self.assertEqual(runner.selected_cases("instrumented-enter"), [("instrumented", "enter")])
        self.assertEqual(len(runner.selected_cases("full")), 6)

    def test_completed_teardown_does_not_prove_a_destroy_was_flushed(self):
        summary = dict(trace_loss=False, instrumented=True, result="stalled_or_failed",
                       trace_complete=False, heartbeat_fresh_at_cleanup=True,
                       last_observed_open_span="run_native_enter")
        events = [dict(seq=1, stage="destroy_return")]
        self.assertIn("buffered destroy", analyze.boundary(summary, events))
        events.append(dict(seq=2, stage="window_event_kind"))
        self.assertIn("later window event arrived", analyze.boundary(summary, events))
        summary["trace_loss"] = True
        self.assertIn("Inconclusive", analyze.boundary(summary, events))

    def test_capture_preparation_checks_every_anchor_and_shell_syntax(self):
        root = Path(__file__).resolve().parents[3]
        dest = self.root / "prepared"
        dest.mkdir()
        prepare.prepare(root, dest)
        self.assertIn('>>"$T082_CAPTURE/session.raw.log"', (dest / "run-session").read_text())
        self.assertIn("--case instrumented-enter", (dest / "terminal.rc").read_text())
        bad = self.root / "wrong-release"
        (bad / "tools").mkdir(parents=True)
        (bad / "tools/run_sophia_session.sh").write_text("#!/bin/sh\n")
        with self.assertRaises(RuntimeError):
            prepare.prepare(bad, dest)

if __name__ == "__main__":
    unittest.main()
