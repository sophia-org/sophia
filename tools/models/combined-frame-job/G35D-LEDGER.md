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
| d-2b3d-2 | c8f5efc9a | FJ, FJ-stop, F4-O declared completion, BP, below |
| d-2b3e | ff34d204a | three capture forms; lock-test repair, below |
| b4 | 0fd5ac842 | trace recorder/checker, integrated rerun; TLC at a4d8036a4, below |

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

## d-2b3d-2, d-2b3e and b4 (2026-11-13 to 2026-11-14)

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

Source-04 at 99d60e5f9 was HELD on two semantic corrections (R1: the
INVALID offer was required to exist after the sweep that forgets a
Refused one, so the completed-refusal case the control accepts would
fail; R2: Refused alone is not the exact release, since a refused offer
write also leaves Refused without a status). Source-05 over 99d60e5f9:
the INVALID offer is read, printed and required Pending or Refused
before the sweep, and afterwards required absent if it was Refused and
still Pending otherwise; a forget requires the state and status pair
the exact release writes, Settled with Accepted for a filler and
Refused with Rejected for the prefix, the status compared by its Debug
text because the client does not re-export the kind. Prefix 2's
additional records remain unidentified. Still unexecuted.

Validation-02 of source-05 at 0200747f4 ran once and FAILED at the
ss-row invocation (72 passed, 1 failed): FJ-stop passed the whole fill,
so the offer lifecycle held; FJ passed prefix 2's admission with 3
retained records (validation-01's 5 did not recur, and the two records
remain unidentified) and failed at filler 3 with "the peer never read
the terminal". The read-only diagnosis traced it to the schedule: the
client keeps one read outstanding, the owner registers a new read only
in an input-serving pass, and filler 3's admission pass answered the
peer's read at once with prefix 2's visible SlotReleased(3), so through
T1 the server held no parked read and the peer-only wait afterwards
could never receive the terminal; the INVALID T1 loop polls the peer
before each owner pass, and FJ-stop's fillers follow a finish that
drains the journal with an input turn. Codex chose one correction:
source-06 over 0200747f4 adds exactly one peer poll before filler_t1's
owner turn, with the budget check kept immediately before the turn, no
other change. One bounded poll's success is a prediction until run.

Validation-03 of source-06 at 23211bebe ran once and FAILED at the
ss-row invocation (72 passed, 1 failed): FJ-stop passed the whole fill
again; FJ failed at prefix 2's admission window, which this time held
exactly the frame's own ResourceStatus(3, Malformed) and
FrameOutcome(3, 3), identified by transaction, resource generation,
candidate generation and stream, with the row still in flight, the
backing decharged, lean 0 and the release owed. The read-only diagnosis
traced the race: the validator returns the INVALID frame's plan and
sends its failure on the owner's completion channel, and the Images
call that admitted the frame drains that channel between its two owned
services, so when the validator wins the second owned service
reconciles the failure and the content step journals the status and
the terminal in the admission turn itself; when the owner wins, the
next turn does. The row's only exit from in flight is the collect that
opens the next Images call, so the entry reads in flight either way.
Codex approved a two-branch admission contract: A, the window empty
with the pending-source facts and the existing result loop; B, exactly
the two identified records with the settled-source facts and no loop;
every other window fails. Source-07 over 23211bebe implements it in the
prefix control, names in every message which turn produced the status
and terminal and which the SlotReleased, keeps the +3 accounting and
adds no poll, pass, ACK, delay or deadline change. MD7-FJ's expected
early kill may now land at either the admission classification or the
later result window; the harness freezes both sites. Unexecuted.

Source-07 was held on two test-contract findings: the common
post-status block joined the status by grant and resource and the
outcome by stream and candidate but neither by transaction, so wrong
transactions would have passed; and the live-credit read preceded the
window classification, so the early-SlotReleased schedule of MD7 would
have failed at the credit reader rather than at the one named
classification. Source-08 classifies the window first, at a single
catch-all assertion, before any read that presumes a live credit; the
branch-B count is an ordinary check, not an expected kill. It asserts
the status's transaction equals the offer's and the outcome's equals
the submission's, beside the existing joins, and carries the A-branch
unblocked assertion. Nothing else moved. Unexecuted.

