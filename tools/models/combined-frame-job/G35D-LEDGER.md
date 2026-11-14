# G3-5d acceptance ledger

The scoped acceptances of the G3-5d device-free prototype stages, by
Codex under niltempus's authority, each at the exact head named. Every
acceptance is scoped to its own evidence: none is acceptance of all of
G3-5d, of gate 3 or of production wiring. Sealed packages live under
`~/.local/state/sophia-framejob-gate3-g35d-*` and are never rewritten;
errata are carried here, not fixed in place. Session off and
native-session clippy remain UNPASSED and UNWAIVED throughout.

| Stage | Accepted head | Scope |
|---|---|---|
| d-0 | aa405fac9 | capture recheck in the accept tests |
| d-1 | dd27f4711 | scoped |
| d-2a | 2b84d6a4d | scoped |
| d-2b1 | 61cee1dec | driver controls, scoped |
| d-2b2 | a84d66434 | running controls, scoped |
| d-2b3a | 3d191911b | owner-side custody/lifecycle evidence, scoped |
| d-2b3b1 | 7e1a64d85 | owner-side fault/decline evidence (injected faults) |
| d-2b3b2 | 188318dc8 | replacement evidence, scoped; row-6 outbox-clear sub-claim HOLD |
| d-2b3c | cbe58ec27 | stub Full-debt evidence (MD1, MD2), scoped |
| d-2b3d-1 | 2200ca0b1 | publication/journal evidence, below |

## d-2b3d-1 (2026-11-13)

Accepted at 2200ca0b15589105f243f87c669542fdfb157b1d (signed G, tree
30118641e119de1387335fb6bcad522ddc42b77d), on validation-03, package
seal 1490456b11a8a7a29614cf516431c944cbca0de43a0f35efe20332d7b2508a8b,
with the unchanged reviewed harness-claude-03 (seal aefc7d6d…; run.py
d297c692…, workflow.py 3ec28c80…).

Lineage: source-01 c21642771 (HOLD F1-F4) → diagnosis-01 repair
79ae9369b → source-02 b7c532f49 (HOLD R1) → source-03 ead1ce91f
(validation-01 FAILED: cumulative count, legacy Submitted decoder,
ResourceReleased after the terminal in P3 and F3) → source-04 7892b7353
→ diagnosis-03 repair 75f185069 (validation-02 FAILED: P3's IT window;
the transport pass does not flush store events) → source-05 2200ca0b1
(diagnosis-04 clean; validation-03 complete). Validation-01 and -02 stay
sealed as FAILED.

Errata carried: validation-01's README says 37 invocation records (38);
source-04's README says the faults file is 309 lines (310);
validation-03's README says 66 invocation logs (68: 66 exits 0 plus the
two recorded baselines at 101). Diagnosis-03's attempt 2 ran on the
clean base 7892b7353 with the recorded uncommitted repair; the signed
commit 75f185069 was made afterwards and is byte-equal to that diff.

Accepted evidence: the eight device-free publication controls
(publication::{r4,p1,p2,p3}, publication::faults::{f1,f2,f3,f4p}) with
their exact observed boundaries; all 36 control counts (ss-row 69,
ss-driver 30, ss-running 26, ss-custody 19, ss-decline 12,
ss-publication 8 and the inherited runtime/backend/Session rows); the
seven list-derived library totals (runtime 259 twice; backend 376, 476,
496; Session 1139, 1211); the twelve required clippy configurations,
layout, the full gate and fmt.

- R4: status 2 commits deferred in T1; the exact owned-ring SlotReleased
  follows in T2 with source credit/releasing state checked; no new Ready
  reply in those turns; head submission is not a completed flip; no H5
  schedule verdict (not applicable).
- P1: T1 commits AllocationResult, refusal 3/AllocationLost, the
  exact-key terminal 3/8 and Closed; the first untruncated reply covers
  only sequence 11; IT commits nothing; PP2 reads the rest to handled 14.
  Supports the already-answered-read explanation for this lane, not
  journal backpressure. Observed status 12 before terminal 13; Closed
  under transaction 30.
- P2: T1 commits Submitted, status 2 and Closed; the first reply covers
  sequence 11 (Submitted, SDK-consumed); IT nothing; PP2 the acceptance
  and Closed under transaction 31, handled 13. Status 12 before Closed
  13; stale marker; lean 0. No completed presentation is claimed.
- P3: status 2 and the Timeout terminal 3/6 covered together by the
  untruncated first reply, consumed in PP1; IT nothing; PP2 nothing; the
  exact content-store release and the owned-ring obligation persist
  through O3; one fresh before-deadline T2 commits exactly SlotReleased
  then ResourceReleased with release work/credit/publication facts
  checked and renderer custody unchanged; PP3 consumes exactly the
  identified ResourceReleased, no frame, handled 14. T2/PP3 facts are
  not T1 or IT delivery. Preconditions held in this run; no timing
  guarantee under arbitrary load.
- F1/F2/F3: the declared fault/decline paths show the exact
  RendererFailed/Malformed/RendererFailed terminals and status facts;
  F2's first reply carries only the refusal (11), PP2 the terminal (12);
  F3's T2 commits SlotReleased plus ResourceReleased after the slot-free
  observation. F2's Mismatch-to-Err(Malformed) conversion stays
  source-only.
- F4-P: exact internal prepared candidate, no Prepared record in its
  complete window, deferred status, no surfaced peer records. H5 Absent
  means no new Ready reply under the witness; no-Prepared is a separate
  window assertion. The presentation outcome is not accepted (b3d-2
  F4-O).

