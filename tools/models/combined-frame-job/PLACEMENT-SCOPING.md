# Stage 4b scope: facade placement is a production change, not a test-only seam

Status: read-only scoping at aa1de21e4, written after Codex went offline on
2026-11-10. No source edit, build, TLC run, push or native launch. It proposes
the exact production diff that stage 4b needs and asks niltempus for the scope
decision; it does not implement it. CPU/frame and security remain priorities;
no service-count or CPU benefit follows from this note.

## What the accepted contract already fixes

TRANSFER.md rev 3 routes combined work through a dedicated bounded(1) input
channel and a dedicated bounded(1) result channel per output. FALLBACK.md
step 1 places a dedicated combined retained-Release cell in
NativeGbmRendererWorker, reserved with admission, one per output, separate from
the ordinary discarded_release Option, with flush_discarded_release's
Full-retains semantics and ownership moving to the command queue only after a
successful send. OwnerTurn.tla models Retain (row -> cell), FlushRelease (cell
-> channel, Full keeps the cell), and CreditProtected (credit clears only once
the lease is in cell, channel or disposed). Stage 3 proved that accounting on a
harness-owned sidecar with zero src lines; stage 4a proved the validator wake
ordering.

## Source facts at aa1de21e4

worker.rs is 999 lines against the 1000 cap. The facade's ordinary cell is
`discarded_release: Option<DiscardedWorkerLease>` (:405, init :377). poll()
flushes it only in the idle branch (:605) and, when quarantined, drains one
late result into it (:607-611); with a request in flight nothing is flushed.
release_discarded_result (:768-782) and flush_discarded_release (:784-800) are
called from poll's misroute and mismatch arms (:652, :671) and from the idle
branch; the only other callers are tests. submit_reserved_capture refuses on
quarantined, in_flight or stalled (:557) and never consults a release cell.
poll's result drain is tied to in_flight: a result arriving on result_receiver
with nothing in flight and no quarantine is not read at all. Production poll
call sites: discovery/worker_export.rs:21, worker/maintenance.rs:145,
worker/preview_recovery.rs:19. The file already splits for size with a textual
include (worker/damage.rs, 251 lines) whose comment says the split exists for
file size, not for a boundary.

## Consequence for routing

Because the accepted topology gives combined results their own channel, poll()
does not route combined results and must not learn a second in_flight kind.
The combined result drain is owner-turn code on the dedicated result receiver,
as stage 3 drains raw results today. Stage 4b therefore reduces to three
production facts: the cell lives in the facade, poll flushes it at its real
flush points, and admission refuses an occupied cell. The dedicated result
channel itself belongs to the renderer service side (run_worker) and to the
runtime/backend join, which is stage 4c.

## Why a cfg(test)-only 4b is not worth buying

A cfg(test) field plus a cfg(test) branch inside poll would prove only that a
flush can be coded at that line in a test build. The production poll would stay
unchanged, the layout checker would need two new inline-tests exceptions for
worker.rs, and the file would sit at the cap. Stage 3's sidecar already proves
every accounting property such a branch could test. The remaining value of 4b
is precisely that production poll flushes the cell, so 4b is a production diff
or nothing.

## Proposed production diff, for decision, not yet written

Ordered so each step is reviewable on its own and the file never passes 1000
lines:

1. Behavior-preserving extraction, byte-moved: release_discarded_result,
   flush_discarded_release and `struct DiscardedWorkerLease` leave worker.rs
   into worker/release.rs through the existing include! pattern (same comment
   rationale as damage.rs). 34 + 5 lines out, 1 include line in; worker.rs
   about 961 lines. No behavior change; the existing renderer-worker tests and
   combined_frame_release tests are the controls.
2. Field `combined_release: Option<DiscardedWorkerLease>` and its None init
   (2 lines). A second counter `combined_release_enqueue_failures` on the core
   beside release_enqueue_failures, so ordinary and combined failures never
   conflate (stage 3 S3).
3. Flush in poll: `self.flush_combined_release()` beside the ordinary flush.
   Decision inside the decision: idle-only (mirrors the ordinary cell) or at
   the top of poll before the in_flight take (busy and idle, which OwnerTurn's
   FlushRelease permits and which Full-retained Releases need while an
   ordinary render is out). Recommendation: top of poll for the combined cell
   only; leave the ordinary cell's timing as it is, because its contents
   arise only in quarantine or misroute states that are idle anyway.
4. Two owner-callable methods in release.rs, pub(crate) like
   submit_reserved_capture: `reserve_combined_release()` refusing when the cell
   is not Free or the facade is quarantined, stalled, in flight or detached,
   and `retain_combined_result(WorkerResult) -> Option<WorkerResult>` with
   stage 3's identity, capacity, then Exported checks, returning every refused
   result intact. These are the stage 3 sidecar methods moved onto the facade;
   their seven tests move with them and the sidecar file is deleted.
5. No change to submit_reserved_capture: ordinary renders proceed while the
   cell holds a Release; the slot pool, not the cell, bounds buffers. Combined
   admission is the only caller of reserve_combined_release.

Estimated reach: worker.rs (steps 1-3, net about -30 lines), new
worker/release.rs (about 90 lines including the moved code), core struct and
its constructor for the counter, renderer_worker.rs test mount for the moved
tests, docs/source-layout-exceptions.txt unchanged (no cfg(test) glue in src),
tools/models/combined-frame-job notes. No wire, feature, dependency, runtime
crate or Cargo change.

## What this does not establish

No combined job can reach the cell until 4c joins the runtime validation
credit to the backend disposition and until the renderer service owns a
dedicated combined input/result pair. Enqueue is never slot freedom;
Disconnected stays unqualified until retirement proof; GPU lease buffer
teardown is unproved. None of this touches the three-service owner, the
fallback latch, scene policy, native launch, timing or CPU.

## Decision requested from niltempus

Stages 1-4a stayed inside test-only seams by rule. Step 1 above is a pure move
and steps 2-5 are the first non-test source change of this lane. Options:

- (a) Authorize steps 1-5 as one reviewed production diff on the research
  branch, frozen and signed before any build, validated with the existing
  isolated recipe. With Codex offline there is no second reviewer; the diff
  would wait for either Codex's return or niltempus's own read before merge.
- (b) Authorize step 1 only now, as room-making prep, and defer 2-5.
- (c) Stop the lane here at the test-only boundary and record it.

Recommendation: (a) if the frame-job line is still wanted, because the cell
is the first piece of the composed design that production code must own and
nothing device-free remains to learn about it; otherwise (c). (b) buys room
without a selected use and is not recommended on its own.
