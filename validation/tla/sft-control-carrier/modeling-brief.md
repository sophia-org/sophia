# Modeling brief: bounded SFT control carrier

Category B (concurrent shared-memory publication), with a message-custody boundary.
New carrier actions are proposed, not annotations of existing runtime code.

## Source anchors

- `validation/tla/frame-submit/LeanPrefix.tla`: Deliver/Receive distinguish host
  ACK application, admission, and client confirmation; lost reply disconnects.
- `crates/sophia-runtime/src/shell_transport/files/export/transaction.rs:20`:
  live reusable token, complete decoding, epoch, prefix ACK, submission, exact
  returned byte count. These checks must remain common to the carrier treatment.
- `crates/sophia-runtime/src/authenticated_files.rs:277`: per-operation check;
  with_delivery checks again before guarded delivery. Guarded Rread uses a fresh delivery gate; not every Rwrite does. The proposed
  CQ publication gate is additional conservative candidate work, not an
  assertion of source equivalence.
- `crates/sophia-runtime/src/shell_transport/files.rs`: bounded 9P server and
  journal reservations. The probe must not claim a win by dropping these costs.
- `crates/sophia-wake/src/lib.rs`: clear BEFORE queue inspection, publication
  before notification. Shared owner wake remains untouched.
- `crates/sophia-linux-peer/src/producer_ring.rs`: size seals, future-write seal,
  mapping ownership and DONTFORK. This is a pixel producer API, not a safe
  concurrent control-ring consumer to reuse without a new memory-safety proof.

## Trust and proposed linearization

Client input is hostile. Bounds checked from a private snapshot are the only
input to decoding; no head/tail value grants authority. Capture is not acceptance.
The candidate acceptance point remains the existing complete-write custody
boundary. Receipt publication is a separate observable boundary requiring fresh
publication authority. Client knowledge changes only on that receipt. Publication
may precede client observation, as a socket reply may already be buffered.

A receipt must identify the exact outstanding attempt, including attach/writer
binding, epoch and submission; retry identity follows the existing rule that the
lean ACK word is not payload identity. Receipt slots and journal slots have
separate reservation/accounting obligations. Neither queue head can raise the
semantic ACK frontier. Old mappings never regain authority on a new attach.

## Verification plan

Start with the two small projections in README; then close its listed gaps.
Every unsafe shared-memory operation requires a separate code-level argument
for size seals, address arithmetic, lifetime, peer write access, snapshotting
and memory ordering. TLC's sequentially consistent abstraction is not that proof.
Use deterministic interleaving tests and negative controls before performance.
A real 9P control and ring candidate must share decode/authority/state work;
any missing production obligation is an explicit limit, not free transport saving.

Prototype and measurements remain blocked on completing the model/contract gate.
No physical output, rendering correctness, safe mapping, latency or CPU claim
is derived from this brief or its initial projections.


## Scope-resolved handoff

Commit-only is selected. No journal/CQ migration: the full journal, offsets,
publication hold and covering ACK implementation remain common. QueueIndex,
CommitOnly and ExitWake cover the newly selected boundary at their stated finite
abstraction. README maps each new invariant to a positive configuration and a
negative control. Concrete arithmetic, bytes, deadlines, short I/O and syscall
mapping remain mandatory executable proofs, not model assumptions to hide.
