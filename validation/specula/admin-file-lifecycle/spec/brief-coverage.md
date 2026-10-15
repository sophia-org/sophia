# Brief coverage

Read against the checked-in configuration files, not intended coverage.

| Brief scenario | Actions | Enabled checks |
| --- | --- | --- |
| 1: cancellation versus claim | RequestCheck, Approve, BeginCancel, Disconnect, Dispatch | CancellationWins, AtMostOnce in MC_hunt_custody.cfg |
| 2: timeout and retained execution | ExpireDispatched, WriteFinish, Publish, Settle, Reclaim | ExecutionCustody, BoundedCustody in MC_hunt_custody.cfg |
| 3: replay and result lifetime | Submit, ReadStatus, ClunkOrFlush, Publish, Reconnect | AtMostOnce, ResultRetention in MC_hunt_custody.cfg |
| 4: catalog/admission refusal | ReplaceCatalog, BeginCancel, Dispatch | CurrentDispatch, CancellationWins in MC_hunt_custody.cfg |

All §5 safety properties are enabled in the merged hunt. Base MC.cfg checks
TypeOK and BoundedCustody. The three §6.1 sensitivity questions have independent
negative configurations: MC_early_release.cfg expects ExecutionCustody;
MC_replay.cfg expects AtMostOnce; MC_consuming_read.cfg expects ResultRetention.
Trace.cfg requires TraceMatched; Trace_bad_settlement.cfg must fail it.
No property is silently disabled in every configuration.

Scenario 5 (restart extension) is checked separately by `Restart_hunt.cfg`:
Stop, Close, SettleCancelled, Expire, Join and Bind establish OldTransportClosed
and ShutdownCancels; interleaved FinishResult, FinishSettled and Publish establish
ResultIsolation. TypeOK bounds effect counts at one per instance. The model
allows one replacement and keeps every reactive owner step enabled.
`Restart_no_cancel.cfg` violates ShutdownCancels; `Restart_cross_instance.cfg`
violates ResultIsolation. `RestartTrace.cfg` checks all fields through eighteen
design events; its bad-settlement variant must fail TraceMatched.

The merged hunt shares the base domains, rather than bounding away reactive
owner steps. There is one catalog replacement and one successor connection;
timeouts are bounded by the finite ticket domains and first-result rule.
Raw parser failures and unavailable-capacity requests stutter and are not counted
as positive runtime coverage. Process restart, weak-memory refinement, wakeup
liveness, real permission inspection and runtime trace conformance are excluded
explicitly in the brief. No production bug is inferred from a deliberate control.
