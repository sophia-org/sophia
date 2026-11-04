# Bounded native FrameCommit design

This is a **proposed protocol**, not a model extracted from an implemented
FrameCommit codec. Source anchor: Sophia `8359816f0`; experimental credit
checkpoint `f639d21d2`; W2 retirement contract `892bd1436`. W2 and W3 are not
production features on the source anchor. No opcode or revision is allocated.

The performance question is whether removing producer-side prerequisite
handshakes and repeated owner dispatch can beat the freshly measured Niri lane.
TLC answers lifecycle questions, not that performance question.

## Scope and source boundaries

This is a message-passing design with asynchronous physical ownership. It reuses
the authenticated files transport, owned-ring capture, validated immutable
resources, existing GL composition and native display completion. It changes the
transaction boundary, not the trust boundary. No Vulkan migration, new shared
memory command transport, unsafe zero-copy or authorization cache is proposed.

| Requirement | Existing foundation; new composition to check |
|---|---|
| D1: reserve before consuming | `shell_transport/owned_copy/admission.rs:55` reserves image work; W3 `frame_credits.rs:126` reserves before spending. New: one credit spans asynchronous capture and candidate admission; typed refusal also needs capacity. |
| D2: privately captured and validated pixels | `shell_content/owned_ring.rs:240` reads once; generation words do not authenticate pixels. `isb_worker.rs:327` scans owned bytes. New transaction must preserve this. |
| D3: independent releases | `owned_ring.rs:188` and `shell_transport/owned_copy.rs:198`: source ticket completion and exact release publication. `resources.rs:484`: backing release after retirement and leases. These are not one completion event. |
| D4: whole scene, fresh authority | `renderer_images.rs:48`, `composition_queue.rs:142`, `mirror_scene_tick.rs:123`: complete validation and prepare-all before individual head submissions. New: validate complete FrameCommit intent, then revalidate after capture and before handoff/submission. |
| D5: terminal accounting | W3 ledger `frame_credits.rs:154`; W2 `shell_transport/content_candidates.rs:433`: terminal transfer, not Prepared/ACK, returns credit and commits retirement intent. |
| D6: revoke and close drain physical work | `candidates.rs:884` retains submitted work; `resources.rs:270` waits for stopped worker completion. W3 facts reconciliation closes mismatched streams. New epoch cannot inherit old completion effects. |

Paths under `crates/sophia-runtime/src` unless the filename names Session/Engine
rendering. Exact source pin is required when resolving these abbreviated anchors;
the W2/W3 rows deliberately reference their separate commits.

The current wire poller admits candidate input with zero additional response
credit (`shell_transport/content_candidates.rs:123`). Unreported candidate errors
stop the shell (`:228`). **Before any FrameCommit opener:** implement peek/shape-aware
reservation, nonfatal Budget/close-race refusal, and trailing-input handling. A
blanket slot per fragment deadlocks a full reserved budget. This model abstracts
one complete bounded record and does not discharge that wire integration gate.

## Proposed transaction

Setup binds connection/grant, output, allocation, facts, ring and stream identity,
scene limits and a server-issued finite credit window. Facts include mode, scale
and formats; a change closes the stream. This is a structural bound, not a rate
limiter. Fresh authority observations remain required at their contract points.

A FrameCommit contains complete scene intent (or a delta against an exactly
bound immutable scene), source slot/version, candidate identity and predecessor
retirement intent. Admission reserves copy/worker capacity, response obligations
and credit before consuming it. Capture and validation run asynchronously under
that reservation. Failure preserves the current display. A rejected copy cannot
return its credit before its read ticket is finished. Revoke can discard logical
obligations but does not free physical worker/native ownership.

Source-release notification can precede presentation. Prepared precedes Presented.
Credit returns at terminal transfer while the stream remains Open. Predecessor
retirement commits with Presented transfer; actual backing release waits for all
leases and its publication rules. Batch events that are already ready; never
hold a source release merely to manufacture one reply per frame.

## Finite model and invariants

`base.tla` separates admission, capture completion, validation, renderer handoff,
per-head preparation/submission/flip, Prepared publication, terminal transfer,
source publication/delivery/reuse, facts change, close, revoke and predecessor
reclamation. Submission is represented by a complete immutable frame identity;
each head changes visibility only at its own completion. Head 1 is explicitly
the primary logical presentation head. Other heads may lag, as current policy
allows. There is no simultaneous cross-head flip claim.

The model has two epochs, at most two one-shot frame identities and up to two
heads. A facts-generation token abstracts the complete authority tuple: it does
not prove independent output/allocation/ring identity checking. Arbitrary captured
bytes are abstracted as validation success/failure; immutable owned storage is a
premise, not a proof of the Rust copier. Prepared data remains private until a
whole scene is ready. An exact callback is abstracted by the outstanding head
slot; stale callback parsing is not modeled.

Enabled properties: type bounds; credit conservation including accepted copies;
physical worker bounds across epochs; no terminal settlement before reader
handoff; no source reuse before read completion and notification; bounded
reserved-plus-queued response records; current stream binding; fresh submit;
complete scene submission; Prepared before Presented; journal epoch isolation;
no predecessor free while any modeled head still displays it.

The local outbound queue is FIFO. Delivery may be delayed arbitrarily in safety
runs. The close record has an independent reservation. Capacity counts records,
not bytes. The original predecessor is the only backing reclaimed here; per-head
visibility conservatively stands in for its remaining physical leases. It is
not a claim that cached GL textures hold CPU resource leases.

## Hunts and progress assumptions

All safety configurations allow cancellation, validation failure, close, facts
change, revoke/replacement and stalled journal delivery between actions. The
two-frame runs exercise reservation contention and old-epoch cleanup against a
replacement epoch. Mirror exercises partial physical progress with two heads.

Mutation controls remove copy charging, permit source release while reading,
skip complete preparation, skip fresh submission authority, reclaim predecessor
early, settle cancelled copies before handoff, or silently retarget a stream.
Each must violate its named property; a passing mutation is a test failure.

Liveness is checked separately for one frame/head with weak fairness for worker
completion/validation, renderer handoff/preparation, publication, submission,
matching flip, terminal transfer and queue delivery. An accepted transaction
eventually settles and its reserved response obligations drain. This assumes
the backend/worker/journal eventually respond; no deadline or bounded latency is
proved. Safety runs make no fairness assumption.

## Explicitly outside this first model

No same-epoch close/drain/reopen, clock arithmetic/TTL/rate guarantees, fragmented
wire decoder, byte/resident budgets, arbitrary resource graphs, supersession,
predecessor intent conflicts/headroom, general accepted-resource reclamation,
independent identity-field substitution, stale callback decoder, actual memory
copy, GL/DRM fences, process death or physical multihead simultaneity. Reuse the
existing proofs/tests where applicable and add refinement/integration tests for
these boundaries before enabling the protocol. The finite IDs do not prove
unbounded stream operation or freedom from implementation memory leaks.

No implementation traces exist. Do not describe this checkpoint as Specula
trace-validation convergence, runtime conformance, deployed-code bug finding,
security certification or a measured optimization.
