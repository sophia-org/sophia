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
