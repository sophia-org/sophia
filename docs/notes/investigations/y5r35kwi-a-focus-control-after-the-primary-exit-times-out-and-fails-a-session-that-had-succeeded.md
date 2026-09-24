---
id: y5r35kwi
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, session, lifecycle, session-fatal, gate]
---
# A focus control after the primary's exit times out and fails a session that had succeeded

## Question

The `xtest-selection` gate on commit f499cbe2 (t064, a change to pointer
projections only) read `session exited unsuccessfully; no client verdict`
although its driver had finished its work. A rerun passed. What ended the
first session?

## Evidence

`.artifacts/xtest-selection/f499cbe2-1790247697/pass.log`:

- The driver completed: `xtest_selection_driver: watched 0x400016 received
  0 events`, `owner=0x400016 bytes=26 row=0`.
- `sophia_live_session_quiescence schema=3 status=started
  reason=successful_primary_exit timeout_msec=2000`, then the second
  xterm's window took the focus (`sophia_x11_focus_delivery … window=6291468
  focused=true`), and the frontend drained
  (`status=frontend_drained … elapsed_msec=7`).
- `sophia_live_session_control schema=1 status=control_refused
  kind=FocusSurface transaction=1000001 surface=6291468 failure=TimedOut`
  (`live_session/owner_loop/session_control.rs`, the command from
  `live_session/policy.rs`).
- `sophia_live_session_runtime_fatal schema=1 status=detected
  source=owner_loop action=bounded_cleanup failure_code=control_timeout
  error="session control failure: TimedOut"`, `sophia_session_failure …
  failure_code=unclassified`, `sophia_session_result schema=1
  status=failed`, `Error: RetirementFailure { message: "session control
  failure: TimedOut", .. }`.

The rerun on the same commit passed with the driver's exact pass line, so
the failure is a race, not the change under test.

## Finding

Quiescence after a successful primary exit drains the frontend, and a
focus transition that the surviving client caused in that window is turned
into a `FocusSurface` control by the policy after the drain has begun.
Nothing is left to answer it, it times out, and the owner loop treats the
timeout as a runtime fatal: a session whose primary already exited
successfully is recorded as failed, and a gate that depends on the session's
exit status reads no verdict. The same shape as the render-worker stall
(t186): a bounded, explainable late event on the way out is treated as a
loss of integrity.

## Resolution (2026-09-24)

The control queue now knows about quiescence (`session_control.rs`,
`SessionControlQueue::begin_quiescence`), and the owner loop tells it the
moment quiescence begins for a successful primary exit, right after the
frontend is told to drain. From then on every pending control -- in flight
or waiting -- is retired as `SessionControlFailure::Quiescing`, reported by
the next service; a control enqueued afterwards (the focus transition the
exit itself caused) is accepted and retired the same way, so the policy's
enqueue does not fail either; and a late acknowledgement for a retired
control is inert rather than `UnexpectedAcknowledgement`. The owner loop
treats `Quiescing` as a classified, non-fatal completion
(`sophia_live_session_control schema=1 status=control_quiesced …`), the
completion summary counts the retirements
(`status=quiesced before_dispatch=… in_flight=…`), and both settlement
predicates account for them, so a session that quiesced with controls
outstanding still reads settled and drained. The failure code
`control_quiescing` is on the diagnostics list.

Proof: `tests/session_control.rs`
(`quiescence_retires_pending_and_later_controls_without_failing_the_session`):
one focus control in flight and one waiting when quiescence begins, one
enqueued after it, a late acknowledgement for the in-flight one; every
retirement reads `Quiescing`, nothing more is dispatched, no timeout and no
unexpected acknowledgement are counted, and the metrics settle and drain.
Before the change the queue had no such state: the in-flight control would
have timed out and the owner loop returned the timeout as its runtime
fatal, which is what the gate's log shows. The race itself is not
reproduced deterministically; the `xtest-selection` gate remains the field
check.

## Required repair and proof (as filed)

- Once quiescence has begun for a successful primary exit, a control that
  finds no frontend to answer it is not a fatal: refuse it as
  `quiescing` (a new, classified refusal), do not raise the runtime fatal,
  and let the session end with the exit status the primary earned.
- Do not issue focus controls from focus transitions that the drain itself
  causes; if that is simpler, the policy stops emitting once quiescence
  begins.
- Proof: a headless session fixture whose primary exits while a second
  client holds a focusable window, asserting `sophia_session_result
  status=succeeded` and no `runtime_fatal`; the negative control is
  today's owner loop.

## Connections

- [One hard stall of the rendered-scanout export worker ends the session](h833kgfy-one-hard-stall-of-the-rendered-scanout-export-worker-ends-the-session.md) --
  t186, the other fatal-by-policy on the way out.
- [A Super+button on a window between two committed layouts ends the session](qvj77ywn-a-super-button-on-a-window-between-two-committed-layouts-ends-the-session.md) --
  the earlier runtime-fatal review.
