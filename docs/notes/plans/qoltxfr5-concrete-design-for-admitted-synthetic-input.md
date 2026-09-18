---
id: qoltxfr5
date: 2026-09-12
kind: plan
tags: [plan, milestone]
---
# Concrete design for admitted synthetic input

Status: **historical design approved** at `1bc946d5`. Its ownership,
synchronization, shared-seat admission and version choices are superseded by
[the authorized private-instance implementation](7xqjn8rp-private-native-input-authority-and-xtest-adapter.md).
Live-seat activation and deployment remain unauthorized.

Authority, principles and the operator's chosen constraints live in
[htm85gg0](../decisions/htm85gg0-admission-ingress-and-provenance-for-synthetic-input.md).
This note says how to satisfy them and does not restate their normative text.

## Owning components and state

| Concern | Owner | State |
| --- | --- | --- |
| Grant issue and revoke | Session | grants keyed by `ClientAdmissionId` |
| Admission predicates | Session, reusing Control v1's | pinned peer, credentials, namespaces |
| Decode, refusal, imperviousness | `sophia-x-authority` connection | per-connection grant handle |
| Delayed request | `sophia-x-authority` connection | one `PendingSynthetic` |
| Contribution ledger | Engine, beside `KeyRepeat` | see below |
| Routing, focus, grabs | Engine | unchanged |

### Contribution ledger

Per `(seat, input)` where input is a keycode or a button:

```
sources:   Set<SourceId>              // physical device ids and grant ids
recipient: Option<DeliveredTo>        // surface, lease, epoch, device
```

`DeliveredTo` is recorded **at delivery of the first press**, not recomputed at
release. That is what makes a last-holder release targetable after focus has
moved, which the repeat guard cannot do.

## Contribution state machine

| From | Event | To | Delivery |
| --- | --- | --- | --- |
| empty | press(S) | {S} | **deliver press**, record `DeliveredTo` |
| {..S..} | press(S) | unchanged | none; duplicate is not a second hold |
| nonempty, S absent | press(S) | +S | none; already held |
| S absent | release(S) | unchanged | none; refusal recorded |
| size>1 | release(S) | -S | none; survivor keeps the hold |
| {S} | release(S) | empty | **deliver release to recorded `DeliveredTo`** |

Release delivery targets the recorded recipient, not current focus, so a
retirement after focus moved to B still clears A.

**A failed release becomes reconciliation debt, not a log line.** A recipient
whose lease is stale or whose epoch advanced may still be alive and still
believe the input is held; that is a different case from a recipient that is
gone.

**Settlement uses the receipts this server actually emits.** The previous
revision built a table on `RoutedInputOutcome`, which is protocol vocabulary
that nothing in this broker emits -- it appears only in a framing test. The real
receipt is `XAuthorityClientInputDelivery` carrying
`XAuthorityInputDeliveryOutcome` (`routing_types.rs:142`), finished by the
writer's flush path (`connection/writers/input.rs:722`).

Core key and button events carry no application acknowledgement, so nothing
below claims one.

| Outcome for the recorded-target release | Postcondition | Debt |
| --- | --- | --- |
| `Flushed` | the release left the server toward the recorded recipient | **settled at transport only** -- not proof the application processed it |
| `ClientDisconnected` | the recipient itself is gone | settled |
| `TargetGone` | the *surface route* lookup missed | **retained** -- see below |
| `EpochRevoked` | refused before routing | retained |
| `RouteRejected` | refused at the route | retained |
| `WriteFailed` | transport failed | retained |
| `TimedOut` | no outcome within the delivery bound | retained |

**A refusal is not evidence of clearing**, and `TargetGone` is a refusal.
`registry/delivery.rs:104-113` emits it when `surfaces.get(target_surface)`
returns `None`, with client id 0; `registry.rs:371` can remove that route while
the client remains registered and alive, and `writers/input.rs:120-127` emits it
on a failed surface lookup too. A vanished surface route is not a vanished
client, and the recipient actually reached may have been a **grab client rather
than the original surface's owner**. Debt is therefore unresolved until a
separate identity or teardown fact proves the reached recipient cannot still
hold the input: its `ClientDisconnected`, or targeted clearing against the
recorded recipient identity rather than against the surface.

