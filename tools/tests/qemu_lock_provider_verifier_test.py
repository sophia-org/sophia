"""The session-lock-provider verifier must pass each mode's evidence and fail
each broken variant by name: lock order, guest stamps, provider health through
the unlock, the one recognized teardown allowed after Session's completion (and the
cleanup and guest completion after it), and reconnects or restarts."""
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
CLEANUP = "sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed"
GUEST_COMPLETE = "sophia_qemu_guest schema=1 status=complete scenario=session-lock-provider"
PASS = "state=failed step=service client={client} remote={remote} refusal=0 service_rc={rc} wire={wire} errno={errno}"
# Each recognized teardown, as the stand-in prints it (the io lines are the
# controls' real EPIPE and ECONNRESET passes).
TEARDOWNS = {
    "estale": PASS.format(client=3, remote=116, rc=-1, wire=0, errno=11),
    "closed": PASS.format(client=4, remote=0, rc=-3, wire=-3, errno=0),
    "io_epipe": PASS.format(client=4, remote=0, rc=-2, wire=-2, errno=32),
    "io_reset": PASS.format(client=4, remote=0, rc=-2, wire=-2, errno=104),
}
COUNTED = {"estale": "estale", "closed": "closed", "io_epipe": "io", "io_reset": "io"}
# The sq1 flood-6 line, from before the stand-in recorded its pass.
SQ1 = "state=failed step=service client=4 remote=0 refusal=0"


def report(mode, state, events, submitted=0):
    return (f"sophia_qemu_lock_provider schema=1 mode={mode} state={state} events={events} "
            f"submitted={submitted} custodied={submitted} again=0 permits=0")


def provider(mode, text):
    return f"sophia_qemu_lock_provider schema=1 mode={mode} {text}"


def evidence(mode, teardown="estale"):
    """One run's evidence as (stamped, line) pairs, in order. `teardown` names
    a TEARDOWNS entry, raw teardown text, or None for no teardown."""
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
        lines.append((True, provider(mode, TEARDOWNS.get(teardown, teardown))))
    lines += [(False, CLEANUP), (False, GUEST_COMPLETE)]
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


def moved_teardown(mode, kind, before):
    """The teardown moved to just before the line `before`."""
    pairs = evidence(mode, teardown=None)
    pairs.insert(index(pairs, before), (True, provider(mode, TEARDOWNS[kind])))
    return pairs


