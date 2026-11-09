# SFT control carrier: initial custody and wake projections

Status: reviewed design projections and a separate executable mechanism checkpoint,
NOT a complete prototype/comparison gate. See decision z8cr4n2p and the limits below.

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

### Follow-up: answered refusal and ACK overlap

The following checked revision restricts `unknown` on Close to an unanswered
queued/captured/pending/delivered attempt. A received EAGAIN or definitive refusal
is not reclassified unknown on disconnect. It adds `OneAckInFlight` and the
`CommitSerializeAck` negative control. Five positive configurations and fifteen
negative controls pass. Evidence: sophia-control-carrier-model-03/reproduce.log;
model-02 stays frozen. No implementation/prototype qualification follows.

### Accepted model limits and the executable checkpoint

Read-only review of 2d5d5b493 permits standalone device-free correctness work,
not timing or host integration. Review seal:
`f821f765d5869e375e40aecc3c4194f3faafe142147f35dcbcd2a4f4ba91873e`
in `sophia-control-carrier-model-review-claude-01`.

- `KeepDeadline`, `EchoAck`, `ConfirmSuccessOnly` and `SerializeAck` controls set
  their bad flag directly from the configuration. They test helper detection,
  not the corresponding design property. In particular, the abstract expiry
  value is written but not read. Executable deadline, receipt and ACK tests must
  establish the actual behavior separately.
- `already` is unreachable in `CommitOnly`: success ends the attempt, and Publish
  requires idle/retry. Epoch 2 cannot Process either. Duplicate EALREADY and
  client fail-closed behavior are not modeled; replacement attach only exercises
  replay refusal, not a legitimate new-epoch submission.
- A stale refusal ends the modeled attempt. Reopening a writer on the same
  attach and republishing the same id is executable-test work, not model coverage.

These are uncomposed finite projections over abstract phases. They contain no
Rust, byte offsets, wrap implementation, memory ordering, pread outcomes, real
clock, fid objects or journal. State counts are not coverage or refinement proof.

The executable mechanism checkpoint lives in
[tools/probes/sft_control_ring](../../../tools/probes/sft_control_ring/README.md).
It selects one SQ slot and one CQ slot with sequential attempts. There is no
peer-written CQ consumer head: the next valid SQ publication follows receipt
consumption by a conforming client. Host-local sequence validation refuses gaps
and wrap. `QueueIndex` is retained as a deeper-queue alternative, not evidence
about a remote-head input in this probe. A hostile client publishing early can
lose its own receipt; that never licenses acceptance, ACK or frame release.

### Executable receipt-pipe amendment

The real-export follow-up supersedes the initial mapped CQ with a directed pipe
carrying one exact receipt. The SQ has a sequence-hint pipe so stale hints can be
classified before any pread of a reused cell. The models and frozen checks above
are unchanged: their abstract response reservation/delivery can describe this
order, but do not prove pipe I/O, mutable input, process wiring or the runtime
export. QueueIndex is not a property of this selected carrier. The probe README
maps code boundaries to projection actions and names the remaining parity gate;
that mapping is not trace validation or refinement evidence.
