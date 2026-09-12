---
id: htm85gg0
date: 2026-09-12
kind: adr
status: accepted
tags: [adr]
---
# Admission, ingress and provenance for synthetic input

The operator later reopened the delegation gate and deferred the shared-seat
host-user administrator mode recorded below. Implementation is authorized only
for the [private native authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md).
That successor changes the implementation scope, not these provenance and
cleanup principles. It authorizes no live-seat activation or deployment.

## Context

[t030](../plans/queue-11-parallel-production-readiness.md) excluded `XTEST` and
said why: it is four requests and cheap to write, but the reference server's is
entirely ungated, and their own notes record that as a known gap -- any client
can drive the pointer and keyboard. Sophia's input is session-owned authority,
so t030 held that synthetic input needs an explicit admission story *before it
exists at all*, and invited a revisit when conformance tooling made that story
worth designing. `xts5` drives the mouse through XTEST, and a physical-acceptance
harness wants it for setup. This is that revisit.

This note settles admission, ingress and provenance. It does not authorise
implementation; it is the contract an implementation would have to satisfy.

## What the shared seat cannot promise

The earlier framing -- that XTEST cannot be namespace-scoped -- was too strong,
and is withdrawn. A virtual seat, or an injector that verifies and bounds every
effect to an admitted namespace, is a coherent design. What is true is narrower:

> The **shared-seat** path cannot promise namespace confinement. Injected input
> follows global focus and grabs, so its effects land wherever those point,
> which may be a window in another namespace.

That is why the shared-seat path is desktop administration in the sense
`docs/scripting.md` defines, and why it is refused by default. It is not a claim
that no scoped design exists. `ClassicShared` membership is not a synthetic-input
grant either: scripting.md separates resource admission from control
authorization, and shared resource access does not imply the right to drive the
seat.

## Admission

**Authorization is per admitted caller, and the global enable is also
necessary.** A global enable makes the capability *available*; it never
authorizes a connected client by itself. Absence of either the enable or the
caller's authorization disables discovery and effects alike.

**Discovery follows authorization.** `ListExtensions` and `QueryExtension` report
XTEST only to a caller authorized for it, and they must agree -- the property
t086 repaired for the rest of the extension set. Absence is the default for an
unavailable capability. Hiding the extension is not a security boundary: a
client that guesses the opcode meets the same denial, and that denial is an
explicit error, never silence or a dropped request.

`BadAccess` is the denial. A client may report an authorization denial and an
absent extension alike as failure, but they are distinguishable on the wire --
`BadAccess` is not `BadRequest` and not `BadImplementation` -- and an
intentionally absent extension is not a defect. What the contract requires is
that the **refusal record** separate intentional authorization denial from
missing implementation, so the log answers which one happened.

### The authorization boundary is atomic, not a check followed by work

A check, then an enqueue, still races revocation; so does a check followed by a
focus, seat or security-epoch change. Authorized work binds to the **admitted
connection, the grant generation, the session, and the seat and security
epoch**, and there is a single authoritative commit boundary shared with
revocation:

- work not committed at that boundary when revocation takes effect does not run;
- effects already committed are not retroactively undone;
- focus, grab resolution and security validation are coherent at that boundary;
- no reconnect, and no reuse of a client or resource identifier, inherits an
  earlier grant's pending input.

**Cleanup outlives the grant.** Retiring a revoked caller's contribution must
remain possible after injection permission is gone, and when the ordinary queue
is full. Cleanup that depends on the path being available is cleanup that fails
exactly when it is needed.

## Ingress

**Synthetic input joins shared authoritative routing through an explicitly
synthetic ingress.** Session authorizes; Engine remains the input authority.
`FakeInput` must not write client events directly, because that steps around
committed focus, active grabs, the security epoch, input recovery, and lock and
VT policy. It does **not** follow that synthetic input impersonates a libinput
device: the protocol requires shared routing, not a forged device identity.

`crates/sophia-session/src/live_session/owner_loop/input_proof.rs` injects
through the committed Engine path and *asserts* it traversed committed focus.
That is precedent that the ingress constraint is satisfiable. It is not a
general XTEST grant, and its single-source release helper is not proof that
multi-source cleanup works.

