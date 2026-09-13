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
| Already-queued raw work | See below. A send that already returned `Ok` cannot be un-answered |
| Receipts | Raw ingress does not *guarantee* a completion contract, but it does not preclude one: `XAuthorityClientInputEvent` carries `delivery: Option<XAuthorityInputDeliveryId>` (`routing_types.rs:76`), and `route_pending` already answers a real `Some(id)` with `RouteRejected`. Preserve a genuine previously admitted receipt; never invent one for `None`, and never treat a caller-supplied id as authority |

A mode-aware facade handles new attempts. It does not, and cannot, handle
sends that already returned `Ok`: that caller has its success and has moved
on, and there is no retained per-message contract to answer through. Demanding
an observable producer outcome for already-queued raw work would require
inventing one, and a fabricated receipt is worse than an honest refusal to
start.

So the answer is construction, not compensation:

- **Preferred.** A private instance exposes no raw ingress before its gate
  exists. There is no window in which a handle can be taken, so there is
  nothing to reconcile later.
- **Fail closed otherwise.** If a gate is being installed on an instance that
  has already exposed raw ingress or has raw work queued, the installer
  refuses with `ActivationRefused`. The instance stays ordinary and private
  injection is not enabled. Refusing to become private is always available;
  un-answering a send is not.

An asynchronous private operation that is genuinely accepted must reserve its
real completion before it returns accepted. Accepted-then-refused is only
expressible when something was retained to express it through.

The pre-obtained handle problem is the shape of the gate escape already fixed
on this branch, where a sender taken before installation kept its own answer,
solved with a cell every handle shares. That fixes future attempts. It does
not reach a send that has already returned, which is why construction order
carries the weight here rather than the cell.

What has to be tested: a sender obtained before install, work queued before
install -- proving either construction or activation refusal, not a retroactive
error -- authorised work continuing afterwards, a refusal that leaves XKB and
query state untouched, and no producer left waiting on an answer that will
never come.

## Private construction and the production entry

### Construction

A private instance is built in one order, and the order is the safety
property.

1. Build the common authority and take an issuer for it.
2. Derive the coordinator from that authority under that issuer. It reads the
   published revision and refuses mid-transition, so it cannot start from a
   revision nobody committed.
3. Build the gate from the coordinator. It captures the authority identity and
   the coordinator incarnation here, and both are fixed for its life.
4. Build the broker **with** the gate, not by installing one afterwards. Every
   handle the broker hands out is therefore issued by a broker that already
   has its gate.
5. Do not expose raw ingress at all. The private constructor offers no
   equivalent of `input_sender`.

`under_control_gate` remains for the ordinary-to-private path, and that path
is where `ActivationRefused` lives: it refuses rather than enabling injection
on an instance whose handles or queue predate the gate.

### Production entry

The entry is `route_pending`, and this is the part a helper cannot substitute
for. Under a gate it stops being a drain of five independent loops and becomes
one ordered pass:

- Operations from every class in the tables above become runnable in a single
  sequence, rather than each loop advancing at its own rate.
- For each runnable operation: final validation and application happen
  together under the common guard, with the X guards taken beneath it in the
  ranked order, and the executor owns the XKB state so no request and reply
  crosses that boundary.
- Emission -- routing to client queues, receipts, acknowledgements -- happens
  after every guard is released.
- Retained completions are bounded, and anything already accepted into a
  tracked lifecycle gets its exact completion rather than being dropped.

The ordinary path keeps today's behaviour where no gate is installed. Nothing
is enabled by the existence of the executor; a private instance has to be
constructed as one.

## Where control completion actually attaches

Written down because the obvious reading of this seam is wrong in four places,
and each was found by looking rather than by reasoning from the shape.

**There are two producer paths, not one.** `registry/delivery.rs:482` builds
`X11RoutedControl::Authority` and queues it, but `route_control` returns early
at `:475` when `route_focus_control` handles the command, so `FocusSurface` and
`ClearFocus` go through `routing/focus.rs:159 route_authority_control` and
never reach `:482` at all. A registration attached there would cover some
control and silently miss focus.

So the record is created before acceptance, at the producer facade, and both
routes carry the same one. That is also the only point where it can be bound
to the admission that accepted it.

**A successful `send_ack` does not mean the acknowledgement was published.**
All eleven writer callsites funnel through `routing/input.rs:354`, which reads

```rust
Ok(()) | Err(TrySendError::Disconnected(_)) => Ok(())
```