Validation-04 ran harness-05 at source-08 and passed every row at its
predicted count; Codex accepted d-2b3d-2, scoped, at c8f5efc9a (receipt
seal aca8cd7a…, harness seal c34d62ae…). Both INVALID prefixes took
schedule A, sixty-one fillers brought the retained count to 250, frame
64's release committed with its ResourceReleased parked and the journal
blocked at 253 records with three holds, frame 65 stayed unadmitted
until the ACK through the observed SlotReleased, and the drain left the
0/4/5 retention case. FJ-stop found the backpressure stop after two
probe turns at 255 records; the first-hold refusal remains a source-order
inference. F4-O proved the publication response to a declared
completion only; BP closed the non-vacuous outbox clear for its one
sequence. MD7-R4 was detected; MD7-FJ was killed at the first prefix's
status window under schedule A and not detected, one executed outcome,
so direct full-journal mutation detection stays a named gap; the
schedule-B branch and its classification site were not executed.
Erratum carried: validation-03 was 72 passed and 1 failed (71/2 was
validation-01). No renderer ACK, slot freedom, backing destruction, real
flip, TLC or native evidence; off and native-session clippy unpassed and
unwaived.

d-2b3e source-01, over the accepted head and the REV2 design Codex
passed: the fixture records each pre-classification consumption check
where it runs, a count and the last result, so a row can say whether no
check ran, one accepted, or one refused and with which refusal; the
preflight is not recorded. Form 2 gets a declared producer, the
discard-less half of the existing capture helper, with the queued mixed
frame and the requested capture each witnessed before and after the
refusal. Three Session rows: a capture before reserve declines to the
ordinary path with no check run; an ordinary frame queued with a capture
answers OrdinaryPending ahead of CaptureActive; the two d-0 discarded-
capture rows keep their names and effects and now also read
CaptureActive. Stub fixture composition throughout; no endpoint, no
completed capture, no production change. Unexecuted.

Validation-01 of the b3e harness ran at 9b5cf6c5e and failed, not in a
capture row but in the inherited protected-provider lock test of the
session-ns-nft whole library, which asserted that one owner poll could
not both reap the old process and drain the service's retirement
marker; the poll reads the supervisor before the events, so one pass
can complete the barrier and reserve the successor, which is the
intended order. The three capture rows had passed; the mutants did not
run. The repair ff34d204a is test-only and separate from the capture
change: the control asserts the owed barrier before the poll, where it
controls the boundary, and after the poll checks the owner state that
belongs to the answer it got; the deterministic either-order control in
tests/session_lock_succession.rs stays the ordering evidence. Its
diagnosis carries two errata (a regrant implies the new epoch, not a
successful reservation; a marker before the reap leaves events admitted
while the start still waits).

Validation-02 ran harness-02 at ff34d204a and passed every row at its
frozen count (session-ns-nft 1139 = 1074 + 65 ignored, the comb library
1218 = 1153 + 65; the repaired test and the either-order integration
test both ok in the gate log); Codex accepted d-2b3e, scoped, at
ff34d204a (receipt seal 809768a8…, harness seal e5690a4e…). MC1 and MC2
were detected at their frozen observation assertions; MC3 was killed
by the bounded-nonprogress assertion and not detected by the intended
reservation and state assertions, an executed non-detection that stays
open. The acceptance covers the three capture forms on the stub fixture:
a capture before reserve goes ordinary with no check run; a capture
queued on a pending ordinary frame refuses OrdinaryPending ahead of
CaptureActive; a capture left after the discard refuses CaptureActive in
the unchanged d-0 rows. No endpoint, native flip, renderer
acknowledgement or slot freedom follows. Validation-01 stays failed;
off and native-session clippy unpassed and unwaived; MD7's full-journal
non-detection and final-head rerun stay with b4.

b4 recorded the row's calls and checked their order. Source-01 (a020dfafd)
added a test-only trace to the row, one event per call named by the
admission's key, a replay checker over it, thirteen synthetic rows and a
checkpoint at the end of every combined control; it was held for three
rule gaps (a Collected without the collection's own identity, a
no-admission join that could carry a verdict, walk branches without the
grant they read), which source-02 (105c26646) closed; its three
compiler/lint attempts left the signed repair a4d8036a4, byte-equal to
the third attempt's diff, ruled compiler-only (the synthetic rows'
"Full" became the enum's Pending). At that head the 24 TLC pairs of the
five design projections ran once under their own frozen runner and every
pair gave exactly its frozen finding (tlc-01, PASSED, accepted as
design-projection evidence and nothing more). The integrated harness
(62 mutant cases, every stage mutant re-frozen at the head plus four of
the recorder and checker) was held once for a survivor rule that never
read the consumer's own ok line and a same-file site frozen in the clean
text, then granted; its validation-01 ran at a4d8036a4 and failed at the
combined row's filtered run, 86 of 89 passed, each failure a b4
checkpoint whose expected end state was wrong, not a control, a driver,
a fixture or production: the declared-head loop's owner-replaced
iteration keeps its declined job as Debt(NotFound), since no exporter
issues the head once the native owner is replaced, while the two other
iterations transfer and remove; the later-generation retirement control
ends with the successor's own admission still in flight, since the stub
produced no result for it and the loop stopped on the old job's absence;
and the queued-frame projection control joins nothing, since the lane's
helper prepares the frame through the fixture and never calls
submit_bundle. Source-03 corrects the three expectations in place (the
owner-replaced iteration asserts its zero transfers and the row's kept
NotFound beside the retained set; the successor is read from its own
Bound event by grant, output and candidate; the projection control asks
for exactly zero joins) and changes nothing else. Validation-01 stays
failed; the mutants, libraries and gate of the integrated run are still
unexecuted at this head; off and native-session clippy unpassed and
unwaived.

