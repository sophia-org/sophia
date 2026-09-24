---
id: h833kgfy
date: 2026-09-24
kind: investigation
status: resolved
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

## Resolution (2026-09-24, t186; the output-level escalation is t191)

A hard stall is no longer a quarantine. The worker facade
(`scanout/rendered_scanout/exporter/worker.rs`) now keeps a hard-stalled
render as `stalled`: nothing else is submitted while it is out, so its late
result can never be assigned to another frame; when that result arrives it
is released, never assigned, and the facade takes renders again
(`stall_recoveries`). Only a render that never returns within
`LIVE_RENDERER_WORKER_STALL_ABANDON` (ten seconds) quarantines the facade
(`stalls_abandoned`), and that is reported as the failure it is. The
exporter (`discovery/worker_export.rs`) answers a hard stall, and every
tick while the render is still out, with a *pending* export rather than a
degraded one, so the native path keeps the last frame on scanout and
retries, as it does for any pending export; the log reads
`status=hard_stall … action=wait abandon_after_ms=10000` and the session
does not end. The maintenance paths that touch what a render may be using
treat a stalled render as in flight.

Proof, `tests/support/renderer_worker_correlation.rs`, on the worker's
channel harness without a GPU: a render a second late is called stalled,
a second submit is refused while it is out, its late result is released
and never assigned, and the next render is accepted with a fresh identity;
a render that never returns is abandoned exactly at the bound, and only
then is the facade quarantined and the next submit refused. Red before
(the first case quarantined the facade for good), green after. The
backend-live suite, clippy with and without features, and the two
transport gates pass; the native scanout path itself runs only on the
installed session.

What remains, filed as t191: after the abandon bound the failure still
reaches the session through the Present cohort as before, and the worker
core (one thread per device) is not replaced; both belong to the output,
not the session, and a replacement needs a fresh EGL context under a live
scanout.

## Required repair and proof (as filed)

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