class VerifierTest(unittest.TestCase):
    def verify(self, pairs, mode):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "evidence.log"
            path.write_text(render(pairs) if isinstance(pairs, list) else pairs)
            return subprocess.run([sys.executable, "-B", str(VERIFIER), str(path), mode],
                                  capture_output=True, text=True, check=False, timeout=60)

    def assert_fails(self, pairs, mode, reason):
        result = self.verify(pairs, mode)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(reason, result.stderr)

    def test_each_mode_passes_with_no_or_one_recognized_teardown(self):
        for mode in ("stall", "flood", "baseline"):
            for teardown in (None, *TEARDOWNS):
                with self.subTest(mode=mode, teardown=teardown):
                    result = self.verify(evidence(mode, teardown), mode)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(f"status=pass mode={mode} ", result.stdout)
                    for kind in ("estale", "closed", "io"):
                        count = int(teardown is not None and COUNTED[teardown] == kind)
                        self.assertIn(f"teardown_{kind}={count} ", result.stdout)
                    self.assertIn("basis=guest_log_observed", result.stdout)

    def test_a_recognized_teardown_before_completion_fails(self):
        failed = "provider failed or reported malformed state"
        # In flood the post-unlock report differs from the one held before.
        after_unlock = report("flood", "serving", 9, 9)
        for kind in TEARDOWNS:
            for before in (COMPLETE, QUIESCENCE, after_unlock, UNLOCKED, LOCKED):
                with self.subTest(kind=kind, before=before):
                    self.assert_fails(moved_teardown("flood", kind, before), "flood", failed)

    def test_unrecognized_failures_after_completion_fail(self):
        wrong = {
            "invalid": PASS.format(client=4, remote=0, rc=-1, wire=-1, errno=0),
            "argument": PASS.format(client=4, remote=0, rc=-4, wire=0, errno=0),
            "io_eio": PASS.format(client=4, remote=0, rc=-2, wire=-2, errno=5),
            "io_no_errno": PASS.format(client=4, remote=0, rc=-2, wire=-2, errno=0),
            "io_eagain": PASS.format(client=4, remote=0, rc=-2, wire=-2, errno=11),
            "rc_disagrees_with_wire": PASS.format(client=4, remote=0, rc=-2, wire=-3, errno=32),
            "closed_without_wire": PASS.format(client=4, remote=0, rc=-3, wire=0, errno=0),
            "closed_with_remote": PASS.format(client=4, remote=5, rc=-3, wire=-3, errno=0),
            "closed_refused": PASS.format(client=4, remote=0, rc=-3, wire=-3, errno=0).replace(
                "refusal=0", "refusal=1"),
            "estale_with_wire": PASS.format(client=3, remote=116, rc=-1, wire=-3, errno=0),
            "other_remote": PASS.format(client=3, remote=5, rc=-1, wire=0, errno=0),
            "other_step": TEARDOWNS["closed"].replace("step=service", "step=connection"),
            "missing_pass": "state=failed step=service client=3 remote=116 refusal=0",
            "sq1_flood_6": SQ1,
        }
        for name, text in wrong.items():
            for mode in ("baseline", "flood"):
                with self.subTest(name=name, mode=mode):
                    self.assert_fails(evidence(mode, text), mode, "provider failed or reported malformed state")

    def test_a_second_teardown_fails_whatever_its_kind(self):
        for first, second in (("estale", "estale"), ("estale", "closed"), ("closed", "io_epipe"),
                              ("io_reset", "estale")):
            with self.subTest(first=first, second=second):
                pairs = evidence("baseline", first)
                pairs.insert(index(pairs, CLEANUP), (True, provider("baseline", TEARDOWNS[second])))
                self.assert_fails(pairs, "baseline", "the provider's connection ended more than once")

    def test_a_teardown_needs_session_cleanup_and_guest_completion(self):
        for kind in TEARDOWNS:
            with self.subTest(kind=kind, missing="cleanup"):
                pairs = [pair for pair in evidence("flood", kind) if pair[1] != CLEANUP]
                self.assert_fails(pairs, "flood", "no clean Session cleanup after the provider's teardown")
            with self.subTest(kind=kind, failed="cleanup"):
                text = render(evidence("flood", kind)).replace(CLEANUP, CLEANUP.replace("clean", "failed", 1))
                self.assert_fails(text, "flood", "no clean Session cleanup after the provider's teardown")
            with self.subTest(kind=kind, missing="guest completion"):
                pairs = [pair for pair in evidence("flood", kind) if pair[1] != GUEST_COMPLETE]
                self.assert_fails(pairs, "flood", "the guest did not complete after Session's cleanup")
            with self.subTest(kind=kind, order="guest completion before cleanup"):
                pairs = evidence("flood", kind)
                pairs.insert(index(pairs, CLEANUP), pairs.pop(index(pairs, GUEST_COMPLETE)))
                self.assert_fails(pairs, "flood", "the guest did not complete after Session's cleanup")
            with self.subTest(kind=kind, order="cleanup before teardown"):
                pairs = evidence("flood", kind)
                cleanup = pairs.pop(index(pairs, CLEANUP))
                pairs.insert(index(pairs, provider("flood", TEARDOWNS[kind])), cleanup)
                self.assert_fails(pairs, "flood", "no clean Session cleanup after the provider's teardown")

    def test_teardown_without_completion_fails(self):
        for kind in TEARDOWNS:
            with self.subTest(kind=kind):
                pairs = [pair for pair in evidence("baseline", kind) if pair[1] != COMPLETE]
                self.assert_fails(pairs, "baseline", "provider failed or reported malformed state")

    def test_missing_post_unlock_report_fails(self):
        after = report("baseline", "serving", 2, 0)
        pairs = evidence("baseline", "closed")
        del pairs[[i for i, (_, text) in enumerate(pairs) if text == after][-1]]
        self.assert_fails(pairs, "baseline", "no provider report after the unlock")

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