**Reserved session actions default to refusal.** Whether this grant may invoke
VT switches, emergency chords, or anything with locked or inactive-seat
behaviour is a separate question from whether it may inject a keystroke.
Anything requiring physical presence or separate authority is refused unless
explicitly granted.

**A delay blocks one client, not the server.** `FakeInput`'s time field is a
delay, not a timestamp, and it constrains the processing order of that
*client's* subsequent requests. During it, revocation, cleanup, healthy peers
and physical input all continue. A later X reply is not by itself proof that a
recipient received anything or that anything was presented.

## Provenance and cleanup

**Provenance is enforced runtime state, not an evidence label.** Synthetic input
carries a synthetic identity that the runtime acts on, and it is never promoted
into something indistinguishable from a physical gesture. `source=synthetic` in
the existing path is the precedent for saying so in evidence; the contract needs
the state itself.

**Synthetic input does not close a physical obligation.** It may drive setup and
supply explicitly labelled regression evidence. It cannot substitute for the
physical evidence required by t077, t060 or t062: it
establishes nothing about hardware or libinput delivery, physical timing, VT
behaviour, or recovery through the original device path. Before a physical phase
begins, retire synthetic contributions under the cleanup rules below and settle
outstanding work, and keep the two phases and their provenance separately
identifiable in the record.

**Cleanup retires a contribution, it does not release the seat.** This is where
the naive reading is actively unsafe. If the operator physically holds Shift,
injector A also injects Shift-down, and A is revoked, cleanup must leave the
physical hold intact. The same holds when a second injector B holds it too.

So the contract requires **source-owned contribution tracking**: each source's
contribution to a key, button or modifier is held separately, with a stated
policy for overlap and for duplicate transitions. Retiring A removes A's
contribution.

The prohibition is on an *unconditional* release, not on releases as such.
Cleanup must not issue an unconditional seat-wide key or button release, remove
another source's held contribution, or arbitrarily cancel another client's grab.
But where retiring a source changes the aggregate logical state -- the retired
source was the last holder -- authority-owned reconciliation may produce the
necessary release transition and the corresponding recipient cleanup, under the
established target, lease and epoch rules. A delivered release necessarily
changes that recipient's protocol state; that is the point of it. What it must
not do is retarget that cleanup into a new user action on whatever window
currently has focus. The overlap and duplicate-transition policy defines this
behaviour.

Forbidding every release would recreate the stuck key this rule exists to
prevent.

**Injector-owned state and target-side state are different things.** A caller's
own `GrabControl` server-grab imperviousness is its to retire. A passive grab
that its injection *activated* inside a target client belongs to that client;
the implementation must specify how that recovers rather than cancelling
another client's grab.

## The other three requests

`GetVersion` is a version handshake, answerable wherever the extension is
advertised at all.

`CompareCursor` splits. With an explicit cursor and window it is a
resource-scoped question, answerable within the caller's namespace. Asking about
the **current** cursor is a disclosure question about what the seat is
displaying, and needs its own check or an admitted virtual-seat reading.

`GrabControl` makes the calling client **impervious to server grabs**. It does
not release pointer or keyboard grabs and does not grant injection. Gating it
with the administrative surface is defensible, but its actual scheduling
semantics must be implemented and tested rather than stubbed. Neither request
may report behaviour the server does not perform.

## Settled here, chosen since, and still owed

Settled: default absence; the global enable and per-caller authorization are
both necessary; discovery agrees with authorization; the atomic commit boundary
and its revocation semantics; synthetic ingress through shared authoritative
routing; provenance as runtime state; source-owned cleanup; corrected
`GrabControl` and `CompareCursor` readings; and the physical-evidence rule.

**The admission gates are now chosen.** The operator approved these on
2026-09-12 as constraints for a concrete design. They are decisions about
authority, not about implementation, and they authorize neither.

- **Authorization.** Session grants access to individually verified
  connections. Disconnect, revocation, locking, or leaving the active seat
  cancels pending input. Reconnecting requires fresh authorization -- never a
  resumption that could replay what was pending.