`XAuthorityRouteLeaseRelease` is **not** a receipt and is not in the table. Its
handler (`registry/delivery.rs:27-63`) looks up a client, ungrabs its pointer
and emits a lease update; it is a command and does not establish that key or
button state was released.

The previously proposed queue-consumed acknowledgement is **removed**: the writer
necessarily consumes its queue entry before flushing, so it is weaker than
`Flushed`, not additional evidence. If transport-level settlement proves
insufficient, what is needed is a **server-state reconciliation acknowledgement**
-- a confirmation that the recipient's held-input state was cleared, produced by
the component owning that state -- named here as proposed new work. No X
application acknowledgement is introduced.

### Two debt layers, kept apart

A hold owes two different things, and one receipt does not discharge both:

| Layer | Obligation | Discharged by |
| --- | --- | --- |
| **Server-owned** | shared modifier, grab and ledger state reconciled | the reconciling transition completing under the inner guard |
| **Recipient** | the release reaching the recorded recipient | `Flushed`, or that recipient's `ClientDisconnected` |

A `Flushed` release or a disconnected recipient settles the **recipient**
obligation only. It is not evidence that server-owned state was reconciled, and
the reverse holds too. Server-owned state is reconciled synchronously under the
guard, so it needs no acknowledgement; an acknowledgement becomes necessary only
if that state is later owned asynchronously, and would then be produced by its
owner rather than by an application.

### Debt storage and attempt scheduling are different populations

Revocation must never fail for want of queue space, and a mass revocation across
16 grants can owe more releases than any attempt queue should hold.

- **Debt lives in the ledger, preallocated per hold**, sized by the capacity
  table. Every owed release is represented the moment it is owed, including the
  sixty-fifth and beyond.
- **At most 64 attempt records are scheduled or in flight.** The attempt queue
  is a working set, not the record of what is owed.
- **A persistent sweep admits the rest in fair arrival order** as attempts
  complete, so no debt is starved and none is dropped for lack of a slot.

### Progress, stated conditionally

Four events and 0.5ms per 16ms period are **ceilings on what may be started**,
not a guarantee that four holds settle. Delayed outcomes, retries and the time
cap all defeat a simple division, so the previous `ceil(n/4)` claim is withdrawn.

What can be stated: given **runnable service opportunities actually occurring**,
every owed release is eventually attempted, because admission is fair and no
debt is dropped. Transport settlement is separate and is not bounded by this
scheduler at all.

### Nonresponse is the recipient's, not ours

Measurement starts at the **defined delivery stage** -- the point the release is
admitted to the recipient's writer -- and **excludes time held by Sophia's own
machinery**: the writer's internal waits, control preemption, and frozen input.
Absence of `Flushed` is therefore not by itself recipient nonresponse.
Termination follows proved nonresponse from that stage, or an explicitly
documented unrecoverable condition. **Debt age alone never terminates anything.**

### Stale releases are scoped to their recipient

A global `(seat, input)` identity is wrong in both directions. If recipient A
still owes release H1 and a new press H2 reaches recipient **B**, a global
mismatch would suppress A's release -- but B's new hold is no evidence that A
was cleared. Conversely an old release to the **same** recipient must not follow
a newer press and clear it.

So a delivery incarnation is keyed by **recorded recipient and connection
generation, plus input and hold identity**. Then:

- a stale attempt for a *different* recipient is delivered normally: it is not
  stale for that recipient;
- for the *same* recipient, a new hold on that input is not deliverable until
  the earlier release has been **proved cleared or superseded** -- a clearing
  barrier, not a race;
- discarding a stale attempt never settles it. It resolves to proved clearing,
  proved supersession, or **retained debt**.

