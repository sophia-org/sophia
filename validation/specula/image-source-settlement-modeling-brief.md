# Image Source Settlement Modeling Brief

**Role:** bounded validation model for the sealed ISB image path and the
default-off D4 option B owned-copy ring. It is not a refinement proof of the
Rust implementation and makes no performance claim.

## Question

Which `ResourceStatus` may spend an admitted offer's held terminal-status
reservation, and when may the source be released? For sealed images the
release is `SourceReleased`. For the ring it is `SlotReleased`.

Reading and runtime tests found three defects in this boundary:

- A client `ResourceCancel` under its own transaction settles under that
  transaction. A matcher keyed on the offer id never spends the hold, so the
  source is never released.
- A failed `ResourceRetire` on an open transfer draws a synthetic status-3
  reply. A matcher keyed on the offer id, or on the resource alone, lets that
  reply spend the hold. The release then precedes the transfer's real final
  status.
- In the ring, publishing the slot release before the read ticket drops lets
  an honest client rewrite a slot Sophia is still reading.

The model explores every interleaving of these at a small bound. Each defect
is kept as a negative control.

## Model

`validation/tla/ImageSourceSettlement.tla` has two offers, one grant and at
most one channel loss. Each offer can receive one client cancel and one retire,
under its own id or another transaction. The source mapping is in the module
header.

`MatchMode` selects the export's matcher:

| Value | Matcher | Where it lives |
| --- | --- | --- |
| 0 | Provenance: only a store-flagged settlement | Option B, and the shared lift |
| 1 | Transaction-keyed, with the EINVAL fault on a spent hold | Sealed `append_image_status` before the provenance fix |
| 2 | Resource-keyed | Option B's interim matcher (review 03) |

`Ring` adds the slot-reuse rule, so a client fills a slot only after it is
released, and checks that no slot is rewritten while being read.

## Properties

The model checks these invariants:

- `NoAppendFault`
- `StatusBeforeRelease`, in the same journal
- `OneFinalPerOffer`
- `OneReleasePerOffer`
- `ReleaseInOwnEpoch`
- `NoWriteDuringRead`

It checks one liveness property, `EveryOfferReleased`: every offer made on a
live journal is eventually released unless that journal is lost. This holds
under weak fairness for owner admission, worker reads, settlement, status
append and release. There is no fairness for client actions or expiry.

## Results (TLA+ Tools v1.7.4, pinned jar, `-deadlock -workers 1 -fp 0`)

| Config | Expected | Result |
| --- | --- | --- |
| `ImageSourceSettlement` (sealed, provenance) | pass | pass; 3,072,078 distinct states, depth 24 |
| `ImageSourceSettlementRing` (ring, provenance) | pass | pass; 357,626 distinct states, depth 24 |
| `ImageSourceSettlementTransactionReply` | `StatusBeforeRelease` violated | violated |
| `ImageSourceSettlementTransactionCancelLeak` (no offer-id reuse) | temporal violation | violated |
| `ImageSourceSettlementRingResourceKeyed` | `StatusBeforeRelease` violated | violated |
| `ImageSourceSettlementRingEarlyRelease` | `NoWriteDuringRead` violated | violated |
| `ImageSourceSettlementRingReleaseIgnoresEpoch` | `ReleaseInOwnEpoch` violated | violated |

The two transaction-keyed controls reproduce the counterexamples that the
runtime test `crates/sophia-runtime/tests/sealed_image_control_provenance.rs`
showed on the committed sealed path.

## Not modeled

The model leaves out the following, which Rust tests cover:

- pixels, torn producer writes, and the generation-word check;
- budgets and per-component credits;
- allocation and topology currency;
- journal ACK room, beyond FIFO order;
- per-job withdrawal on identity loss;
- more than one loss, and ring re-registration.

## Gate

`tools/check_tla.sh` runs both positive configurations against the shared
`ImageSourceSettlement.tla` module and all five negative controls. Each control
must produce its named invariant failure, or the temporal-property failure for
`TransactionCancelLeak`; parser errors and unrelated failures do not qualify.
