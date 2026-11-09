# SFT control carrier: initial custody and wake projections

Status: initial design projections, NOT the complete prototype gate. No carrier
implementation exists. See decision z8cr4n2p in docs/notes/decisions.

Run the pinned TLC helper from the repository root:

```
SOPHIA_SPECULA_TLA2TOOLS_JAR=/absolute/path/to/pinned/tla2tools.jar \
  bash tools/check_sft_control_carrier_model.sh
```

The helper checks both positive configurations and requires the named invariant
or temporal failure for every negative control. All checks run in temporary
copies; TLC does not write state/trace files into the checkout.

Carrier models one attempt, two possible payload values and one epoch withdrawal.
Capture reserves one receipt slot and copies the payload privately. Accept
represents reaching existing file custody, not frame admission or presentation.
The producer may mutate input before or after capture. Success knowledge comes
only from an owner-authored matching receipt; ring consumption is not ACK. `reuse` represents reuse of semantic accepted-custody state, not reclamation of an SQ cell after its complete private capture. That transport cell can be reclaimed independently without releasing pixels or frame credit.
The model has no reopen/retry action: it cannot prove no replay across reconnects.

Wake models one publication and notification, separately interleaved with owner
clear/check/sleep. Weak fairness requires notification and enabled owner steps
to complete. It does not guarantee progress when a publisher stalls or dies
before notification. A real carrier must poll process-exit/control descriptors
and have bounded cleanup; that coupling is still to be modeled. It does not model
notification suppression, CPU cost, memory reordering or eventfd overflow.

## Coverage and remaining gate

| Scenario | Check | Negative control | Limit |
|---|---|---|---|
| Known acceptance must be real | KnownIsReal | ExactReceipt | one attempt |
| Peer mutation cannot replace captured record | ImmutableAcceptance | SnapshotInput | abstract two-value payload |
| Revoked epoch cannot accept | AuthoritySafe | FreshAuthority | abstract fresh gate, no pidfd implementation |
| Response space before effects | CapacitySafe | ReserveReply | one reservation, no full journal |
| Consuming queue is not covering ACK | AckBeforeReuse | SemanticAck | ACK eligibility abstracted |
| Publication between empty check and sleep | Progress | WakeLateClear | cooperative fair notification |

Before a prototype is cleared: extend coverage to bounded queue/index arithmetic,
multi-attempt/retry/epoch replacement, receipt-delivery authority, ACK prefix
application/confirmation on refusal, full journal/close reserves, independently
held events, stalled publisher with process-exit wake, and cleanup/drain. Add
trace-to-model mapping when executable code exists; do not claim refinement from
these projections. The existing FrameSubmit/LeanPrefix and lifecycle models stay
unchanged and remain separate evidence, not automatically composed proofs.

## Initial check, November 9

Pinned TLC jar e22f8ffb: Carrier Safety 345 generated / 100 distinct states;
WakeSafe 15 / 13. Both complete. Five custody guard-removal controls reach their
named invariant violations; late-clear reaches a Progress temporal violation.
The helper exits 0. Evidence: ~/.local/state/sophia-control-carrier-model-01,
SHA256SUMS c05e688651abafdbe4a79cc5876e808b835600d9e09add5d9dead3896586ecea.
Initial syntax failure and the helper's plural/singular error-message mismatch
are retained in that evidence; only reproduce-final.log is the successful gate.
The listed open scenarios above remain open; these counts are not qualification.

Contract review 29e08b85 identifies the covered exchange set as an unresolved
prototype choice: commit-only is recommended; journal-on-CQ changes the events
contract. See z8cr4n2p's challenge addendum for all eight required semantics.
The initial Carrier projection combines acceptance and receipt creation and
therefore does not prove guarded publication after an intervening withdrawal.
Its reuse property is abstract, not an implementation of lean commit recycling.

## Commit-only expansion (scope selected November 9)

niltempus selected commit-only and pread-first. The old unresolved-choice text
above is the history of checkpoint 708b1cb25, not the current scope.

`CommitOnly` adds two attempts of the same submission, an ACK word refreshed on
retry, pre-ACK response reservation, original deadline identity, token expiry,
separate acceptance/publication/observation, withdrawal and replacement attach,
and unknown-custody replay refusal. Success receipts echo the attempt's ACK word;
EAGAIN never confirms it. A live attach with an expired writer yields definitive
stale refusal, not a blocked request. Expiry after acceptance does not erase it.
One ACK in flight spans both carriers. Journal offsets, holds and event delivery
stay on the unmodified 9P path; this model assumes its valid ACK frontier.

The proposed fresh CQ publication gate is a conservative new carrier rule.
`connection.rs::read` sets guarded delivery for Rread, not all Rwrite responses;
we do not claim that existing Rwrite has an identical additional delivery check.
That candidate work must be included in the comparison. Already-published bytes
can be observed after withdrawal in the old attach; they authorize nothing new.

`QueueIndex` checks monotonic validated remote-head accounting and two-entry
capacity through four publications, including a hostile ahead-of-tail value.
`ExitWake` checks cleanup through a registered exit descriptor if notification
never happens. It assumes the owner services that descriptor fairly and proves
no real-time bound. A stalled-but-live publisher additionally needs the existing
operation deadline; timeout must close without inventing acceptance or release.

These are separate bounded design projections, not a composed refinement proof.
Before measurement, executable tests must cover actual byte sizes/offsets,
integer overflow (refuse before wrap), mutable metadata/payload snapshots,
response identity including ACK word, retries with the same bytes/deadline,
EAGAIN vs definitive refusal, old mappings, token expiry and delayed/failed I/O.
The existing journal remains authoritative; exercise its full/held/read-offset
and idle-ACK behavior in both carriers rather than invent a second journal model.
Trace observations must be compared with model transitions once code exists.
No host path or performance gate is cleared by this model check alone.

| New scenario | Positive invariant/property | Negative control |
|---|---|---|
| Response reservation before ACK effect | CapacityBeforeEffects | CommitReserveBeforeAck |
| Withdraw between accept and publish | FreshDelivery | CommitGuardDelivery |
| Retry response names applied ACK word | ExactAckEcho | CommitEchoAck |
| Refusal does not confirm ACK | SuccessConfirms | CommitConfirmSuccessOnly |
| Retry does not extend deadline | OriginalDeadline | CommitKeepDeadline |
| Lost result cannot license reconnect replay | NoReplay | CommitNoUnknownReplay |
| Hostile CQ head cannot grow free capacity | Bounded | QueueUnchecked |
| Exit wakes even without notification | ExitProgress | ExitUnwatched |

All five positive configurations and fourteen named controls pass the expanded
helper. Full local log: sophia-control-carrier-model-02/reproduce-expanded.log.
This adds review input; it is not approval of a memory mapping or a live carrier.
