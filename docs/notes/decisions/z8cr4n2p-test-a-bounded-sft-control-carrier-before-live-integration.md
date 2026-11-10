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

Reuse the existing complete record grammar and `SHELL_FILE_MAX_TRANSACTION_BYTES` upper bound (65,536 bytes). Snapshot
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

The commit-only/pread-first scope and bounded model review clear device-free
prototype correctness work. The first standalone mechanisms and their remaining
production-parity gate are recorded below and in the probe README. No transport
comparison or saving is established. The ring-01 source disposition remains
independent; frozen historical evidence is unchanged.

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

## Model accepted; first executable mechanisms, November 9

Claude's review at 2d5d5b493 (seal f821f765) clears standalone device-free
correctness work. Its three modeling limits are now explicit in the model
README: wiring controls are not behavioral proofs, EALREADY/legitimate epoch-2
processing are unmodeled, and same-attach writer reopen needs executable proof.
The models remain separate finite projections, not implementation refinement.

The [probe README](../../../tools/probes/sft_control_ring/README.md) records the
one-deep CQ choice, bounded private-copy/receipt/ACK/refusal tests and a real
two-process memfd exchange. The custody module is a test projection with supplied
authority and ACK effects, not a host path. Actual 9P/journal parity and persistent
carrier servicing are still required before timing. No production adoption,
source disposition, native run or performance decision is made at this checkpoint.

## Real-export seam and receipt-pipe amendment, November 9

Mechanism review cee06b68 at 350ce8c3d accepts the standalone checkpoint and
permits a cfg(test)-only seam beside the existing runtime export proofs. The
follow-up deletes the supplied admission owner and calls the real reusable-writer
check and FrameSubmit export. Production src items and APIs are unchanged;
negotiation is supplied by fixtures, so authenticated wrapper parity remains open.

The current response carrier supersedes the earlier shared CQ: one exact 80-byte
receipt travels directly over an owner-created nonblocking directional pipe.
Pwrite cannot maintain a reusable peer-write-sealed CQ: FUTURE_WRITE also blocks
host writes, while a size-sealed read-only fd can be reopened writable through
/proc. A writable host mapping would require its own production decision; this
probe avoids it. A peer can inject receipt lies into its own reader by reopening
it, but that cannot change host custody. No peer-supplied response grants authority.

SQ publication is an eight-byte sequence hint on the other directed pipe. The
owner classifies it before pread: expected next permits capture, one duplicate
last hint is idle without touching the SQ, and further duplicates/gaps/short
records/errors close. The header must match the hint. The client keeps the SQ
immutable until its receipt; no host mapping or shared CQ consumer head remains.
Full/short/error writes are not delivery and close without undoing accepted work.
The pending response reservation still precedes ACK effects. Receipt transport
is still independent of the 9P journal and its covering ACK.

Real-export tests cover retry/backpressure, changed payload, duplicate poisoning,
expiry/clunk/reopen, ACK-before-refusal and journal holds. A persistent three-attempt
child fixture exercises both directed pipes, duplicate hints and peer-exit wake.
The [probe README](../../../tools/probes/sft_control_ring/README.md) records gaps:
complete socket-framed/authenticated parity, withdrawal/replacement and lifecycle
composition still precede measurement. Next is a matched counts-only census of
both complete paths. Static carrier I/O is 3–4 host calls per attempt before
poll/authority work, not a syscall reduction claim. No timing, native run or
integration is selected by this correctness checkpoint.

Review 6f59474c accepts 52ea995d4 as device-free correctness evidence, with
full authenticated/socket-framed parity still open. Its writer-binding and HUP
clarifications are in the probe README. The next client-only lifecycle follow-up
marks an attempt published only after its hint write succeeds; failed publication
closes as unpublished instead of unnecessarily unknown. This does not grant a
replay on the closed carrier or weaken host custody.

## Authenticated adapter challenge, November 9

Challenge 4471e1d4 accepts a cfg(test)-only binding and real authenticated-wrapper
join within step 3. Connection/epoch come from host adopt/attach, not the SQ;
write access is bound during setup and reusable-writer expiry is checked on every
dequeue. The current probe adds a real socket/Pipeline control and supplied-owner
check counters. No production accessor, carrier API or custody scope is added.

