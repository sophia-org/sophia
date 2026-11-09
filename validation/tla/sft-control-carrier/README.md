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
only from an owner-authored matching receipt; ring consumption is not ACK.
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