Literal H5: P1/P2/P3/F1/F2/F3 Supported (untruncated true); F4-P
Absent; R4 not applicable. "Untruncated" refers to the visible end when
the witnessed reply was produced. Delivery ordering is per queue; no
cross-kind dequeue-order claim. Aggregate invocations establish their
assertions, not unseen diagnostic output.

Limits: the owned-ring SlotReleased (source-slot obligation), the
content-store ResourceReleased (backing retirement) and the renderer
lease (slot freedom) stay distinct; backing destruction is not directly
observed; a journal commit is neither peer consumption nor a renderer
ACK. The later-turn windows require a fresh b3d-2 filler record/credit
accounting trace and approve neither the old +3 count nor a replacement.
MD7 and its consumers, full-journal capacity and admission-stop
controls, F4-O declared completion, backpressure and the naturally
nonempty outbox/disconnect clear remain b3d-2; b3e, b4 and the final
integrated gate-3 acceptance remain outstanding. No mutant, TLC, native,
GPU, DRM, real-flip or performance evidence was added. No execution,
merge, push, default change or gate-4 source authority follows.

## d-2b3d-2 (in progress)

Source-01 at 6900f8e13 was HELD (R1: a manual-ACK peer cannot issue
successive non-lean requests; R2 cumulative release counts; R3 the
missing final FJ-stop filler; R4 the reused wake; R5 budget checks
outside the admission loop; R6 BP occupancy arithmetic under automatic
ACKs). The fill design went through REV7 (baseline ACK A_B, no spare
allocations reachable device-free), REV8-A1 (a pending replace request
is consumed by the first content step; a lost allocation aborts without
a ResourceReleased; FJ-stop settled as the parked final release),
REV9-A2 (the images-step order: a filler's T2 publishes its
SlotReleased and admits the staged next frame before the Resources
stage flushes its ResourceReleased, so the last filler must start at
250) and REV10-A3 (PASS for source preparation: the prefix is two lean
frames of INVALID pixels, +3 visible records each, no renderer
involvement, the Malformed terminal returning the stream credit). The
revision over 6900f8e13 carries source-01 R2-R6 and REV10's
requirements: journal-local budget-checked admission and settlement
loops, a fresh wake boundary before every submission, cumulative
counters, the full T2(65) with four parked records, the ACK joined to
the observed frontier (two cases), the FJ-stop availability facts
through a read-only admission reader, BP on the frame's own
allocation, and the fixture's post-preflight invariant. All counts,
windows and the timing budget remain predictions until a separately
granted run.

Source-02 at 436b51c0d was HELD (R1: `deadline_of` after the C3
decline reaches its expect, the row entry having been removed; R2:
the FJ-stop stop witness and its availability reader read the
ordinary `Control::Offer` field, which the lean frame commit never
writes, so the probe could not break on the Stop and the reader
returned `None` in that state; baseline: one automatic-ACK pass with
R_B asserted 0 rather than the explicit covering A_B). Source-03 over
436b51c0d: `Admitted.late` is read at admission and reused by T1, T2
and the FJ-stop T2(64), never refreshed; `queued_frame_facts`
(test-support, `experimental-native-frame-transaction`) reads the
queued frame commit's transaction, candidate generation and embedded
offer with the admission's `owned_offer_available`, grant check and
control room, `None` without such a commit; FJ-stop breaks on the
identified frame 65 commit, runs one more turn in which it stays
queued with the committed boundary unchanged, asserts the ordinary
offer queue empty and the copy owner idle, and asserts the commit
consumed by the admission after the ACK; the baseline performs REV7
section 2's A_B (handled == next_sequence − 1, deferred None, manual,
`acknowledge_journal`, one input-serving turn, acknowledged == queued
== handled confirmed, R_B == 0, floor == tail). FJ-stop moved to
combined_journal_stop.rs for the file-size limit. Still unexecuted.

Validation-01 of source-03 at e973c7b22 ran once and FAILED at the
ss-row invocation (71 passed, 2 failed, 73 total): FJ stopped at
prefix 2's O0 assertion with 5 retained records against the predicted
3, the two new records unidentified; FJ-stop ran fillers 1 to 4 with
the predicted windows and counts through 16 and then `submit_ring` for
filler 5 returned `Image(Busy)`. The read-only diagnosis traced the
Busy to the vendored producer's four offer records: the exact
`SlotReleased` frees a slot and leaves the record Settled or Refused,
only `image_forget` frees the record, and the fixture never forgot
one. Codex carried two errata on that note: fillers 3 and 4 passing
shows that some recycling occurred, not that both earlier releases
were applied; and an owner-side journal window establishes commitment,
not client decoding. Source-04 over e973c7b22 has the two directed
parts. (a) Diagnostics at the existing observation boundaries: before
each submission the peer's progress and every live offer's record;
across each admission (the two prefixes, every filler, frame 65's
T2(64), the FJ-stop first stop and its admission after the ACK) the
appended entries, both journals, the outbox, the owned credits, the
releasing list, the stores, the row, the worker and the peer's
progress, printed before the unchanged assertions, with no added turn,
poll, ACK or wait. (b) The client offer lifecycle, local to the fill
controls: `Fill` carries the live offers; the offer id `submit_at`
returns is checked Pending with the frame's resource identity; a
filler's offer is forgotten at the end of its T2 after the peer's read
of its SlotReleased and ResourceReleased, with the record required
Settled; the INVALID prefix's offer is forgotten after a later read
finds it Refused; Pending and Unknown are refused, a forget must
succeed and leave the record absent; the staged or blocked frame 65
stays live. The prefix-2 count remains a prediction to be observed,
not revised. Still unexecuted; compiler-only diagnosis and validation
each need their own grant.