so a gone receiver returns `Ok`. Treating every `Ok` as publication would mark
work complete whose acknowledgement nobody received. `Full` is different again:
it returns an error, and the exact acknowledgement and its completion
responsibility have to be retained rather than the command replayed, because
the command's effect has already happened.

Effect application and acknowledgement publication are therefore separate
events, and a completion hook belongs at the publication, not at function
entry.

**The stale-control wrapper is not on the private path.** The broker's
`XServerFrontendControlRouter::route_control` maps `UnknownClient` and
`Disconnected` to `acknowledge_stale_control`, but the private host calls
`registry.route_control` directly, so instrumenting that helper would be
instrumenting something the private path never reaches. The private error and
cancellation path needs joining with an owned continuation instead.

**Client plus public transaction is not an identity.** `send_ack` sees only
those two, and mapping on them aliases requests that share a transaction. The
registration has to be an opaque server-issued token carried with the command,
bound to origin and admission plus an operation incarnation, and to a grant
generation where one applies -- without requiring a live synthetic grant, since
privileged issuer cleanup must outlive grants.

### The cancellation edges, as audited

Two acknowledgement publishers are a useful start and not the map. These are
the other ways a control ends without one, from a source audit.

| Edge | What happens |
| --- | --- |
| Writer stop (`writers.rs:249`, exiting at `:731`) | Channels and queued commands are dropped. A disconnected receive drains buffered work first, so this is not buffered loss, but the exit itself settles nothing. `dispatch.rs:2443` sets stop explicitly |
| A `?` after dequeue and before the ack | `write_x11_control_records` at `:720` returns before `send_ack` at `:721`; surface, metadata and runtime lock failures do the same. `focus.rs:336-354` adds output lock, write and flush failures |
| `terminate_client` (`:225-245`) | Acknowledges its own request and returns. Controls accepted later and still queued are owned by nobody |
| Registration `Drop` (`delivery.rs:602-648`) | Cleans input recovery and frozen input. There is no control completion registry for it to clean, so one needs an origin-bound teardown sweep, including an early registration `Drop` |
| Worker join (`dispatch.rs:2435-2456`) | The input writer is joined first with `??`, so an error or panic returns before `control.stop` and its join ever run. `X11ControlWriter` holds a stop flag and a `JoinHandle` and has no `Drop` cleanup |
| `register_client` failing (`dispatch.rs:567`) | Can fail after writers are spawned and before the cleanup closure exists |

Two consequences for the record's shape.

It has to carry an execution **phase**, not just an identity, and what is
retained differs by phase. "Retain the outcome rather than the work" is too
simple and would lose the case that matters most.

| Phase | What is retained | Why |
| --- | --- | --- |
| Accepted, not executed | The owned command and its registration | It can still be executed, or cancelled. There is no outcome to keep yet |
| Outcome known | That exact acknowledgement, for publication | The effect has happened. Publishing again is right; replaying the effect is not |
| Partly applied, outcome not established | The original operation identity, its phase, and cleanup responsibility | The runtime may already have mutated -- `AdmitSurface` at `:374` applies before later selection handling -- so it is neither unexecuted nor answered. It stays this way until reconciliation or cancellation establishes the truth |

The third row is the one to be careful about: inventing an outcome to fit the
type would be fabricating a receipt, which is the thing every other rule here
exists to prevent. Cleanup for it stays issuer-privileged and outlives grants.

And private construction has to own every worker it started, and their
accepted controls, across all of these exits, including the ones where another
worker failed first. Without widening ordinary-mode behaviour to get it.

## What still needs stating

Pure getters need coherent snapshots and request ordering. They do not need
mutation operations invented for them, and inventing some would be a worse
error than leaving them unlisted.

## What landed

The completion record now exists in production, default disabled: a private
instance builds one, installs it on its route registry, and every client writer
of that instance reports outcomes to it. The public path has none.

