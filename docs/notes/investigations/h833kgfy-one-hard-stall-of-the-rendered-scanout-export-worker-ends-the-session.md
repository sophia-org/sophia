---
id: h833kgfy
date: 2026-09-24
kind: investigation
status: investigating
tags: [investigation, scanout, renderer, session-fatal]
---
# One hard stall of the rendered-scanout export worker ends the session

## Question

Mason's installed session (commit 4eacfcfb, the daily desktop) exited at
06:43:33 local on 2026-09-24 after 10.8 hours, with two of this agent's
background jobs at full parallelism on the same machine: the core
conformance profile and the session crate's test suite. What ended it, and
should it have?

## Evidence

`~/.local/state/sophia/hagia-session/session.log`, the last seconds:

- 10:43:32.281Z `sophia_renderer_worker schema=1 status=soft_stall
  age_ms=100`, the first soft stall of the session.
- 10:43:33.175Z `sophia_renderer_worker schema=1 status=hard_stall
  age_ms=1000 action=quarantine`, the first and only hard stall.
- 10:43:33.175Z `sophia_live_native_submit schema=1 status=failed output=1
  reason=ScanoutExportFailed content=Some(MixedPresent { .. })`.
- 10:43:35.790Z the X authority's egress drained (`status=drained
  tickets_advanced=1722251`), then
  `Error: RetirementFailure { message: "Present output cohort failed while
  servicing output 1: submit_status=ScanoutExportFailed; the session also
  refused 61 X requests by_opcode=[2/0/3x7 14/0/9x1 135/3/1x1 145/6/3x26
  145/8/3x26]", retirement: NativeRetirement { latest: Some(1), .. } }`.
- `lifecycle.log`: `sophia_session_diagnostic status=failed phase=session
  installed=true commit=4eacfcfb exit_status=1`, then the hand-off to the
  display manager; `sessions/…/outcome`: `status=failed exit_status=1`.

The refused X requests are the clients' own teardown after the session
stopped answering (BadWindow on ChangeWindowAttributes, BadDrawable on
GetGeometry, and extension errors), not a cause; the application stderr shows
the two launched applications ending after the session did.

The previous session (`session.log.previous`, ended 2026-09-23 19:35 local)
ended through the same class with a different trigger:
`Error: RetirementFailure { message: "MissingSource(DmaBuf { handle: 28 });
the session also refused 25 X requests …" }`, no stall at all.

Load: the profile's report was written at 06:43:35 and the session suite
finished at 06:43:53, so both were running through the stall on a 32-core
machine with cargo and the test harness at full parallelism. Nothing in the
kernel journal (no GPU reset, no OOM). No display, VT or live session was
touched to investigate; the logs were read after the session had ended.

## Finding

The chain is entirely the session's own policy
(`crates/sophia-backend-live`):

1. `scanout/rendered_scanout/exporter/discovery/worker_export.rs`: a
   `WorkerPoll::HardStalled` (the export worker one second late) answers a
   `Degraded` export with `WorkerStalled` and logs `action=quarantine`;
   the worker is not replaced and nothing is retried.
2. The native submit reads that as `ScanoutExportFailed`.
3. `production_visual_runtime/native.rs` (`Present output cohort failed
   while servicing output …`) turns any submit status other than pending,
   in-flight or cleanup into an error, which the owner loop reports as a
   `RetirementFailure` and exits.

A render worker that is a second late once in eleven hours is the machine
being busy, not the session being wrong. Ending the operator's whole
desktop for it -- every client killed, the VT handed back -- is the same
disproportion the note on the Super+button fatal describes: a fault that
should be contained to what it touched (one frame, one worker) is treated as
a loss of the session's integrity.

## Required repair and proof

- On a hard stall: keep the last presented frame on scanout, quarantine the
  stalled worker and replace it (or wait it out with a bound), and count the
  episode; the Present cohort for that output degrades for the frame rather
  than failing the session. Only a worker that never returns within a stated
  bound, or a second failure while degraded, may escalate, and the escalation
  should be the output's, not the session's, when the other outputs are
  fine.
- The `MissingSource(DmaBuf)` ending of the previous session belongs to the
  same review: which retirement failures are integrity losses and which are
  one frame's.
- Proof: a fixture that stalls the export worker for longer than the hard
  bound while a client keeps presenting, asserting the session stays up, the
  degraded frame is counted, and presentation resumes; and the negative
  control (today's policy) that fails it.
- Meanwhile (this agent): builds, suites, probes and gates run under
  `nice -n 19` with `CARGO_BUILD_JOBS=8` whenever a live session may be up,
  and never two at once.

## Connections

- [A Super+button on a window between two committed layouts ends the session](qvj77ywn-a-super-button-on-a-window-between-two-committed-layouts-ends-the-session.md) --
  the same fatal-by-policy shape on the input side.
- [One withheld vblank ended a thirty-five session run](../sources/2026-08/legacy-active-0545-2026-08-28-one-withheld-vblank-ended-a-thirty-five-session-run.md) --
  the earlier lesson that one late frame must not end a session.
- [Authority and lifecycle hardening](../plans/queue-13-authority-and-lifecycle-hardening.md) --
  t032, the page-flip callback bound this sits beside.