**`DeliveredTo` is invalidated when something else already cleared that
delivery.** A global focus transition that released A's input clears the record,
so a later retirement cannot deliver a second release for the same hold.

**Repeat ownership is explicit.** Repeat belongs to the source that started it.
If A started repeat and B remains a holder, retiring A **cancels the repeat**
and does not transfer it to B: B never asked to repeat, and inheriting it would
manufacture input B did not request. The aggregate hold survives; only the
repeat ends.

A passive grab activated in a target client by synthetic input is the target's.
Settlement delivers the release that would ordinarily end it and does not cancel
the grab directly.

## Delayed request lifecycle

A delay cannot be a sleep. `connection/dispatch.rs:575` reads synchronously and
then busy-waits on `server_owner` with a 1ms sleep; a sleeping connection cannot
observe departure, revocation, or a completion it must wait for.

**Readability is not EOF.** `POLLIN` is also set by ordinary unread pipelined
bytes, and with ingress paused those bytes stay unread, so polling `POLLIN`
alone spins forever. The wait is therefore on:

- `POLLRDHUP` and `POLLHUP` and `POLLERR` for peer departure, which is what
  actually distinguishes a gone peer from a talkative one;
- a **pollable** completion and revocation notifier -- an eventfd or pipe read
  end registered in the same `poll` set, not an unpollable channel;
- the deadline, as the `poll` timeout.

`POLLIN` on paused ingress is *not* a wake reason and is masked out. A stream
read returns EOF only after buffered bytes are consumed, so departure is
concluded from the hangup flags rather than from a read this design refuses to
perform.

**Half-close is latched, because `POLLRDHUP` stays set.** Once observed it would
wake the poll forever, so the write-half-close is recorded once and its wake
interest removed while ingress is paused. `POLLHUP`, `POLLERR` and the revocation
notifier remain armed. An independent check confirms the hazard: two consecutive
zero-timeout polls both returned `POLLRDHUP` while `MSG_PEEK` still showed a
buffered request.

**Half-close does not discard what the peer already sent.** A peer may send a
delayed `FakeInput`, then `GetInputFocus`, then `shutdown(SHUT_WR)`. Finishing
only the `FakeInput` and closing would drop a valid request the peer is entitled
to have answered. So after the delayed request is processed, ingress resumes and
parses the buffered requests in order, bounded as ordinarily, until a read
actually returns EOF. Delays encountered among them are honoured, and errors are
reported normally. Full departure -- `POLLHUP` or `POLLERR` -- still cancels
immediately.

**States, owner, and what each waits on.**

| State | Timer | Waits on | Leaves on |
| --- | --- | --- | --- |
| `Ready` | none | socket `POLLIN` | request decoded |
| `Submitted` | none | notifier: accepted | Session accepts, arming the delay |
| `Delayed` | armed by **Session at acceptance**, observed by the connection as a `poll` timeout | hangup flags, notifier, deadline | deadline reached, or cancelled |
| `Executing` | none | notifier: processed | Session reports processed |
| `Retiring` | none | notifier: settled | contributions and debt settled |

The connection owns the `poll`; **Session owns the delay's start**, so the delay
is measured from acceptance rather than from whenever the connection got
scheduled. `Delayed` reaching its deadline transitions to `Executing`; the next
request is read only after `Executing` reports processed. Sequence numbers and
errors stay with the connection thread throughout.

**Server-grab wait services cancellation too.** The busy-wait at `:592` gets the
same treatment: it waits on the notifier and hangup flags alongside grab
ownership, so a client blocked on another client's server grab still observes
revocation and departure. An impervious client does not enter it. A delayed
`FakeInput` whose deadline expires while another client holds a server grab
waits there if ordinary, proceeds if impervious, and remains cancellable in
either case.

**Capacity refusal cancels the request, not the holds.** `BadAlloc` refuses the
one request that could not be admitted and leaves existing contributions intact;
it does not enter `Retiring` and does not revoke the grant. Only disconnect,
revocation and epoch invalidation retire holds. The previous revision routed all
capacity refusals into `Retiring` without saying so, which would have dropped a
caller's existing holds because a later request did not fit.