Diagnosis-02 compiled source-03 clean (eleven rows, empty diff, no
repair). Harness-03 re-pinned harness-02 and relocated four d-0 consumer
sites by text; its validation-02 ran every control, library and restored
control green and all 62 mutants, six of them killed at their frozen
file:line but missed by the frozen text (three metrics regexes cut at a
fixed width and then end-anchored, one marker still naming the stage-era
line, two rule texts without the replay's event context), and its gate
failed on sophia-engine's allocation-count test, unchanged since the b3e
gate that passed it. The read-only diagnosis traced the only in-process
activity outside the measured work that it could name to the libtest
main thread's first blocking receive after spawning the test thread,
unordered against the test's window, and withdrew its timing, count and
history claims under review; a twenty-run probe of the sealed gate binary
passed twenty times under its recorded conditions and ruled nothing.
Harness-04 froze the six complete sealed lines; validation-03 ran the
whole workflow green at 0fd5ac842: 145 invocations, 62 of 62 mutants as
frozen (MD7-FJ and MC3 killed without detection, MT4 surviving with its
consumer's own line), the full gate of 744 binaries and fmt. Codex
accepted b4, scoped, at 0fd5ac842 (receipt seal 519ce075b9ba…, harness-04
0e3b27934cdc…): the recorder/checker controls and finite-prefix
checkpoints, the final-head control/library/gate execution, the frozen
mutant classifications, and the TLC evidence at a4d8036a4 carried by the
byte-identity of its 29 model inputs with this tree. Validations 01 and
02 stay failed; off and native-session clippy unpassed and unwaived;
MD7-FJ's full-journal negative, MC3, MT4 and the synthetic-only Debt and
Invariant arms stay named gaps; this is scoped b4 acceptance, not final
integrated gate-3 acceptance. This ledger commit is documentation over
the tested head: no source file changes.

## Final integrated gate-3 acceptance (2026-11-14)

Codex accepted gate 3 "within the documented device-free gate-3 scope and all carried evidence limits" at the integrated source 0fd5ac8425a38e3b633d884570643c2f4d70245c (tree 91aba974e8ff7a33d560aa1172839582d4b3638d), on the package sophia-framejob-gate3-g35d-final-acceptance-claude-01, seal 7a88758dd5543cf78866b81143f0172a98544d146523a31364e5ec3406851e21: the thirteen scoped stage acceptances d-0 through b4 plus the TLC receipt, every indexed head an ancestor of the integrated head; the accepted integrated execution is b4 validation-03 (seal 519ce075b9ba…) with harness-04 (seal 0e3b27934cdc…); TLC-01 stays an execution at a4d8036a4 bridged by its 29 byte-identical inputs. Errata carried against that sealed package, not fixed in it: the d0-validation-01 disposition is gate3-g35d-d0-repair01-review; the 63 exits 101 of validation-03 are 61 mutant copies plus the two recorded off and native-session clippy rows, MT4 being among the 82 exits 0; the compiler-attempt remark applies only to repair-bearing attempts, clean-base attempts stay clean-base evidence. Unresolved and not promoted: off and native-session clippy unpassed and unwaived; MD7-FJ full-journal detection, MC3's intended detection, MT4, the synthetic-only Debt and Invariant join arms and the row-6 outbox-clear sub-claim stay gaps; the FJ frontier alternative, the FJ-stop inference, the declared F4-O completion, the distinct release domains, the capture, Disabled and stub qualifications, the lock-repair branch uncertainty and the R1 custody retention limits stand; the one historical allocation-test failure is unexplained. No native, GPU, DRM, flip, renderer-ACK, backing-destruction, timing or performance evidence and no Rust-model refinement is established. Next is a design-only freeze rebasing the gate-4 authority draft onto this head; gate-4 source and execution need their own review and grants. This entry is documentation over the tested head; no source file changes.
