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

Reuse the existing complete record grammar and `SHELL_FILE_MAX_TRANSACTION_BYTES` upper bound (8192 bytes). Snapshot
untrusted metadata and bytes into bounded private storage before decoding. Never
lend a Rust reference into peer-mutable input. Validate the private snapshot;
producer generation words cannot authenticate it. Reserve space for the receipt
before any ACK/admission effect, and retain the existing journal reservations.
The lean ACK prefix may be applied before a subsequent admission refusal, just
as today; only the matching successful response confirms it to the client.

One outstanding frame; one ordered journal consumer per attach. An SQ cell can be reclaimed after complete private capture; that is not reuse
of an accepted semantic submission or its pixels. Completion ring
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

## Contract challenge and prototype hold, November 9

Claude's read-only challenge is retained at
`~/.local/state/sophia-control-carrier-contract-challenge-claude-01`, seal
`29e08b85e8005c47f5667302f4a7b0780e59f8fa8057498007dc197b3496520b`.
It permits model/brief work but does not clear the prototype gate.

The conservative read method is **pread into private bounded storage**, with
short reads/errors refused and the syscall included in measurements. Mapping
peer-writable input in the production host is not selected. A mapped probe arm
would require its own memory-safety argument and convey no host authority.

The covered exchange set still needs a recorded choice. Recommendation:
**commit-only**: replace FrameCommit Twrite/Rwrite with SQ and a CQ receipt;
keep terminal/source/resource/refusal journal records on the 9P events file.
This leaves the existing per-frame Tread/Rread and its offset, visibility,
publication-hold and ACK semantics intact. Moving those events onto CQ is a
larger alternative requiring an explicit events-contract decision. No prototype
or performance comparison starts while the covered exchange set is ambiguous.

The frozen implementation contract must also include these obligations:

- A success receipt echoes the exact applied ACK word, in addition to the
  attach/writer, epoch, submission and byte-count binding. Retry payload identity
  excludes the ACK word, but receipt/attempt identity must distinguish it.
- EAGAIN is a retryable receipt, not a definitive refusal. It consumes the SQ
  cell but advances no submission watermark; the same submission may be
  republished. The original staging identity/deadline must be preserved according
  to the existing reusable-writer path, rather than reset by each retry.
  Its ACK remains unconfirmed even if already applied. Other errors retain their
  existing definitive/unknown-custody classification.
- Reserve a CQ receipt slot before `apply_ack`, including its recycling and image
  refusal retry effects, not merely before admission. Preserve independent
  journal/terminal reservations.
- A receipt is not a journal record: no journal sequence, terminal reserve or
  covering ACK. Journal events alone do not establish lean file custody.
- Bind the carrier to one authenticated attach and its owning fid/writer. Each
  dequeue re-runs expiry/token, revision, epoch, lean-mode and caller-fresh checks;
  ring activity cannot refresh or extend the writer's deadline implicitly.
- Notification is a descriptor in the existing owner poll set, alongside exit
  and control readiness. No futex waiter, helper thread or second owner loop.
- One ACK-bearing operation may be unanswered across both carriers. Published
  without its receipt is unanswered; standalone ACK behavior remains unchanged.
- Input captured after revocation causes no response publication or state effect;
  receipts cannot be delivered into a replacement grant. Publication requires a
  fresh delivery gate. Already-published replies and unknown custody require
  distinct model states; the initial Carrier projection does not cover them.

A positive comparison does not authorize integration. Integration requires an
accepted decision, wire-contract/allocation-ledger amendments and custody
review under 4rifczhw, followed by its separately selected qualification.

## Scope selected by niltempus, November 9

niltempus answered "proceed with your recommendation": select **commit-only**
and **pread-first**. FrameCommit Twrite/Rwrite alone is the treatment; CQ contains
only its transport receipts. Every journal event and standalone ACK remains on
9P with the existing ordered reader, offsets, visibility/hold and release rules.
The exchange-set hold is resolved. Model/implementation proof and bounded review
still precede a measured probe. Journal-on-CQ and host mapping are unselected.
