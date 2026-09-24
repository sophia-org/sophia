---
id: y5r35kwi
date: 2026-09-24
kind: investigation
status: investigating
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

## Required repair and proof

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
