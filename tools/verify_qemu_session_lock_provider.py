#!/usr/bin/env python3
"""Verify a session-lock-provider QEMU run and report its unlock latencies.

The scenario is session-lock with a stand-in lock provider
(tools/qemu_lock_provider_standin.c) in one mode, and only the right
password typed. This checks the lock's own record and order, then the
provider's state around the unlock:

  stall     negotiated once, stopped after reading the Locked object and
            before the password was typed, still stopped after the unlock
            (no counter moved), and never revoked or reconnected before the
            unlock: the stall was in force at the verdict;
  flood     negotiated once and still submitting across the observed unlock:
            its activity grew from the last report before the accepted
            verdict to the first report after the unlock;
  baseline  negotiated once and serving.

Provider health is bounded: from its start through the unlock and the first
report after it, every provider report must be healthy. Only after Session's
own completion record ("sophia_live_session ... status=bounded_complete"),
which must follow that post-unlock report, may the provider's connection
end, and then only as one expected teardown read (TEARDOWN below: the
closing lock service answers ESTALE). Any other failure, a second one, or
that one anywhere earlier, fails; so does a reconnection or a restart.

Latencies come from the guest's stamps (tools/qemu_line_stamp.c), which
record when the stamper observed each line on the session's output pipe:
log observation, not the instant a cover left a screen. They are reported,
never judged against a threshold:

  pam_ms                 checking attempt=1 -> unlocking (accepted verdict,
                         new input epoch)
  verdict_to_unlocked_ms unlocking -> unlocked (cover withdrawn)
"""

import re
import sys

LOCK = "sophia_live_session_lock schema=1 "
STAMP = re.compile(r"^sophia_qemu_stamp schema=1 mono_ns=(\d+)$")
COMPLETE = re.compile(r"^sophia_live_session schema=\d+ status=bounded_complete ")
TEARDOWN = "state=failed step=service client=3 remote=116 refusal=0"
REPORT = re.compile(
    r"^sophia_qemu_lock_provider schema=1 mode=(\w+) state=(\w+) events=(\d+) "
    r"submitted=(\d+) custodied=(\d+) again=(\d+) permits=(\d+)$"
)


def fail(message):
    print(f"QEMU session-lock-provider evidence: {message}", file=sys.stderr)
    sys.exit(1)


