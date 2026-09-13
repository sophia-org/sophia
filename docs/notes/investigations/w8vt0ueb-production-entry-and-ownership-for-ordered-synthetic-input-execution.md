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

### Control is queued in one place and applied in another

Sequenced enqueue is not an ordered state change, and this is the family the
first draft got most wrong. `registry/delivery.rs:468-485` only queues the
control. The mutation happens later, in the connection's own writer:
`connection/writers.rs:602` calls `runtime.set_input_focus`, `:617` swaps that
connection's `focused_surface_window`, and `:682` unmaps on
`WithdrawSurface`. Ordering the queue says nothing about when any of that
lands.

Direct core `SetInputFocus` does not pass through the control queue at all
(`dispatch/core/input_discovery.rs:44-48`, reached from
`connection/dispatch.rs:1544`).

| Producer | Current mutation | Sequence point |
| --- | --- | --- |
| `control_receiver` then connection writer | `set_input_focus`, `focused_surface_window` swap, withdraw unmap | The **application**, not the enqueue |
| Core `SetInputFocus` | Same runtime focus, no queue involved | Same transaction as the above |

### Route-relevant window state

Event masks and do-not-propagate, map and unmap, hierarchy and stacking all
change who a final recipient is (`connection/dispatch.rs:1550-1610`, consumed
in `writers/input.rs:72-91` and `:134-153`). Reparent and configure move the
anchors a pointer query resolves against (`runtime/windows.rs:296`, `:458`,
into `runtime/pointer_query.rs:14-30`). Destroy clears query window and focus
(`runtime/windows.rs:534`, `:549-550`).

This does **not** mean running every window or property operation inside the
input executor. The seam is an explicit invalidation and publication
transaction with immutable recipient resolution: route-relevant state
publishes coherently under the ranked boundary, and a writer never selects
from state that has changed since the resolution it is acting on.

Clipboard and property work has no reason to route through the executor.

### Cleanup has more than one source

`cleanup_owner` is the orderly one. It is not the only one.

| Source | What it drops |
| --- | --- |
| `XServerFrontendClientRouteRegistration::drop` (`registry/delivery.rs:602-647`) | Client, surfaces, focus, subscriptions, frozen state |
| Input recovery (`routing/recovery.rs:377-380`) | Calls `cleanup_owner` independently |
| Pointer grab activation rollback (`connection/dispatch.rs:2176-2207`) | Undoes a partially activated grab |

Each needs either joined cleanup or an explicit private exclusion, with
receipts retained either way. A cleanup that runs outside the ordering is a
teardown that can reorder against the presses it is tearing down.

### Repeat has producers outside this host

The repeat row above covers delivery only. Ownership, arming and cancellation
live in Session: `live_session/input.rs:1064` and `:1127`,
`client_keys.rs:76` and `:79`, `session_control.rs:199-200`. Which of these are
absent from a private host, and which bridge into the ordering, has to be
stated rather than assumed.

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

## Raw ingress: the API change this needs

`route_pending` consumes `input_receiver` and routes it without any stamp. In
private mode that work is refused. The refusal has to reach the producer, and
today it cannot.

`XServerFrontendRouteBroker::input_sender` (`broker.rs:505`) hands out a bare
`SyncSender<XAuthorityClientInputEvent>`. A successful `send` means the queue
accepted bytes, nothing more; a consumer that later declines has no way back
to the caller. Treating send success as admission would be inferring
authorisation from the channel, which is the thing being refused.

So the change is a facade, not a check at the consumer:

| Concern | What it has to do |
| --- | --- |
| Refusal point | At `send`, on the producer's thread, before acceptance |
| Refusal type | Typed and specific -- denied because private, or denied because unstamped -- carrying the rejected payload back |
| Not | A silent drop; a service-fatal `route_pending` error; or a denial dressed as queue saturation or recipient failure |
| Producer | Fails its own operation immediately, while the authority keeps serving healthy authorised work |
| Pre-obtained handles | Must observe activation too, or construction must prove none escaped |
| Already-queued raw work | Drained and refused on activation, with an observable producer outcome |
| Receipts | Raw events have no synthetic request cell. Do not invent one, and do not fabricate a delivery receipt. Work already inside a tracked lifecycle gets its exact negative completion, issued after locks drop |

The pre-obtained handle problem is the same shape as the gate escape already
fixed on this branch: a sender taken before installation kept its own answer.
That was solved by putting the decision in a cell the broker and every handle
already share, set once. The same shape applies here, with the difference that
this one must also carry a refusal back, which a `SyncSender` cannot.

What has to be tested: a sender obtained before install, work queued before
install, authorised work continuing afterwards, a refusal that leaves XKB and
query state untouched, and no producer left without an answer.

## What still needs stating

Pure getters need coherent snapshots and request ordering. They do not need
mutation operations invented for them, and inventing some would be a worse
error than leaving them unlisted.

The concrete private construction and production call path is not in this note
yet. It is the next thing owed, and it should name the actual entry rather
than another helper nothing calls.

## Status

Source-confirmed inventory, and known to be incomplete: an independent audit
found the four families above after the first draft, which listed only the
broker drain loops and the connection-side grab writers. No implementation is proposed here for live mode
or any ambient fallback, and nothing here is an approval to enable one.

Related: [[0t8n7mwl]] for the writer-side target finding this ordering has to
subsume. Its modifier finding is withdrawn: that atomic is per connection, not
per seat.
