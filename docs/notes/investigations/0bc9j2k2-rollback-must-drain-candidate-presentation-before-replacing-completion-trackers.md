---
id: 0bc9j2k2
date: 2026-10-01
kind: investigation
status: closed
tags: [investigation, rendering, validation]
---
# Rollback must drain candidate presentation before replacing completion trackers

## Trigger and evidence

The attended revision-1 output gate on Sophia
`344e9534b32e176f2ffb4cea8694269758ad57f0`, assembled by niltempus
`4f4e5ba34eb1fac33bba48e36b007f5bd6bd32a2`, passed validate, rejection and
commit A-to-B-to-A. The final peer-death stage failed. The declared layout kept
both heads enabled and changed only DP-1 from 1440p120 to 1440p60. Session killed
the supervised peer after all cards applied B, while withholding terminal
first-presentation acceptance.

Evidence root:
`~/.local/state/sophia/development-evidence/ipc-retirement/`.

- `t253-native-run-4f4e5ba-01/run/`: retained four-stage attempt; the first three
  stage verdicts passed. The overall result is failed.
- `run/peer-death/state/sophia/output-file-native-session/untrusted-session-output.log`
  under that run root: lines 122–149 show complete candidate and rollback
  preparation, apply, termination request, installation and queued first frames
  11/12. Lines 150–153 show signal-15 exit, pause, disconnect and cancellation.
  Lines 154–156 show accepted KMS rollback followed by the fatal runtime rebind
  refusal. Lines 1729–1731 report forced detach and two in-flight scanouts.
- The adjacent `recovery.log` records normal console and keyboard restoration;
  every stage's foreground-console check matched tty4.
- `t253-peer-death-audit-01/AUDIT.md`: independent source and log diagnosis.

There is no restored readback, local RolledBack settlement or peer-loss proof
pass. Accepted reverse KMS programming alone does not prove restoration.

## Cause

Peer exit is asynchronous. Between requesting termination and observing it,
Session can install B and submit its first frames. Cancellation then changes
the preparation to RollingBack. Normal frame service is permitted only in
FirstFramesQueued, so those submitted flips stop retiring.

The reverse modeset was allowed to run immediately. Installing its result
replaced the candidate's submitted-content and page-flip trackers before the
runtime relinquished its submitted owners. Rebind correctly refused
`native topology runtime rebind requires quiescent presentation ownership`.
Later cleanup could no longer correlate callbacks with the original pacing
cycles, producing MissingCycle errors and two abandoned scanouts. Session's
completion rollback loop had the same ordering gap.

## Correction

Both normal cancellation and completion now use the same runtime retirement
step before reverse KMS programming:

1. Cancel unsubmitted prepared frames and drain renderer work already in flight.
   Completed exports are released without creating or submitting a framebuffer.
2. Pump native completion events and retire submitted flips through their
   existing custody and pacing owners. Mirror retirement keeps its own cohort
   cleanup consumer. This path makes no new submissions and does not accept
   the topology's first presentation.
3. Once physical submissions have settled, skip remaining queued or unframed
   client presents through the existing feedback path. Keep the displayed
   framebuffer in custody.
4. Admit reverse KMS programming only after runtime and native ownership are
   quiescent. The backend also returns Retry while native ownership remains.
   Blocking restoration then makes the candidate's displayed owner safe for
   the existing rebind handoff to retire.

Session paces the wait at one millisecond and bounds it by two seconds. Normal
operation starts that bound on its first rollback turn; completion uses its
existing abort deadline. An expired wait reports a named rollback-quiescence
error without running the reverse apply. Readiness accepted before the deadline
is latched across later cards, with new frame service still gated.

The completion path no longer suspends candidate scanout before restoration.
The drain does not discard submitted custody or reset its trackers to make the
rebind predicate pass.

## Deterministic validation and limits

`t253-rollback-drain-01/` retains backend checks. The fake-device handoff test
uses the production custody operations: topology owner 70 remains displayed
while frame 71 is submitted; early retirement is refused; an accepted flip
retires 70 exactly once and displays 71; only a supplied blocking restoration
permits the handoff to retire 71 and adopt rollback owner 72.

The backend library (211 cases), libdrm-events integration (305 cases), existing
presentation-skip tests (three cases) and Session output tests (90 passed,
three ignored) pass. Strict clippy passes for both changed crates with every
feature and target enabled. The first repository-wide attempt stopped at the
CLI lifecycle test because the isolated harness omitted `XDG_RUNTIME_DIR`;
that log is retained separately from the rerun with a private runtime directory.
The rerun reached the Session library (738 passed, 25 ignored) but failed the
unchanged shell fixture `the_content_profile_cannot_admit_a_descriptor_child`:
its supervisor started the test binary with `--serve`, and the child exited 101
before protection capture. The isolated rerun passed while printing
`Unrecognized option: 'serve'`. This fixture modifies only `base_launch_spec`
but directly starts the supervisor's original spec; it can therefore pass on
transport timeout or fail on early child exit. The full repository gate is not
claimed green, and its failure is retained in log 06 with the isolated result
in log 07. This separate shell-test defect does not execute the rollback path.

`t253-rollback-quiescence-01/` retains seven Session gate tests and two private
mutant runs. Ignoring pending ownership fails four tests. Checking the deadline
after quiescence fails the deadline and late-readiness tests. Canonical sources
were never mutated; the controls used a separate copy and target directory.

These tests do not execute the composed runtime drain with real DRM events or
renderer workers. The handoff's blocking restoration and flip are supplied.
The next attended run must prove that completion pumping, retirement, reverse
apply, readback, local settlement and clean shutdown compose on hardware.
No old failed run becomes accepted through this correction. A new signed
candidate requires fresh native preparation, performance evidence, integration
pins and sealed inputs before that run.

## Connections

The [singleton custody correction](foc74sm7-topology-rebind-must-transfer-singleton-framebuffer-custody-before-ordinary-presentation.md)
and [installed rollback timing correction](l696uhdc-rollback-preparation-must-use-the-installed-mode-after-a-topology-commit.md)
passed their previously failing native boundary: commit-restore now succeeds.
This investigation owns the subsequent peer-death failure. Acceptance remains
in [t253](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t253--migrate-the-separate-output-role);
output IPC retirement in t272 still waits for it.

## Physical acceptance, 2026-10-01

Signed Sophia `ddd27bd6d9`, assembled by integration `bec6db137d`, passed all
four attended stages, including commit-restore and peer death after apply.
Restored KMS and owner readbacks, local RolledBack settlement, the joined
peer-loss verdict and clean native/console shutdown are present. The
[acceptance record](../milestones/ofard23a-accept-the-revision-1-output-file-role-through-native-rollback.md)
binds the exact manifests, independent audits and performance evidence.
This closes the observed defect for the declared one-card, two-head,
refresh-only fixture. Earlier failures and the deterministic-test limits
above remain evidence; they are not relabelled as passing runs.