The fresh receipt gate is deliberately stricter than **all** existing shell
replies: ShellFiles does not enable guard_read_delivery, including for Rread.
An already-owed lean Rwrite may flush after revoke; a carrier receipt must not.
The candidate uses AuthenticatedFiles::with_delivery around its actual receipt
write. The README records the extra inner check and the remaining binding and
persistent lifecycle gaps. This checkpoint does not establish full carrier
parity, authorize integration or select a census/timing/native run.

## Test-only lifecycle composition, November 9

Adapter 4f12ca40a is accepted (probe-04 seal e502a429, independent review
5f6e1ec9, acceptance 1898d844). The next bounded test joins its existing
check/process/with_delivery path to the actual child SQ/hint/receipt mechanism.
Normal receipt, withdrawal after processing and accept-then-peer-exit are tested
with retained owner work; the exit is synchronized over the control stream before
pidfd observation and transport closure. Writer clunk between processing and
delivery preserves accepted custody; a subsequent attempt refuses the lost token.

The 9P attach, writer and FrameSubmit fids still belong to the harness Pipeline
in the owner process. The child owns only carrier endpoints. This is adapter
composition, not binding to the child's own authenticated attach. The source
challenge in sophia-control-carrier-lifecycle-challenge-claude-01 records this
limit and the three distinct exit orderings. No production source/API changes,
real custody proof, census, timing, native run or integration are selected.

## Lifecycle acceptance and next comparison gate

Checkpoint `4cb4c9bc3` is accepted as test-only composition: probe-05 seal
`d3cac90d`, independent review `d23d1d46`, acceptance `d6618dd5`. Normal receipts,
withdrawal after processing, synchronized accept-then-exit, and writer clunk after
processing retain owner custody as specified. The harness still owns the attach
and fids; supplied verdicts are not actual grant-teardown proof. Complete child
binding, FrameSubmit lifetime/replacement and journal composition before the
matched counts-only comparison. Preserve the intentionally stricter receipt gate
in the candidate and report its checks; do not silently equalize it away.

The approved continuation retains separate SDK and immutable-image investigations
under 4rifczhw. Neither is an additional carrier treatment or permission to move
journal events onto a queue. No production API or native run is selected.

## Binding seam limit

Consult `sophia-control-carrier-binding-consult-claude-01` (amended seal
`01bfc758`) finds no export-visible FrameSubmit fid identity: Connection owns
clunk/replacement checks and ShellFiles ignores Plain release. The child-owned
fixture can prove actual attach teardown and reusable-writer lifetime, plus
characterize socket EBADF/replacement, but cannot prove absent carrier coupling.
Do not add fixture flags and call them parity. Production integration must select
an export-visible registration lifetime with its wire/allocation and authority
review. No such production change is selected here. Whole-attach revoke clears
queued proposals; carrier-only pipe closure with a live attach does not. The
probe README records both distinct assertions.

## Child-attach acceptance and census scope

`49876afa7` is accepted as test-only child-attach/journal composition: probe-06
`71f42c66`, independent review `54296aaa`. It retains supplied authentication and
the export-visible FrameSubmit lifetime limit above. No production or teardown
qualification follows. The first full-gate EAGAIN failure remains recorded on the
same tree; the second passed, with nondeterminism rather than branch independence
claimed.

The counts-only screen uses the same owner poll array for actual socket framing,
split pread and full-extent pread, three sequential records per invocation.
Authentication checks and raw-Pipeline peer counts are harness observations;
9P events/ACK traffic is excluded from this commit-only scope. The constant-based
implementation always allowed 65,536 bytes; earlier numeric prose was stale.
Full extent is retained as a byte-amplification contrast, not advanced to CPU
measurement. Counts do not decide dispatch cost. Next is an untraced socket-versus-
split owner-service CPU screen with explicit test-owned draining; only a repeatable
reduction warrants designing the larger matched comparison. No native or
integration selection follows automatically.
