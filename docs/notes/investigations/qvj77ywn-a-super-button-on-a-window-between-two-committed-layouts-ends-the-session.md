---
id: qvj77ywn
date: 2026-09-23
kind: investigation
status: resolved
tags: [investigation, wm, input, session-fatal]
---
# A Super+button on a window between two committed layouts ends the session

## Question

The operator installed release `bf43425d` to run the selection driver, and
the desktop died about forty seconds after login, before the driver ran.
What ended it?

## Evidence

Session `00000001790160649943-485d72fa-…` (copied to
`.artifacts/t161-super-click-fatal/`): `profile=direct`,
`release_commit=bf43425d…`, Hagia as the WM. Two xterm-sized columns had just
mapped (surfaces 6291468 and 8388620), the scrolling layout was moving the
older column 2097166 off to the left, and a launcher window (client 2,
1×1, `PolicyManaged mapped=false`) had been created and failed
(`bemenu_native status=failed stage=service code=4`). Then, within 170 ms:

```
sophia_live_session_input_routing … pointer_button_count=1 pointer_routed_count=0
sophia_live_session_pointer schema=2 status=button_observed count=1
sophia_live_wm_pointer schema=2 surface=6291468        (an admitted gesture, reduced)
sophia_live_resize_epoch schema=2 status=held transaction=15 surfaces=1
…
sophia_live_wm schema=1 transaction=15 surfaces=2
sophia_live_wm_snapshot schema=1 status=complete surfaces=2
sophia_live_session_runtime_fatal schema=1 status=detected source=owner_loop action=bounded_cleanup failure_code=unclassified
```

The Hagia session log carries the error the owner loop raised:

```
Error: RetirementFailure { message: "pointer interaction target is absent from public-policy state", … }
```

XTEST shows only its admission record; the driver had not run. The lifecycle
log then hands off to the display manager (`exit_status=1 emergency=false
handoff=display_manager`), which is the whole desktop going away.

## Finding and resolution

The button was a floating pointer gesture: Super held plus a button over
surface 6291468, Hagia's `pointer-bind Super+left policy:move`. The physical
phase hands it to `LiveWmSession::enqueue_pointer_interaction`, which looks
up the output whose committed projection places the surface. Surface 6291468
was policy-managed but had no placement in `reducer.committed()` at that
instant: transaction 15 was mid-flight, holding a resize of one surface,
and the committed layout went from three surfaces to two. The lookup miss
was raised with `?`, and the owner loop treats any error from its loop body
as fatal (`physical_input_phase.rs`, `sophia_live_session_runtime_fatal`),
so the session was torn down cleanly and the login ended.

Nothing in this path changed between the operator's previous release
(`20260918-d444eba2`) and `bf43425d`; the check has been there since
`3cb22ef7` (2026-08-08). What changed is that the operator pressed
Super+button on a freshly tiled column at the wrong moment.

A gesture on a surface that policy is not placing right now has nowhere to
go. That is an ordinary state -- a surface between mapping and its first
committed layout, a column a scrolling layout holds out of view, a surface
between the transaction that removed it and the one that returns it -- and
it is not the session's failure. `enqueue_pointer_interaction` now drops the
gesture and records it, the same disposition as a gesture on a surface
policy does not manage:

```
sophia_live_wm_pointer schema=2 status=interaction_dropped reason=target_unplaced phase=Begin mode=Move surface=N
```

The other `?` on the same path, a gesture that starts outside every
public-policy output, is dropped the same way (`reason=outside_outputs`).
The placement lookup is `committed_output_placing` in `wm/commit.rs`, pinned
by `a_pointer_gesture_on_an_unplaced_surface_goes_nowhere_rather_than_failing`.

`sophia_live_wm_pointer` had no reduction allowlist, so retained evidence
kept only its schema and surface -- which is why the admitted gesture in the
log above shows as a bare `surface=` line. `diagnostics/wm_pointer.rs` now
keeps status, reason, phase, mode and surface.

## Validation and remaining work

- [x] The lookup and its reduction are pinned by a unit test. The
      end-to-end red is the retained log above; an owner-loop-level test
      would need a live WM process.
- [x] Both misses drop rather than fail; no other `?` remains on the
      gesture path in `enqueue_pointer_interaction`.
- [ ] Operator: the next installed release carries this. Super+button on a
      column immediately after opening a window should now do nothing, or
      start a move, and never end the session.
- [ ] Why the committed projection lacked a placed, visible column for
      those milliseconds is Hagia's layout transaction shape under a resize
      hold, and is worth a look on its own if the drop shows up in logs
      often; it is not this row's question.

## Connections

- [PRIMARY selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  the install that surfaced this was for its operator step.
- [A shell component retries without recording why it failed](ehar321u-a-shell-component-retries-without-recording-why-it-failed.md) --
  t117 closed on the same principle: a component or gesture that cannot
  proceed is not a reason to withhold the desktop.