## The commit boundary, as an extension

Since no single atomic boundary exists, this names one, and the previous
revision's "same step" claim is withdrawn:
`advance_control_epoch` (`routing/broker.rs:94`) is a compare-and-swap on an
atomic counter, and `route_pending` (`:497`) notices and *applies* it later.
Advance and application are two moments, so nothing is atomic by virtue of
calling it.

**Vocabulary, fixed here because the review found it ambiguous.**

| Term | Meaning |
| --- | --- |
| queued | accepted at the connection, stamped with its bound generation and epoch, not yet validated |
| committed | validated under the serializing operation and admitted to routing |
| processed | routing has applied it and Session has produced completion |

A connection's next request waits for **processed**, not queued or committed.

**One guard: the inner input-authority mutex.** `XAuthorityRuntime` holds
`input_authority: Arc<Mutex<XInputAuthorityState>>` (`runtime.rs:244`), locked
on its own by `input_authority_mut()` (`:341`). That is a *different* lock from
`Mutex<XAuthorityRuntime>`, so a producer holding the outer guard excludes
nothing from a consumer holding only the inner one. Grant state therefore lives
under the **inner** guard -- the one the consumer already takes, and where grab
resolution happens.

| Participant | Takes | Purpose |
| --- | --- | --- |
| Connection dispatch | outer, then inner | decode, reserve, enqueue |
| Grab writers | inner | already do |
| Revocation | inner | generation invalidation |
| Consumer execution | inner | validate, apply, resolve grabs |

**Nesting order is outer then inner, never the reverse**, matching existing code
that takes `lock_x11_request_runtime` and then `input_authority_mut()`.

Under that inner guard, as one step: grant generation is read, the **security
epoch is read there too** rather than passed in as a previously loaded value,
the ledger transitions to applied, and grabs are resolved.

### Session publication is part of the ordering, not a version check

Broker X-grab resolution does not establish current Engine focus, and a
minimum-revision check does not either. Grant generation and publication
revision are **different identities**: a grant issued under focus A still
satisfies an issuance-time minimum after Session commits focus B, so execution
could route against A while B's snapshot is still in flight. Lock and
inactive-seat transitions make that worse, not better.

So a **routing publication revision** is defined separately from grant
generation, and the transition -- not the reader -- carries the obligation:

1. A Session focus, seat, or security transition, **under the inner guard**,
   marks synthetic routing **unavailable** and records the revision it is moving
   to.
2. Synthetic execution attempted while unavailable **waits or is refused**. It
   never falls back to the previous snapshot.
3. When the matching snapshot is installed, the transition marks synthetic
   routing available again at the new revision.

Every security-epoch mutation participates in this ordering. Reading an atomic
epoch under the guard is not sufficient if another writer can change it outside
the guard, so the mutations move inside it; the epoch is state, not a hint.

This is fail-closed by construction: the window between commit and publication
refuses injection rather than serving stale focus.

**The commit point is authoritative execution, not enqueue.**
`route_engine_input` (`registry/delivery.rs:65-83`) rejects an epoch mismatch
regardless of when an item was enqueued, and per-client cancellation
deliberately does not advance the seat epoch, so an epoch-only check cannot
catch a single revoked grant. Enqueue produces a pending entry carrying grant
identity, generation and bound epoch; the consumer validates generation as well
as epoch. **The grant-generation check is new broker integration**, not an
existing behaviour reused.

**Four states, because they settle differently.**

| State | Meaning | If revoked here |
| --- | --- | --- |
| queued | reserved, not applied | drop the reservation, nothing owed |
| authority-applied | modifier and grab state changed | **debt owed**, even with no wire flush |
| writer-pending | handed to the writer | debt owed |
| transport-complete | `Flushed` observed | settled at transport |

