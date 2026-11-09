---
id: z8cr4n2p
date: 2026-11-09
kind: adr
status: proposed
tags: [adr, sft, performance, portability, custody]
---
# Test a bounded SFT control carrier before live integration

## Selection and limits

niltempus selected the device-free prototype first, then requested implementation.
This admits documentation, a custody/wakeup model, an isolated Linux probe and a
matched transport comparison. It does not admit a production transport, protocol
identifier, native launch, SDK/Lom integration or adoption. The contract below
remains proposed pending review and model checking.

Reuse the [portable interface direction](../concepts/fp9o6pdb-portable-plan-9-interfaces-with-native-frame-mechanisms.md)
and [Redox research](../investigations/sy7lhvb8-which-redox-ring-and-orbital-patterns-can-reduce-sophia-frame-overhead-safely.md).
9P remains setup, authentication, configuration and recovery. Current SFT is
unchanged. No URL grammar, global broker, Redox dependency, allocator replacement
or general platform framework is selected. t344 owns this experiment; t256 owns
actual platform qualification. Linux mechanisms are not portable guarantees.

## Proposed custody boundary

The existing lean path establishes known file custody from exact Rwrite. A ring
head advancing proves neither acceptance nor ACK. The probe must model a separate
owner-authored receipt bound to attach epoch, submission identity and exact byte
count, with success or a definitive refusal. Success means the privately captured
record reached the same accepted-custody boundary as complete-write submission;
it does not mean candidate admission, pixels released, Prepared or Presented.

Reuse the existing complete record grammar and 8192-byte upper bound. Snapshot
untrusted metadata and bytes into bounded private storage before decoding. Never
lend a Rust reference into peer-mutable input. Validate the private snapshot;
producer generation words cannot authenticate it. Reserve space for the receipt
before any ACK/admission effect, and retain the existing journal reservations.
The lean ACK prefix may be applied before a subsequent admission refusal, just
as today; only the matching successful response confirms it to the client.

One outstanding frame; one ordered journal consumer per attach. Completion ring
transport reclaim is separate from the journal's covering cumulative ACK. Idle
ACK progress remains available on 9P. Queue consumption cannot return frame
credit, release a source, retire a predecessor or authorize another grant.
Disconnect with no definitive receipt is unknown custody, never a replay license.
No transport switch while accepted work or uncertain custody remains.

Keep every caller-fresh authority check, request/delivery refusal and phase order.
M1/M2 remain deferred, B6 retained; T4/M3 remain parked. A replacement carrier may
not use a cached attach or a shared-memory flag instead of those checks. Any
inability to preserve that boundary stops the prototype for a named authority
decision. Four dependent owner services are not predicted to disappear.

## Implementation sequence

1. Freeze this contract and a source-mapped modeling brief; obtain Claude's bounded
   challenge via herdr. Model bounded capacity, private snapshot, acceptance
   knowledge, ACK, epochs, disconnect, wake/arm races and fairness assumptions.
2. Check safety/progress and guard-removal controls. This is a finite design
   projection, not Rust memory-order or implementation refinement evidence.
3. Only after that gate, implement tools/probes/sft_control_ring as a standalone
   Linux two-process probe, using the existing tools/probes/d4_owned_ring layout.
   Compare actual 9P/socket framing and bounded SQ/CQ with identical SFT messages,
   decoding and lifecycle stimuli. Keep simulated completion explicitly labelled.
   No production APIs change; unsafe mapping code stays in the isolated probe.
4. Test hostile indices/lengths/mutation, full queues, wraparound, stale epochs,
   duplicates, lost replies, death, revocation, idle ACK and notification races.
   Preserve bounded servicing, no spinning or deliberately delayed batching.
5. Alternate carrier order for paced 60 Hz and separate throughput screens;
   report setup separately, per-process/combined CPU per transaction, latency,
   allocations, bytes and wake/syscall counts. Tracing runs are separate from
   timing. No multiplication into native CPU savings or detection-floor claim.
6. Review the evidence before selecting host/SDK integration. An inconclusive
   experiment ends without automatic tuning, native reruns or adoption.

## Current checkpoint

Contract drafted; review requested. Initial [custody/wake projections](../../../validation/tla/sft-control-carrier/README.md) exist, with their remaining gate explicitly listed. No complete model gate, executable carrier, transport comparison or saving is established yet. The existing ring-01 source disposition
remains independent. Frozen historical evidence must not be rewritten.
