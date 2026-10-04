"""The session-lock-provider verifier must pass each mode's evidence and fail
each broken variant by name: lock order, guest stamps, provider health through
the unlock, the one teardown read allowed after Session's completion, and
reconnects or restarts."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

VERIFIER = Path(__file__).resolve().parents[1] / "verify_qemu_session_lock_provider.py"
LOCK = "sophia_live_session_lock schema=1 "
STARTED = "sophia_live_lock_provider schema=1 status=started executable=/usr/bin/sophia-qemu-lock-provider"
CONNECTED = "sophia_live_lock_provider schema=1 status=connected connection_epoch=1 chords=0"
LOCKED = LOCK + "status=locked epoch=1"
UNLOCKED = LOCK + "status=unlocked epoch=1"
QUIESCENCE = "sophia_live_session_quiescence schema=3 status=started reason=input_proof_complete timeout_msec=2000"
COMPLETE = "sophia_live_session schema=18 status=bounded_complete display=:181 surface_resize=disabled present_complete_copy=0"
TEARDOWN = "state=failed step=service client=3 remote=116 refusal=0"


def report(mode, state, events, submitted=0):
    return (f"sophia_qemu_lock_provider schema=1 mode={mode} state={state} events={events} "
            f"submitted={submitted} custodied={submitted} again=0 permits=0")


def evidence(mode, teardown=True):
    """One run's evidence as (stamped, line) pairs, in order."""
    held = {"stall": ("stalled", 1, 0), "flood": ("serving", 5, 5), "baseline": ("serving", 2, 0)}
    after = {"stall": ("stalled", 1, 0), "flood": ("serving", 9, 9), "baseline": ("serving", 2, 0)}
    lines = [
        (False, "sophia_qemu_guest schema=1 status=rerooted pid=1 root_mount=2 parent=1 bwrap_smoke=pass"),
        (True, STARTED),
        (True, CONNECTED),
        (True, report(mode, "negotiated", 1)),
        (True, LOCK + "status=locking source=proof epoch=1 input_epoch=3 revoked_leases=0"),
        (True, LOCKED),
        (True, report(mode, *held[mode])),
        (False, "sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right"),
        (True, LOCK + "status=checking epoch=1 attempt=1"),
        (True, LOCK + "status=unlocking epoch=1 input_epoch=4 revoked_leases=0"),
        (True, UNLOCKED),
        (True, report(mode, *after[mode])),
        (False, QUIESCENCE),
        (False, COMPLETE),
    ]
    if teardown:
        lines.append((True, f"sophia_qemu_lock_provider schema=1 mode={mode} {TEARDOWN}"))
    return lines


def render(pairs):
    out, ns = [], 1_000_000
    for stamped, line in pairs:
        if stamped:
            ns += 1_000_000
            out.append(f"sophia_qemu_stamp schema=1 mono_ns={ns}")
        out.append(line)
    return "\n".join(out) + "\n"


def index(pairs, line):
    return next(i for i, (_, text) in enumerate(pairs) if text == line)


def moved_teardown(mode, before):
    """The teardown read moved to just before the line `before`."""
    pairs = evidence(mode, teardown=False)
    pairs.insert(index(pairs, before), (True, f"sophia_qemu_lock_provider schema=1 mode={mode} {TEARDOWN}"))
    return pairs


class VerifierTest(unittest.TestCase):
    def verify(self, pairs, mode):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "evidence.log"
            path.write_text(render(pairs) if isinstance(pairs, list) else pairs)
            return subprocess.run([sys.executable, "-B", str(VERIFIER), str(path), mode],
                                  capture_output=True, text=True, check=False)

    def assert_fails(self, pairs, mode, reason):
        result = self.verify(pairs, mode)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(reason, result.stderr)

    def test_each_mode_passes_with_and_without_the_teardown_read(self):
        for mode in ("stall", "flood", "baseline"):
            for teardown in (True, False):
                with self.subTest(mode=mode, teardown=teardown):
                    result = self.verify(evidence(mode, teardown), mode)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(f"status=pass mode={mode} ", result.stdout)
                    self.assertIn(f"teardown_estale={int(teardown)} ", result.stdout)
                    self.assertIn("basis=guest_log_observed", result.stdout)

    def test_teardown_read_before_completion_fails(self):
        failed = "provider failed or reported malformed state"
        for before in (COMPLETE, QUIESCENCE, UNLOCKED, LOCKED):
            with self.subTest(before=before):
                self.assert_fails(moved_teardown("baseline", before), "baseline", failed)

    def test_other_teardown_errors_fail(self):
        for wrong in ("remote=5 refusal=0", "remote=116 refusal=1"):
            with self.subTest(wrong=wrong):
                text = render(evidence("baseline")).replace("remote=116 refusal=0", wrong)
                self.assert_fails(text, "baseline", "provider failed or reported malformed state")

    def test_second_teardown_read_fails(self):
        pairs = evidence("baseline")
        pairs.append(pairs[-1])
        self.assert_fails(pairs, "baseline", "the provider's connection ended more than once")

    def test_teardown_read_without_completion_fails(self):
        pairs = [pair for pair in evidence("baseline") if pair[1] != COMPLETE]
        self.assert_fails(pairs, "baseline", "provider failed or reported malformed state")

    def test_completion_before_the_post_unlock_report_fails(self):
        pairs = [pair for pair in evidence("baseline") if pair[1] != COMPLETE]
        pairs.insert(index(pairs, UNLOCKED) + 1, (False, COMPLETE))
        self.assert_fails(pairs, "baseline", "Session completed before the provider reported after the unlock")

    def test_reconnect_and_restart_fail(self):
        pairs = evidence("baseline")
        pairs.insert(index(pairs, UNLOCKED) + 1, (True, CONNECTED.replace("epoch=1", "epoch=2")))
        self.assert_fails(pairs, "baseline", "the provider must connect exactly once, before the lock")
        pairs = evidence("baseline")
        pairs.insert(index(pairs, UNLOCKED) + 1, (True, STARTED))
        self.assert_fails(pairs, "baseline", "expected exactly one provider start, found 2")

    def test_missing_or_backward_stamps_fail(self):
        text = render(evidence("stall"))
        lines = text.splitlines()
        at = lines.index(UNLOCKED)
        self.assert_fails("\n".join(lines[:at - 1] + lines[at:]) + "\n", "stall", "unlock has no guest stamp")
        lines[at - 1] = "sophia_qemu_stamp schema=1 mono_ns=1"
        self.assert_fails("\n".join(lines) + "\n", "stall", "guest stamps go backwards")

    def test_stalled_counters_must_hold_and_flood_must_span(self):
        text = render(evidence("stall")).replace(
            report("stall", "stalled", 1) + "\n", report("stall", "stalled", 2) + "\n", 1)
        self.assert_fails(text, "stall", "the stalled provider's counters moved")
        text = render(evidence("flood")).replace(report("flood", "serving", 9, 9), report("flood", "serving", 5, 5))
        self.assert_fails(text, "flood", "the provider's flood did not span the unlock")

    def test_reroot_and_secrets_are_required_and_refused(self):
        pairs = evidence("stall")[1:]
        self.assert_fails(pairs, "stall", "expected exactly one re-root, found 0")
        text = render(evidence("stall")) + "typed qzv\n"
        self.assert_fails(text, "stall", "a password appears in the evidence")


if __name__ == "__main__":
    unittest.main()
