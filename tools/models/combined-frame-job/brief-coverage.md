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