| Edge | What answers it now |
| --- | --- |
| Producer reserves | `PrivateControlProducer::submit` refuses outright for a client whose control writer has stopped, then reserves a record. A reservation is not acceptance: it is still the producer's, nothing may answer for it, and no cancellation edge may take it |
| The instance accepts | The queue entry and the phase handoff are one transaction, prepared inside the admitting hold. Published together or rolled back together, so a refused command has exactly one owner: the caller it was handed back to |
| Routing, which is the first authoritative effect | Execution is claimed there, not at the writer. Focus routing sends FocusOut to the previously focused client and moves the focused surface before any writer runs, so a claim taken at the writer leaves those effects behind a record still calling the operation unexecuted. The claim is fallible and atomic: it and cancellation contend for one lock, so an operation is either claimed and never reported unexecuted, or cancelled and never applied |
| Writer continues execution | The writer resumes a claim it did not make, and a refusal stops it before anything that answers for the operation -- including the unknown-surface acknowledgement, because an acknowledgement is an outcome. A refused claim leaves the owner that refused it holding the operation, so nothing is dropped by declining |
| Writer publishes | The record authorises the acknowledgement and the send happens under the same hold, so one the record refuses never reaches the receiver. Delivered retires the record; a full channel retains the exact acknowledgement and never the command; a gone receiver publishes nothing and so closes nothing, though the writer is still not failed for it |
| Writer stops, however | A guard records it on every exit including an unwind, against the client's own route state. A registration outlives its writer -- returning on a full acknowledgement channel is exactly that -- so the state lives with the route senders, bounded by the clients that exist and gone when the registration goes |
| Instance closes | Commands still in the queue are answered or handed back by the existing settlement, giving up their records as they go. Commands a writer took and never ran are carried out in `pending` with their records given up. Commands caught mid-application stay in the registry, which the returned settlement still reaches through its origin |
| Retry | `publish_owed_with` republishes in place. Nothing is handed out that a caller could drop, and a failed retry keeps the outcome |

Credit release follows the record rather than the send: a control credit is
released exactly when its record retires, and an unreadable registry releases
nothing. That closes the earlier note on `reclaim_settled` that control was not
observable from the frontend.

Ownership transfer is kept distinct from termination throughout. When a command
that never claimed execution is handed to another owner at close, its identity
leaves the outstanding ledger in the same step, so one owner holds it and one
credit is released for it.

Every registration is also bound to the operation it was made for. An
acknowledgement must name what was registered, the first established outcome
is not replaced by a later contradiction, repeating it changes nothing, and
neither part of an identity is ever reused: an exhausted counter refuses to
register rather than issue a value twice, and a registry that cannot take an
unused origin is not built. Three of the rows above are now owned. A client connection's writers are held
together, so a setup failure after any spawn shuts down whatever had already
started, and teardown stops every writer before joining any and joins every one
whatever an earlier one reported -- returning on the first failure left the
rest running, never told to stop, against a closing stream. A control writer's own exit
reconciles what it was applying: it is the executor, so it does not have to
guess whether one is still there. An established outcome is untouched, a
command that never started stays truthfully unexecuted, and one caught
mid-application becomes abandoned. Losing a client's route registration
reconciles too, but only where it can establish that nothing is still serving
that client -- a registration ending is not proof that its writer stopped, and
abandoning an operation whose writer is still inside it would refuse the real
outcome that writer is about to establish.

Stopping every writer before joining any is necessary and is not sufficient. A
writer parked on a condition only another thread can clear observes no stop
flag, and the join waits for it forever. The waits that can be waited on now
observe it and give up without writing, so a join is bounded by the stop rather
than by whether anyone happens to rescue it. A stalled socket write is a
different case and is not claimed here.

A claim is refused for a client nothing is serving, at the routing site as well
as at the producer. Those are separate moments, and a record left claimable
after its client was swept would take a claim and start producing effects for a
client with no executor.

Abandoned is a state, not a completion. Nothing is published for it, nothing is
replayed, it cannot be resumed, and its credit stays held.

What is built for it is bookkeeping and nothing more. `cleanups_owed` and
`record_cleanup` have no production owner: nothing performs or proves any
operation's native cleanup, and a record is retired only because a caller said
cleanup was done. Calling this Applying reconciliation would claim a step that
does not exist. The reconciler that owns it, what each operation's obligations
actually are, and retirement only on established proof, are the work this
leaves open. A cleanup reported as not done keeps the record and returns the
responsibility to the caller, and every lookup that cannot read the registry
says so rather than reporting nothing owed.

Not done here: the worker-join row is owned but the route registration at
`dispatch.rs:468` and `state.register_client` at `:571` remain different
registrations, so a failure between them is owned only for the writers; the durable owner observes carried control only
through the origin it kept; records retained as applying are counted and
reachable but not yet reconciled, so sealing is not completion; and the
process-global origin counter's exhaustion refusal is unreachable from any
test that does not add a setter to production source, so it is argued rather
than demonstrated.

## Status

Source-confirmed inventory, and known to be incomplete: an independent audit
found the four families above after the first draft, which listed only the
broker drain loops and the connection-side grab writers. No implementation is proposed here for live mode
or any ambient fallback, and nothing here is an approval to enable one.

Related: [[0t8n7mwl]] for the writer-side target finding this ordering has to
subsume. Its modifier finding is withdrawn: that atomic is per connection, not
per seat.
