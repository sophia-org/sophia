# Coverage of the T4 design probe

All rows refer to WorkerSubmit.tla. Configurations enable their named invariant
or property explicitly; there is no trace-validation or native coverage claim.

| Scenario | Config | Expected result |
|---|---|---|
| Protected abstract handoff | Safety | All six type/safety invariants pass |
| 1: withdrawal between check and ioctl | Unserialized | FreshSubmit violation |
| 2: accepted then lost reply | LostReply | PhysicalCustody violation |
| 2: accepted then worker exit | WorkerStop | PhysicalCustody violation |
| 2/3: submission ownership installed only on return | LateLedger | SubmittedAccounted violation |
| 3: no pre-armed card coverage | LateCard | CompletionArmed violation |
| 3: free displayed buffer at its own completion | EarlyRelease | PhysicalCustody violation |
| 3: foreign callback consumed | ForeignCompletion | ExactCompletion violation |
| 4: stalled submission gate | WithdrawalProgress | WithdrawalEventuallyApplied temporal violation |

The seven negative safety controls remove proposed guards; they are not
counterexamples against today's owner-side submission. The progress probe uses
the protected design, not a fault-switch variant. No physical-detach fairness
assumption forces that probe to succeed.