A press can alter server modifier and grab state at *authority-applied*, before
any writer succeeds, so a missing flush does not mean nothing needs
reconciliation. Debt begins at authority-applied.

**Deferred and frozen entries revalidate when they become executable**, against
the generation, epoch and focus snapshot in force at that moment rather than
when they were deferred.

**Race traces.**

- *Single-grant revoke between enqueue and execution:* generation no longer
  matches; refused under the guard, reservation dropped, no debt.
- *Seat epoch advance:* the existing epoch check rejects with `EpochRevoked`.
- *Revoke after authority application, before writer completion:* the press
  already changed server state, so debt is owed and settles through
  reconciliation rather than being dropped.
- *Peer grab or focus change:* resolved at execution under the guard, recording
  the recipient actually reached.

**Per-client cancellation does not advance the seat epoch.** Disconnect or
single-grant revocation is its own ingress into the serializing operation,
retiring only that client's sources and debt. It must not clear another source's
hold and must not cancel a target's grab. The seat-wide epoch advance remains
for seat-wide events.

**Protected actions, and why the obvious rule was dangerous.** The previous
revision said any synthetic contributor disqualifies a protected action. That
hands an injector a **veto over the operator's recovery path**: an injector
holding Ctrl would disqualify a physically complete Ctrl+Alt+Backspace even
while the operator physically holds every key of it. That is exactly backwards
-- the emergency chord exists for when something has gone wrong, including an
injector misbehaving.

The rule is therefore about sufficiency, not contamination:

- a protected action requires the necessary state to be held **by physical
  sources**, and to be triggered by a **physical** transition;
- synthetic contributions **cannot supply a missing component** of such a chord;
- synthetic contributions **cannot taint** an independently sufficient physical
  chord.

This needs the physical recognizer fed independently of the aggregate. If
synthetic already holds Ctrl and the operator then physically presses Ctrl, the
*aggregate* does not change and the ledger delivers nothing -- but the physical
source transition must still reach the emergency and VT recognizers, or the
overlap silently disables recovery. Physical transitions are reported to those
recognizers whether or not they change the aggregate.

This grants synthetic input nothing new; it prevents it removing something.

**Lock and seat state.** No existing lock path is asserted. What exists is seat
release in `owner_loop/lifecycle.rs` and the session control surface. The
proposed integration is an explicit predicate consulted before commit, reading
seat-active and session-lock state from those, **failing closed** when either is
unavailable or indeterminate rather than permitting.

## Scheduling: provisional values, to be measured

Numbers are starting points to instrument, not capacity claims. They are stated
because the previous revision removed them and left nothing to verify.

- **Insertion point.** Synthetic work is taken where authority batches are
  taken in `owner_loop/authority.rs`, after physical input and after revocation
  and cleanup, which are serviced unconditionally first.
- **Replenishment: 2ms of synthetic service per 16ms interval.** An interval,
  not a turn, because turns are work-driven and not uniform. Unused budget does
  **not** carry over; carry-over would let an idle period fund a burst that
  displaces physical input.
- **Cap: 32 synthetic events per interval**, whichever of cap or budget is
  reached first.
- **Round-robin cursor persists across stops**, so a source that missed an
  interval is served first in the next.
- **Progress expectation: N = 16 intervals, conditional.** Given service
  opportunities actually occurring and runnable work, every ready source is
  serviced within 16 intervals under the 16-slot bound. It is not a bound under
  arbitrary owner-loop stalls, a held server grab, or a non-preemptible
  operation overrunning its budget. That conditional form is what the tests
  assert.

## Capacity accounting

**One slot policy, chosen.** Sixteen grant slots per seat, **total**, counting
grants still carrying unsettled debt. A slot is reusable only once its debt is
settled. The previous revision offered two alternatives; the alternative is
removed, and the formulas below follow from this one.