- **Permissions.** One administrator bundle initially: input injection, cursor
  comparison, and correctly implemented server-grab control. Namespace
  restrictions on resource access remain.
- **Trust boundary.** Sophia's existing host-user administrator boundary,
  **disabled by default**. The operator was explicitly informed, and accepted,
  that this trusts every eligible application in that domain including a
  browser, and that it is **not harness-only authorization**. Selective
  delegation to confined harnesses is deferred to a later design.
- **Limits.** Start at 16 authorized injectors per seat and one pending request
  per injector, with bounded queues, fair scheduling, reserved cleanup capacity,
  and physical input that stays responsive. Protocol delay semantics are
  preserved with bounded state rather than clamped.
- **Protected actions.** Ordinary application and window-management automation
  is permitted. VT switching, emergency chords, authentication and unlock, and
  injection into a locked or inactive seat are denied. Synthetic tests do not
  replace physical acceptance.

The concrete design answering these lives in
[qoltxfr5](../plans/qoltxfr5-concrete-design-for-admitted-synthetic-input.md),
proposed and separate, so this note stays the principles and authority record.

**What the concrete design still owes.** Choosing the gates did not settle how
to satisfy them, and a design returns for review before any code:

- exact scheduling limits, with justification rather than assertion -- the
  service turn budget, the per-turn event count, and why those numbers;
- how the delay is held in bounded state across the full CARD32 range without
  blocking the server or delaying disconnect and revocation;
- the mechanics of the atomic commit boundary shared with revocation, and where
  it sits relative to focus, grab and epoch resolution;
- the representation of source-owned contributions, and the overlap and
  duplicate-transition policy in concrete terms;
- how discovery filtering is implemented so `ListExtensions` and
  `QueryExtension` cannot disagree;
- what the refusal record contains, so intentional denial stays distinguishable
  from missing implementation.

One value is already contested and the design should resolve it rather than
inherit it: 16 injectors with one pending request each makes a separate
16-per-seat request cap unreachable. Either it guards a case not yet named, or
it is redundant.

## Consequences

Default remains absence, as t030 decided. What changes is that there is a
contract to satisfy and a named list of what remains unchosen.

An implementation admitted under this note owes independent manifest cases for
enabled, disabled, unauthorized and revoked callers in both byte orders, and
these negatives specifically:

- physical and synthetic holding the same key, with the synthetic source
  retired, proving the physical hold survives;
- two injectors holding the same key, with one retired, proving the other
  survives;
- a sole synthetic holder revoked or disconnected, proving the aggregate and
  recipient state clear with no stuck key or button;
- revocation landing on delayed input at the commit boundary;
- a reconnected client, or a reused identifier, not inheriting pending input;
- cleanup succeeding while the ordinary queue is exhausted;
- healthy peers and physical input continuing throughout a delayed caller's block.

XTS results stay separate from physical acceptance records.

Nothing here authorises writing the extension. It states what writing it would
have to honour, and what still has to be decided first.

## Acceptance and connections

Accepted 2026-09-12 on Codex review of `a60f837a`, the revision restoring the
physical-evidence rule and correcting source-owned cleanup so that retiring the
last holder may produce the release which clears an aggregate hold while other
sources' holds survive.

**Accepted as a principles contract only. Grant issuance/binding, permission
scope, delegation boundary, bounds/fairness and reserved actions remain open
implementation-admission gates. No XTEST implementation or deployment is
authorized by this acceptance.**

On the same date the operator subsequently approved the five gates, recorded
above under "Settled here, chosen since, and still owed". That approval is
likewise constraint rather than authorization: a concrete design returns for
review, and no implementation or deployment is authorized by either step.

Default absence and current implementation behaviour are unchanged by this
record. [t030](../plans/queue-11-parallel-production-readiness.md) excluded XTEST
pending exactly this admission story and invited the revisit; this note supplies
the principles it asked for, not the decisions the operator still owes. When
those gates are chosen, the concrete design returns for review before any code.
