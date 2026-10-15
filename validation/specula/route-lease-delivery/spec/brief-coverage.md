# Coverage

All four brief scenarios share one small exhaustive search, `MC.cfg`, which
enables Capacity, Custody, Unique, EffectCustody and Authority. Full channel and
delayed drain exercise Transfer and retained pending values. Reserve/Abort
exercise unused credit; Disconnect can interleave with Effect and Publish.
Advance/Consume model Session's existing stale-identity refusal.

`MC_drop.cfg` enables a deliberate silent-drop mutation and must fail Custody.
`MC_stale.cfg` enables a deliberate stale-confirmation mutation and must fail
Authority. Neither is a newly discovered production bug. `Trace.cfg` checks a
design sequence through a full channel, delayed drain and stale consumption,
including post-state values. It is not a runtime trace or refinement proof.

Wake delivery, operating-system scheduling, aggregate input queues, mutex
poisoning and whole-Service shutdown are outside this finite model. Real Rust
controls must cover nonblocking return, delayed drain, FIFO, reservation limits,
disconnect and the Session capacity wake. No broader bug-archaeology claim is
made for this narrowly approved repair.