| Population | Bound |
| --- | --- |
| Grant slots per seat | 16 total, active plus retiring-with-debt |
| Pending requests | 1 per active grant; seat total is *derived*, never more than 16 |
| Ledger entries | ≤ keycodes + buttons per seat, fixed by the input domain |
| Sources per entry | ≤ physical devices + 16, matching the slot total |
| Debt entries | ≤ ledger entries × (devices + 16); storage reserved before a press is accepted |
| Connection buffer, FDs | existing bounded receive and arity caps, unchanged |
| Seats | **this design covers the single exposed seat only** |

Multi-seat is out of scope rather than hand-waved: a Session-wide ceiling would
need a seat inventory this design does not specify, so a second exposed seat
requires a revision.

**Cleanup has a reserved service allowance**, not merely reserved storage: a
fixed share of each service interval is spent on settlement before ordinary
synthetic work, so debt drains even while injectors are saturating the path.
That is what makes the resumable sweep terminate.

### Admission happens before discovery, not at first use

The previous revision issued a grant at the first grant-requiring request while
discovery read an already-issued grant. That is a dead end: an eligible client
sees XTEST absent, and a normal toolkit never sends `GetVersion` or `FakeInput`
to an absent extension. Guessing an opcode is not a discovery protocol.

**Both enumeration paths call one idempotent admission helper before
answering.** `advertised_extension_names()` and `extension_query_result` invoke
`admit_synthetic(connection)`, which issues or returns the connection's grant
and reserves its slot. Calling it twice yields the same grant.

The ordinary flow therefore works without guessing:

    setup -> QueryExtension("XTEST") -> present -> GetVersion -> FakeInput

**Decision order**, evaluated inside that helper, first match deciding:

| Condition | Discovery | Guessed opcode |
| --- | --- | --- |
| Capability disabled | absent | `BadAccess` |
| Caller ineligible | absent | `BadAccess` |
| Grant revoked | absent | `BadAccess` |
| Eligible, no free slot | absent | `BadAlloc` |
| Eligible, slot reserved | **present** | proceeds |

`BadAccess` for the disabled row keeps
[htm85gg0](../decisions/htm85gg0-admission-ingress-and-provenance-for-synthetic-input.md)'s
accepted rule that a guessed opcode meets authorization denial. The previous
revision's `BadRequest` there would have changed an accepted contract: a
disabled-but-implemented entry point is not the same as a build with no
implementation registered, and only the latter is `BadRequest`.

**Retry.** A client refused for **capacity** may call discovery again; the
helper re-evaluates and admits once a slot has settled.

A **revoked** client stays denied. Re-admission requires a **fresh Session
authorization decision** taken after the cause of revocation has cleared;
calling discovery again does not override an outstanding denial, and a new
generation prevents replay without establishing authorization. Clients that are
disabled, ineligible, or on a locked or inactive seat stay denied throughout.

## Discovery and refusal, restored

These were in the first revision and dropped from the second; they belong here.

**One discovery function.** `advertised_extension_names()` and
`extension_query_result` both read the connection's grant through a single
helper, so the two enumeration paths cannot disagree for a given authorization
state -- the property t086 repaired by hand. They agree *for a state*, not
across an intervening revocation: a client that enumerates, is revoked, then
queries correctly sees different answers, and that is not a disagreement.

**Refusal record.** `kind` (absent | unauthorized | revoked | capacity |
protected-action | malformed | unsettled-debt), `client`, `grant generation`,
`security epoch`, `seat`, `request`, `decision`. Intentional denial stays
distinguishable from missing implementation, which the wire alone does not
settle.

## The advertised bundle

**GetVersion** returns major 2, minor 2 -- the version whose semantics this
design implements -- wherever XTEST is advertised. A client asking for more is
answered with what is supported, not refused.

**CompareCursor** takes a window and a cursor. The **window is namespace-checked
in every case**, including `None` and `CurrentCursor`, and an **explicit cursor
argument is itself resolved through the caller's namespace** like any other
resource lookup; the previous revision
checked it only for an explicit cursor, which would have let an unauthorized
window be named so long as the cursor argument was special. `None` compares
against no cursor. `CurrentCursor` reads what the seat displays and is the
bundle's one pure-read disclosure: permitted for the administrator bundle,
refused otherwise.

