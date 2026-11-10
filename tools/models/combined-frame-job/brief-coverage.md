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
