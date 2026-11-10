# Coverage audit

Read against none.cfg/stale.cfg/release.cfg/retire.cfg, not inferred from exits.
All four configs enable TypeOK, FreshSubmission, WorkerOwnership,
SubmittedOwnership and PredecessorLifetime. none additionally enables temporal
WithdrawalProgress under weak fairness of ApplyWithdrawal, with NO worker
fairness. The three mutations exercise source guard removal, early source lease
release and predecessor retirement respectively. They are design counterexamples,
not implementation bugs. SubmittedOwnership has no dedicated mutation yet.

Model completion proves these finite predicates only. Missing scenarios from the
full plan (no prototype gate yet): real budget failure/rollback; pixel mutation;
source extent/read errors; scene/topology replacement separate from grant epoch;
result identity/misroute; renderer Deferred/retry; real ioctl unknown result and
proven detach; held/full journal and covering ACK; worker exit/wake delivery;
reference-counted resource release and actual GPU/scanout last use.
The trace-generation phase is deferred because the proposed path has no code.
A fabricated trace would not establish correspondence. README gives the future
instrumentation fields. The next model must cover these remaining scenarios or
explicitly compose independently checked existing contracts before approval.

## Selected direct-transfer projection

Transfer.tla follows TRANSFER.md's chosen two-thread topology. It checks two
sequential reservations, unique payload ownership on transfer, held credit until
owner consumption, private validated-only rendering and cancellation at render
start. Cancellation racing after start may finish private work. Weak fairness
of Withdraw requires no worker or queue progress. Full result route/busy renderer
may stall forever without enabling authority retention. Worker death quarantines
and never releases credit: no speculative detach proof is modeled.

transfer-none.cfg enables all five invariants and WithdrawalProgress. Negative
controls duplicate/credit/cancel change transfer or render actions and must fail
UniqueOwner/CreditUntilConsumed/CancelGate respectively. No private-validation
mutation yet. These are design tests, not implementation trace validation.

This projection does not prove wake delivery, eventual completion/fairness, actual
slot-release or last-use, real full-inbox behavior, deadline arithmetic, descriptor
I/O, scene versioning, install rollback or DRM outcomes. Consume assumes the owner
can fully settle an envelope: resource retention beyond it is outside this credit
projection. Do not use that abstraction as permission to free scanout resources.

Review clarifications: Transfer.tla checks cancellation at Enqueue/Render, not
validation chunk boundaries or Take. Reserve requires idle, so there are two
sequential reservations and never two active jobs; no displayed predecessor in
this projection. Consume resets slot; slot-pressure deferral is not reachable
independently of the unconstrained Busy toggle. Fail-from-waiting is an abstract
owner-return route, not permission to drop the envelope on the renderer.

## Held-peer source-release witness

SourceRelease.tla checks journal-first finished-ticket release and release-gated
offers with two slots and one admitted frame. It includes validation, parking,
render, owner consumption, presentation, journal room/ACK and peer WAIT milestones.
The journal control deliberately finds a timeout with completed rendering and
presentation; the early-confirm mutation violates ReleaseProof. The normal model
does not promise that ACK/worker/owner progress is fair, or that a future native
run meets cadence. The held peer does not produce the earlier conjectured n+2
busy-slot Budget. Read README for the simplified peer flags and illustrative
parked bound; no source model is composed with either ownership projection.

Revision 4 qualification (review 58230480): SourceRelease.room is unconstrained
and omits the four reserved responses. Its journal timeout counterexample is
outside that contract and must not be transferred to the held lane. No identified
unreserved record can spend that reservation. The future composed model must
carry it. Transfer.tla also lacks the new one-way forwarded flag and its memory
ordering; no atomic implementation proof follows from its global phase variable.

## Approved fallback clause

FALLBACK.md F1-F6 map one-to-one to fallback-eligibility/retry/rollback/release/
prepared/fresh.cfg. Every config, including fallback-none.cfg, explicitly enables
TypeOK and the six corresponding safety properties; controls change actions, not
invariant constants. No control or positive check establishes the exact real
Release acknowledgement, fresh-check/kernel interval, multi-output rollback,
complete scene version, channel wakeups or forwarding flag memory ordering.
The model begins after validation and speculative render and ends at conventional
entry, so no composed proof or Rust trace is claimed. Description stands for
validation equality/current geometry eligibility, not mutable validated pixels.

## Forwarding witness amendment

All five transfer cfgs enable ForwardedBeforeRetirementSettlement and
ForwardedOnlyAfterMove. transfer-forwarding removes the witness gate on exact
retirement settlement and must fail ForwardedBeforeRetirementSettlement.
ForwardedOnlyAfterMove has no dedicated mutation; it is a structural check.
Move, delayed flag store, validator exit and exact retirement are separate actions.
A later old-job flag store is indexed by job, and cannot satisfy a successor's
proof. This is a sequentially consistent design projection; the real memory
ordering and native owner's extended resource disposition remain unproved.