**GrabControl** is scheduling behaviour. An impervious client progresses while
another client holds a server grab; an ordinary client waits at `:592`; both
remain cancellable there. Imperviousness retires on revocation and disconnect,
returning the client to ordinary waiting.

**FakeInput** distinguishes malformed from unusual:

- **Off-screen motion is valid** and is clipped to the nearest on-screen
  coordinate, per the protocol. The previous revision's blanket "no clamp" was
  wrong: clamping is the specified behaviour here, and only *fields* are
  rejected rather than refused-and-clamped.
- Malformed is an out-of-range event type, a keycode outside the server's
  min/max range, or a button outside the pointer mapping -- these are errors.
- Keycodes convert through the server's keycode range; buttons through the
  current pointer mapping, so a remapped pointer injects what the user's mapping
  means.
- Motion is absolute against the named root by default, relative when requested.
  **Root `None` is valid** and selects the pointer's current screen -- the
  previous revision called it an error. A root that is neither `None` nor an
  existing root is an error.
- **FakeInput produces no wire reply on success.** Its completion is internal:
  it releases the connection to read again, and later requests keep ordinary
  sequencing. The previous revision implied a reply.
- A guessed opcode from an unauthorized caller is refused exactly as an
  authorized-but-denied one, since hiding is not a boundary.

## Admission rechecks

The grant is not only issued and trusted. At execution, under the serializing
lock: the peer is rechecked as current, protected-role exclusion is re-applied
so a protected role cannot acquire a grant through namespace or resource
admission, and lock and seat state are read from their owning components. Any
of those being unavailable or indeterminate **fails closed**.

## Acceptance matrix

Socket, both byte orders: authorized; unauthorized; disabled; revoked during a
delay; reconnect not inheriting pending work; `BadAlloc` at capacity with no
side effect; each protected action denied; discovery agreeing with authorization
*for a given authorization state*; GrabControl progress and wait; FakeInput
field validation and sequence completion; CompareCursor explicit, `None` and
current.

Rust, which the ClassicShared socket host cannot establish:

- **cross-namespace administrative effect is permitted and correct** -- an
  authorized administrator drives the committed focus target in another
  namespace, delivered only to the authoritative recipient, with no broadcast
  and no disclosure beyond it. This replaces the previous case, which promised
  namespace confinement the approved design explicitly rejects;
- unauthorized injector denied regardless of namespace;
- resource and cursor lookups still namespace-restricted;
- ordinary first press and release, before any overlap;
- focus change between press and retirement, release reaching the recorded
  recipient;
- grab change between press and retirement;
- physical + synthetic, synthetic retired, physical hold survives;
- A + B, one retired, the other survives;
- sole holder retired, aggregate and recipient clear;
- failed reconciling delivery holds bounded debt until settlement is
  acknowledged, and never retargets -- replacing the earlier case, which
  accepted a forgotten failure;
- a fully physical emergency chord still triggers while an injector overlaps one
  of its components;
- a chord needing a synthetic component does not trigger;
- a physical press that does not change the aggregate still reaches the physical
  recognizers;
- repeat started by A, with B still holding, cancels on A's retirement rather
  than transferring to B;
- `BadAlloc` at capacity leaves the caller's existing holds intact;
- write-half-close with pending work completes rather than aborting;
- paused ingress with unread pipelined bytes does not spin;
- grant/revoke cycled repeatedly does not accumulate grants or debt past the
  bound;
- revocation landing on a delayed request at the boundary;
- cleanup of many held inputs while the ordinary path is saturated;
- duplicate press and stale release;
- EOF observed during a delay behind unread pipelined bytes;
- half-close with a pipelined request behind the delayed one: the buffered
  request is parsed and answered after processing, not discarded;
- half-close does not spin: repeated polls do not rewake on a latched
  `POLLRDHUP`;
- enqueue, then single-grant revoke, then consume: refused on generation, no
  debt, because no press was applied;
