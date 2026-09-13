---
id: w8vt0ueb
date: 2026-09-12
kind: investigation
status: investigating
tags: [investigation]
---
# Production entry and ownership for ordered synthetic input execution

## Question

A single private executor has to order every operation that mutates seat or
grab state, or the paths left outside it reorder against the ones inside. What
are those operations, what mutates today, and where does each one join the
ordered transaction?

## Why the obvious XKB split does not work

The tempting shape is to map the key outside the guards and apply the result
inside. It is unsafe, and not marginally.

`XkbKeyboardState::map_evdev_key` (`keyboard.rs:203`) takes `&mut self` and
calls `xkb::State::update_key` at `:208`. That mutates depressed, latched,
locked and layout state at map time. There is no pure result to carry: by the
time a refusal could be reached under the common guard, the seat has already
moved, and nothing in this design can put it back. A speculative clone and
commit protocol would be a different design and none is established.

One detail shapes the ordering. `map_evdev_key` captures `self.modifier_mask()`
*before* the update and returns it, so its own result is the **pre-event**
modifiers. The after-state is not missing from the existing reply, though: the
`XkbWorkerCommand::Key` wrapper at `registry.rs:212-214` reads
`state.modifier_mask()` again after the mapping and returns
`(keycode, before, after)`. Both semantics exist today and the private
executor has to preserve both, published in execution order rather than
whenever a writer happens to finish.

The direction taken instead: keymap and seat state are compiled and
initialised outside the execution guards, the private ordered executor **owns**
that state, and final validation, the aggregate ledger transition, the XKB
update and the guarded routing application all happen on the executing thread.
No synchronous request and reply to the worker inside the transaction. A
duplicate or a join must not call `update_key` again -- the ledger says whether
this press began the hold, and only a press that did should move seat state.

The ordinary ungated worker is unchanged in this tranche.

## The operations

Everything below mutates seat, grab or routing state today. `producer` is what
originates it; `current mutation` is what it does now; `sequence point` is
where it joins the ordered transaction.

### Broker drain loops (`routing/broker.rs::route_pending`)

| Producer | Current mutation | Sequence point | Stamp / identity | Guard | Completion | Teardown |
| --- | --- | --- | --- | --- | --- | --- |
| `routed_input_receiver` (Deliver) | `route_engine_input_admitted` routes to the client queue | Ordered execution, after ledger transition | Full `ControlStamp` carried from enqueue, never restamped | common, then X beneath | `RequestCompletion` plus a separate transport receipt | `XAuthorityInputDeliveryOutcome` |
| `routed_input_receiver` (Repeat) | Same path, repeat mode | Same, ordered against key and pointer | Same stamp | Same | Same | Same |
| `routed_input_receiver` (StateOnly) | `xkb_worker.request` then `observe_query_modifiers` (`delivery.rs:150`) | **Joins final validation and ordering**, not deferred | Same stamp | Executor owns XKB state; no request-reply inside | Same | Same |
| `input_receiver` (raw) | `observe_direct_query_input` then `route_input`, bypassing the epoch envelope | Private mode: **refused** unless it enters proven issuer-owned private ingress carrying an original stamp | No stamp-on-consume, and the channel is not authorisation | n/a when refused | Refusal is explicit | n/a |
| `control_receiver` | `route_control`, `acknowledge_stale_control` | Ordered with the rest | Control identity | common | Control ack | Stale ack |
| `route_lease_release_receiver` | `release_route_lease` | Privileged cleanup, ordered but **not** a synthetic reservation | Lease identity | common | n/a | Outlives grants |
| `drain_thawed_input` | Re-routes the frozen queue | Ordered; validated again against the stamp each item carries | Original stamp, revalidated per thaw | common, then X | Same as Deliver | `EpochRevoked` |

### Connection-side writers (`dispatch/core/grabs.rs`, `dispatch/extensions/xi.rs`)

These are not in any drain loop and are the ones most easily forgotten.

| Producer | Current mutation | Sequence point |
| --- | --- | --- |
| `grab_pointer`, `grab_keyboard`, `grab_button`, `grab_key` | Install active or passive grabs | Ordered: they change who a later press resolves to |
| `ungrab_pointer`, `ungrab_keyboard`, `ungrab_button`, `ungrab_key` | Remove them | Ordered, same reason |
| `grab_server`, `ungrab_server` | Set or clear `server_owner` | Ordered as **request scheduling**, never recipient selection |
| `allow_events` | Freeze and thaw | Ordered: it gates whether anything is deliverable at all |
| `select_xi_events` | XI subscription state | Ordered with the rest |
| `cleanup_owner` (client teardown) | Drops that client's grabs and query state | Privileged cleanup, outlives grants |

## Rules this table has to respect

Lease retirement and client teardown are privileged cleanup that outlive
grants. They are not synthetic input reservations and must not be made to
depend on one, for the same reason the control transaction is separate from
the execution transaction: a transition revokes the grant that a cleanup would
otherwise need.

Raw ingress is refused in private mode rather than stamped on consume.
Inferring authorisation from which channel something arrived on is the same
mistake as inferring it from a packet field.

No waits and no socket writes under the common or X guards. Application is
explicit and separate from writer or transport completion: a ledger transition
is not a delivery.

Retained completions are bounded, and the agreed external private-instance
watchdog stands outside this.

## Status

Source-confirmed inventory. No implementation is proposed here for live mode
or any ambient fallback, and nothing here is an approval to enable one.

Related: [[0t8n7mwl]] for the writer-side target finding this ordering has to
subsume. Its modifier finding is withdrawn: that atomic is per connection, not
per seat.