def main():
    if len(sys.argv) != 3 or sys.argv[2] not in ("baseline", "flood", "stall"):
        fail("usage: verify_qemu_session_lock_provider.py EVIDENCE baseline|flood|stall")
    path, mode = sys.argv[1], sys.argv[2]
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = [line.rstrip("\n") for line in f]

    # Each stamped line carries the stamp written just before it. One clock,
    # one writer: stamps never go backwards.
    stamps = {}
    previous = -1
    for i, line in enumerate(lines):
        match = STAMP.match(line)
        if match:
            value = int(match.group(1))
            if value < previous:
                fail("guest stamps go backwards")
            previous = value
            if i + 1 < len(lines):
                stamps[i + 1] = value

    def only(pattern, description):
        found = [i for i, line in enumerate(lines) if re.fullmatch(pattern, line)]
        if len(found) != 1:
            fail(f"expected exactly one {description}, found {len(found)}")
        return found[0]

    def stamped(index, description):
        if index not in stamps:
            fail(f"{description} has no guest stamp")
        return stamps[index]

    locking = only(LOCK + r"status=locking source=proof epoch=1 input_epoch=\d+ revoked_leases=\d+", "lock start")
    locked = only(LOCK + "status=locked epoch=1", "covered lock")
    typed = only("sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right", "password delivery")
    checking = only(LOCK + "status=checking epoch=1 attempt=1", "attempt")
    unlocking = only(LOCK + r"status=unlocking epoch=1 input_epoch=\d+ revoked_leases=\d+", "accepted verdict")
    unlocked = only(LOCK + "status=unlocked epoch=1", "unlock")
    started = only(r"sophia_live_lock_provider schema=1 status=started executable=/usr/bin/sophia-qemu-lock-provider", "provider start")

    # PID 1 re-rooted off the initramfs rootfs and a bubblewrap smoke passed
    # before Session started (tools/qemu_reroot.c, tools/qemu_guest_init.sh).
    rerooted = only(r"sophia_qemu_guest schema=1 status=rerooted pid=1 root_mount=\d+ parent=\d+ bwrap_smoke=pass",
                    "re-root")
    if not rerooted < started:
        fail("the provider started before the re-root")
    if not locking < locked < checking < unlocking < unlocked:
        fail("lock steps are out of order")
    if not started < locking:
        fail("the provider started after the lock")
    # The host writes its marker after the keys went out; the guest may have
    # acted on them first, so the marker bounds only what preceded the typing.
    if not typed > locked:
        fail("the password was typed before the lock")
    for line in lines:
        if line.startswith(LOCK) and re.search(
            r"status=(refused|stale_verdict|unavailable|unlock_repaint_failed|already_locked|failed)( |$)", line
        ):
            fail("the lock recorded a refusal, failure, stale verdict or unavailable authenticator")
        if "sophialock" in line or re.search(r"\bqzv\b", line):
            fail("a password appears in the evidence")

    # Session's completion record bounds provider health.
    completes = [i for i, line in enumerate(lines) if COMPLETE.match(line)]
    if len(completes) > 1:
        fail("Session completed more than once")
    stopping = completes[0] if completes else len(lines)
    reports = []
    teardown = []
    for i, line in enumerate(lines):
        if line.startswith("sophia_qemu_lock_provider "):
            if line == f"sophia_qemu_lock_provider schema=1 mode={mode} {TEARDOWN}" and i > stopping:
                teardown.append(i)
                continue
            match = REPORT.match(line)
            if not match:
                fail(f"provider failed or reported malformed state: {line}")
            if match.group(1) != mode:
                fail(f"provider mode {match.group(1)} is not {mode}")
            counters = tuple(int(match.group(n)) for n in range(3, 8))
            reports.append((i, match.group(2), counters))
    negotiated = [r for r in reports if r[1] == "negotiated"]
    if len(negotiated) != 1 or negotiated[0][0] > locked:
        fail("the provider must negotiate exactly once, before the lock")
    if any(line.startswith("sophia_live_lock_provider schema=1 status=exited") for line in lines[:unlocked]):
        fail("the provider exited before the unlock")
    # One connection for the whole run: a restart or reconnection fails closed.
    connections = [i for i, line in enumerate(lines)
                   if line.startswith("sophia_live_lock_provider schema=1 status=connected ")]
    if len(connections) != 1 or connections[0] > locking:
        fail("the provider must connect exactly once, before the lock")

    pam_ms = (stamped(unlocking, "accepted verdict") - stamped(checking, "attempt")) / 1e6
    unlock_ms = (stamped(unlocked, "unlock") - stamped(unlocking, "accepted verdict")) / 1e6
    if pam_ms < 0 or unlock_ms < 0:
        fail("a lock interval is negative")
    unlocked_at = stamped(unlocked, "unlock")
    after_unlock = [r for r in reports if r[0] in stamps and stamps[r[0]] > unlocked_at]
    if not after_unlock:
        fail("no provider report after the unlock")
    if not after_unlock[0][0] < stopping:
        fail("Session completed before the provider reported after the unlock")
    if len(teardown) > 1:
        fail("the provider's connection ended more than once")

    if mode == "stall":
        stalled = [r for r in reports if r[1] == "stalled"]
        if not stalled or stalled[0][0] < locking:
            fail("the provider did not stall after the lock started")
        settled = [r for r in reports if r[0] >= stalled[0][0]]
        if not any(locked < r[0] < typed for r in settled):
            fail("the provider was not shown stalled between the lock and the typing")
        if any(r[1] != "stalled" or r[2] != stalled[0][2] for r in settled):
            fail("the stalled provider's counters moved")
        if any(line.startswith(("sophia_live_lock_provider schema=1 status=disconnected",
                                "sophia_live_lock_provider schema=1 status=connection_failed"))
               for line in lines[:unlocked]):
            fail("Session dropped the stalled provider before the unlock: the stall was not in force")
        detail = f"stalled_reports={len(settled)} counters_constant=true connected_at_unlock=true"
    elif mode == "flood":
        before = [r for r in reports if r[0] < unlocking and r[1] == "serving"]
        if not before:
            fail("no flooding report before the accepted verdict")
        first, last = before[-1], after_unlock[0]

        def activity(report):
            return report[2][1] + report[2][3]  # submitted + EAGAIN retries

        if last[1] != "serving" or activity(last) <= activity(first) or activity(first) == 0:
            fail("the provider's flood did not span the unlock")
        detail = f"activity_before={activity(first)} activity_after={activity(last)}"
    else:
        if not any(r[1] == "serving" for r in reports):
            fail("the baseline provider never served")
        detail = "serving=true"

    connection = [line for line in lines if line.startswith("sophia_live_lock_provider schema=1 status=")]
    revoked = sum("status=disconnected" in line or "status=connection_failed" in line for line in connection)
    print(
        f"sophia_qemu_session_lock_provider_evidence schema=1 status=pass mode={mode} "
        f"pam_ms={pam_ms:.3f} verdict_to_unlocked_ms={unlock_ms:.3f} "
        f"basis=guest_log_observed provider_disconnects={revoked} "
        f"teardown_estale={len(teardown)} {detail}"
    )


if __name__ == "__main__":
    main()