- enqueue, then seat-epoch advance, then consume: refused on epoch;
- enqueue, then peer grab or focus change, then consume: routed against state at
  execution, recording the recipient it actually reached;
- a refusal outcome (`TargetGone`, `RouteRejected`, `WriteFailed`, `TimedOut`)
  retains debt rather than settling it;
- a surface route removed while its client stays alive: `TargetGone` does not
  settle, and clearing targets the recorded recipient identity;
- revocation after authority application but before writer completion: debt is
  owed, not dropped;
- revocation on either side of the final guard;
- more than 64 simultaneous debts: every one stays represented in the ledger
  while at most 64 attempts are in flight, and all settle;
- more than 32 held inputs with a healthy draining recipient: all settle, and
  the recipient is **not** terminated for our backlog;
- A owes H1 while a new press H2 reaches B: A's release is still delivered,
  because B's hold says nothing about A;
- same recipient, H1 owed and H2 pressed, with an old writer attempt delayed
  across the new press: the barrier holds H2 until H1 is proved cleared or
  superseded, and the stale attempt does not clear H2;
- a Session focus or seat transition paused after commit and before publication:
  injection waits or is refused, never routed against the old snapshot;
- a genuinely stalled recipient, timed from admitted delivery, does terminate;
- a stale release is suppressed on the wire, not merely in bookkeeping, so a
  newer hold survives it;
- a revoked client calling discovery again stays denied until a fresh Session
  authorization decision;
- `Flushed` settles transport only, and is not recorded as application
  processing;
- a recipient proved nonresponsive from the delivery stage terminates; **debt
  age alone never terminates**, and time held by Sophia's writer, control
  preemption or frozen input is excluded from the measurement;
- ordinary discovery flow with no guessed opcode: setup, QueryExtension,
  GetVersion, FakeInput;
- disabled capability answers `BadAccess` to a guessed opcode, not
  `BadRequest`;
- a revoked client re-admitting receives a new generation and replays nothing;
- a late debt completion carrying an old hold identity does not erase a newer
  hold on the same input;
- an unresponsive recipient escalates to termination within its bound rather
  than holding debt open;
- a slot is not reusable until its debt settles, and becomes reusable after;
- the ordered decision table: disabled, ineligible, revoked, capacity and
  granted each produce their own answer.

## Out of scope

Virtual seat, selective delegation, configurable permission subsets, and any
route by which synthetic input could close a physical acceptance obligation.

## Design-review approval

Approved 2026-09-12 on Codex review of `1bc946d5`, after eight prior revisions.
The approval covers the **design only**. It authorizes no implementation and no
deployment.

What the review found closed: the publication protocol shuts the stale
focus and seat window, because the transition disables synthetic routing under
the shared guard before installing the matching publication, with grant
generation distinct from publication revision and epoch changes participating in
the same ordering. Cleanup separates preallocated debt from the 64 scheduled or
in-flight attempts, conditional scheduler progress from transport completion,
recipient-scoped hold incarnations from a global input identity, and
server-owned reconciliation from recipient transport. A refusal or a
disappearing route does not silently settle a hold that was already applied, and
physical emergency input stays usable despite synthetic overlap.

**The acceptance matrix is a list of obligations, not evidence obtained.**
Nothing in it has been run. Implementation carries these specific verification
duties, which the review named because they are the places a correct-looking
implementation could still be wrong:

- every relevant **writer** genuinely participates in the guard and publication
  protocol, rather than some path bypassing it;
- **internal stalls are not classified as recipient nonresponse**, so Sophia's
  own writer waits, control preemption and frozen input never terminate a
  healthy client;
- the **same-recipient clearing barrier orders actual wire delivery**, not only
  bookkeeping;
- concrete **timeout constants and progress instrumentation are explicit** in
  the implementation and its tests.

No measured latency claim is made by the design or by the review. The provisional
scheduling values are starting points to instrument.

The five gates chosen by the operator stand unchanged.
