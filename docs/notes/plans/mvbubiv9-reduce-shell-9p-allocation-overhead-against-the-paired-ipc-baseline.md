---
id: mvbubiv9
date: 2026-10-15
kind: plan
tags: [plan, shell, performance]
---
# Reduce shell 9P allocation overhead against the paired IPC baseline

## Scope and exit



Reduce measured shell 9P allocation overhead without weakening admission,
resource custody, queue bounds or presentation semantics. This is a targeted
prerequisite for t252's existing cost gate, not a promotion of t289's broader
engine optimization programme. Requested by niltempus on October 15.

## Task details

### t331 — Reduce shell allocation overhead

On October 17 niltempus accepted leaving the panel allocation result red while
descriptor work proceeds. This is permission to proceed, not a passing result
or a changed threshold. t331 remains open; all four +10% gates
(panel and bulk CPU/MiB and allocations/frame) must be rerun after t338 under
the existing accounting method. Per-request allocation attribution and the
journal-read-pairing investigation remain required t331 work and may proceed
independently of the completed t338 implementation. The failed B2/B3 pilot
remains the latest native IPC gate evidence.

On October 17 niltempus accepted closing t338 with its report-only native
panel CPU pair and left repeatability/performance qualification here. The
[retained t338 result](x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md#current-resumption)
compares files bytes with per-frame fresh sealed memfds: sealed used
10.34–10.61% more Session-plus-peer CPU in that single pair. These are counter
bounds, not confidence intervals. There is no IPC arm, allocation measurement
or passing t331 verdict. Preserve this regression as evidence; balanced
restarted trials and the existing workload/rate/condition gates remain required.

The paired debug diagnostic records 821–851 allocation operations per frame
on files versus 250–271 on IPC for its small bar workload. With the 4 MiB
transfer probe enabled, the bounds are 21,498–21,538 versus 8,288–8,300.
These count Session and peer Rust allocations plus reallocations. They establish
a profiling target, not a native B9 verdict or whole-desktop cost measurement.

1. Attribute the excess to concrete client, codec, export or owner call paths
   using matched workloads and bounded profiling. Keep baseline artifacts and
   raw unsuccessful runs. Separate the small-bar and bulk-upload results.
2. Reduce avoidable allocation and copying at their actual owners. Prefer
   bounded reusable storage or borrowed data only where ownership permits it.
   Do not hide allocations by moving them into uncounted foreign allocators,
   other processes or unbounded caches, and do not disable client features.
3. Apply production fixes to production and carry the same fixes onto the
   isolated comparison branch. Never merge the restored IPC adapters into
   production. Preserve independent protocol checks, malformed-input refusal,
   authentication, revocation, backpressure, retained pixels and final release.
4. Use the owner-accepted [Session-plus-client accounting scope](../../sophia-shell-files.md#cost-accounting-scope-accepted-october-15-2026)
   and freeze the runnable campaign before acceptance. Measure exact bytes, all offered frames and
   complete outcomes, with separate instrumented and latency runs.
5. Demonstrate allocations/frame and CPU/MiB within IPC +10% under that accepted
   method on matched optimized native runs. Preserve conservative bounds;
   overlapping bounds remain inconclusive. Verify the unchanged t252 upload,
   demand and presentation budgets, including both refresh rates. Record exact
   revisions, artifacts, environment, per-pair results and retained evidence.

The performance evidence may also satisfy t252's B9 gate. Closing t331 does not
close t252's separate lifetime/input, desktop-selection and rollback gates.
Task state and execution order remain in todo.md.

## Implementation sequence

### Immutable Shared Buffers — October 18

niltempus confirmed that clients unable to use the sealed channel need not be
supported. The [accepted amendment](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md#accepted-amendment--immutable-shared-buffers-october-18-2026)
selects Immutable Shared Buffers (ISB), backed by sealed memfds, for every
production shell and lock CPU image upload.
Remove byte fallback and byte-only compatibility rather than maintain a size
threshold or adaptive transport choice. This migration is pending; recording
the decision does not complete runtime or SDK removal and does not reopen t338.

1. Inventory production shell and lock byte-upload entry points, SDK APIs and
   actual client call sites. Require sealed capability and ChannelReady at
   image setup, and make absent or failed channels explicit errors. Preserve
   role grants and metadata-only clients; do not infer image authority from a
   revision number alone.
2. Migrate both SDKs in their independent repositories, then import signed
   snapshots and migrate production clients. Remove byte-upload and fallback
   APIs and server dispatch/storage once their callers have moved. Preserve
   control records such as cancellation and retirement that the sealed path
   shares. Do not patch vendored snapshots or reuse retired record numbers.
3. Replace production fallback controls with required-channel refusal and
   recovery controls. Cover absent/malformed discovery, ChannelClosed before
   Source and during validation, cancellation, timeouts, epoch replacement,
   revocation, retained pixels and final release. Keep lock coverage independent
   of image failure. Prove the normal owner path through both SDKs and verify
   that removed byte-upload entry points cannot admit production images.
4. Keep byte and IPC implementations only where needed by the isolated
   comparator and retained historical evidence. They do not require production
   compatibility switches. Preserve all frozen plans, captures and unsuccessful
   arms; never reinterpret an old bytes capture as sealed.
5. Attribute per-request allocations and repeated service work on the sealed
   and common control paths, then remove measured avoidable work. Keep the
   journal-read-pairing investigation. The four existing CPU/MiB and
   allocations/frame gates compare the production sealed files path with IPC
   on matched panel and bulk workloads; thresholds, accounting, latency budgets
   and acceptance sampling rules remain unchanged. New native runs are prepared
   only for a concrete optimization or qualification question.

The October 18 report-only pilots are retained under
`~/.local/state/t331/{panel-pilot-01,bulk-pilot-01,large-surface-pilot-01}/pilot-report.json`.
They show fewer allocations with sealed than files bytes in all three workloads,
and lower combined CPU than IPC for the bulk and large-presented workloads.
The small-bar gaps remain. These pilots do not satisfy the full acceptance
matrix or establish a single cause of the cost. Corrected process-attributed
profiles and counter replay are under
`~/.local/state/t331/attribution-followup-20261018/`; use
`corrected-profiles/`, not the superseded schema-1 profile reports.

### Follow-up: remove Rust's canvas-to-memfd copy

Removing Rust's canvas-to-memfd copy remains an optimization, tracked under t331;
the ISB naming and architecture change does not implement it.
The current benchmark peer fills a reusable canvas and the Rust SDK copies its
contents into the memfd with `pwrite`. A mapped builder could let it draw directly
into the future ISB backing before sealing, removing that preparation copy. The
C builder already provides this writable-mapping boundary and unmaps it before
sealing. Any Rust implementation must preserve bounded ownership and complete
write exclusion before admission; this is a proposed optimization, not a measured
saving or a reusable mutable pool.

1. Add a bounded writable-mapping builder in the independent Rust SDK, then
   migrate the producer to fill that backing directly. Finalization must end
   every writable view and unmap it before sealing and offering the descriptor.
2. Match the C builder's custody contract. Cover exact size and pixel contents,
   successful seal enforcement, and mapping/descriptor cleanup on failure or
   abandonment. Verify that finalized storage cannot regain a writable view.
3. Compare the complete preparation and upload path with the retained baseline,
   using identical pixels, dimensions and cadence. Keep creation, filling,
   sealing and validation inside the accounting; report producer and Session
   costs separately. Removing a copy does not itself prove a performance gain
   or end-to-end zero-copy rendering.

Immutable accepted-resource reuse remains part of the content model. A changed
image still gets a fresh sealed source; mutable pools and DMA-BUF retain their
separate design and custody gates.

### Journal-read pairing evidence — October 17

For a steady submission, pairing means the events-file replies carrying its
Submitted receipt and its outcome, and the ACK writes acknowledging those
replies. A joined reply carries both; a split receipt arrives before a later
outcome reply. Report reads and ACKs per frame, records per read and joined or
split counts per submission. Match submission and journal sequence identities;
byte lengths alone cannot establish those joins.

The retained `t331-b3-20261017/ordered-candidate-01` device-free traces contain
643 panel event-read requests and ACK writes for 321 submits, and 1027 bulk
requests and ACK writes for 513 submits. Each final read has no sent reply:
the completed reply counts are 642 and 1026. Equal aggregate request/ACK counts
therefore do not prove a one-to-one read/ACK join. The 320 panel and 512 bulk
replies of 48 bytes are consistent
with lone Submitted receipts. That classification is inferred from sizes, not
decoded record identities. The 65,525-byte requested reads rule out a small
client read-size limit in those runs. This is neither native-load evidence nor
a measured cost per pair.

The trace lacks journal-append timing, so it cannot show whether an outcome
was already available when a receipt-only reply was encoded. A diagnostic
extension needs append sequence ranges, read-delivery ranges and completion
triggers, with deterministic same-turn and later-turn outcome controls.
Per-request allocation attribution remains separate. No change to read
completion timing, ACK policy or batching is authorized by this definition.

### Approved queued submit and pixel-write window — October 17

niltempus approved the [B2/B3 plan](nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
and directed implementation after reviewing the capture-first sequence. Record
the prepared first-slice native results before changing either SDK. Then queue
submit behind the final confirmed-prefix record fragment and add up to four
outstanding pixel writes for the same live resource, in separate signed changes.
The [accepted amendment](../../sophia-shell-files.md#queued-shell-writes) preserves
custody, fences, deadlines and existing budgets. Both SDKs must pass independent
tests and vendor gates; C retains its confirmed-ACK barrier and borrowed-buffer
lifetime. Use matched pilots for each change before the unchanged full matrix.
This supersedes the earlier prohibition on these two scheduling changes only.
Larger chunks, changed ACK policy, independent-resource concurrency and desktop
installation remain outside the approved scope.

### Approved reusable writer and cumulative ACK slice — October 17

The subsequent B2/B3 amendment above extends this slice only after its frozen
native capture. The following sequence records the reusable-writer work.

niltempus approved the replacement plan and directed implementation. The
[accepted decision](../decisions/7mls2e2q-reuse-a-shell-transaction-writer-without-changing-custody.md)
and [normative amendment](../../sophia-shell-files.md#reusable-transaction-writer)
authorize an optional `transaction-reuse` node and independent C/Rust support.
They supersede the earlier prohibition on retaining a transaction writer only;
upload-fid reuse, concurrency, higher limits and desktop installation remain
outside this slice.

1. Carry production `116075541` and its SDK pins onto the isolated comparator.
   Keep restored IPC there. Preserve unrun CPU-20 as superseded preparation.
   Validate both wires, record a fresh deterministic request ledger, and freeze
   four CPU plus four allocation baseline arms before attended capture.
2. Attribute allocations by request kind and owner pass. Separate constructed
   objects from allocation calls, request count from dependency waits, and both
   from measured CPU. Current sources, not batching-14 alone, select the targets.
   Record owner passes per frame on both wires and attribute them where observable
   to setup, record write, submit, pixel write, flush-only work or frame cadence;
   retain unattributed passes rather than inferring causes from totals.
3. Implement the reusable writer under the existing transaction allowance.
   Keep legacy behavior, exact custody and replay, permanent expiry fencing,
   bounded buffer reuse, and equivalent SDK discovery/error behavior. Test old
   clients against the new export and new SDKs against an export without it.
4. Separately coalesce automatic ACKs in both C and Rust at existing drive boundaries: before the
   next transaction and before sleeping or yielding with eligible progress.
   Preserve manual control, snapshot holds, budgets and the covering ACK barrier;
   add no delay timer or benchmark-only policy. Measure any reduction.
5. Publish signed SDK commits and import exact snapshots through their gates.
   Prove each slice deterministically, then use fresh matched native pilots.
   Run the unchanged full matrix only after passing pilots. Preserve failed
   arms, exact presentation joins, independent accounting modes and all budgets.

The historical ledger predicts 15 fewer panel requests and 24 fewer bulk requests
from writer reuse alone, not a guaranteed CPU or allocation pass. If the pilot
still misses +10%, publish the remaining attributed costs before another slice.
Aim to beat IPC without changing the ceiling. Pending-read deferral, speculative
submit, larger canonical chunks and per-pass polling changes remain investigations,
not implementation permissions in this amendment. t252's other gates and t289
remain separate. Native capture still needs an attended, coordinated window.

### Approved relay and writer-setup slice — October 16

The owner approved this bounded extension after progress-12 showed repeated
Session service between dependent requests. First, serve launch relays from
the owner's readiness observations and their deadlines. Keep custody,
revocation and retirement checks on every pass; use one aggregate readiness
sample when immediate work bypasses the owner's wait. Test idle relays,
completion wakeups, expiry, peer closure, replacement and all 64 slots.

Second, queue fresh writer walk and open requests together in the Rust SDK.
Wait for both successful replies before payload and retain a sequential
fallback at the existing capacity limit. Preserve upload admission,
sequential writes, write-before-submit, Submitted/ACK barriers and clunk
semantics. This explicitly permits the SDK scheduling and shell request-order
clarification needed for this slice; it does not permit general concurrency,
cached upload fids, new threads, raised limits or desktop installation.

Keep the relay and SDK changes in separate signed commits and carry shared
fixes onto both comparator wires. Isolated progress counters must distinguish
relay dispatch, idle skips and expiry work. Prepare fresh progress, CPU and
allocation pilots before the unchanged full native gate. Diagnostic counts
alone cannot close t331 or establish a performance win; t332 remains an
investigation.

### Earlier allocation and measurement sequence

The owner authorized this plan on October 15 and asked that 9P beat IPC, not
merely stay within +10%. Aim for lower CPU/MiB and fewer allocation operations
per frame on both workloads; claim a win only when the 9P upper bound is below
the IPC lower bound. The acceptance ceiling remains +10%.

1. Audit the measurement code before attributing its costs to production.
   Test snapshots, record formatting and steady-state sampling for allocation
   overhead. Replace benchmark JSON value construction with typed records;
   preserve every required event and reject oversized records before writing.
   Keep the native panel and panel-plus-4-MiB baselines as instrumented evidence.
2. Add a separate CPU-only comparator using the ordinary allocator, plus a
   separate bounded profiling build. Identify every mode in prepared plans and
   reports; reject mixed modes. CPU-only reports cannot claim allocation results,
   and profiling cannot supply acceptance arms. Align phase observations with
   the workload interval; never add overlapping scope totals or describe process
   allocations observed during a scope as exclusive stack attribution. Do not
   subtract estimated measurement overhead or change the accepted cost scope.
3. Prioritize Session after the native results attributed about 98% of the
   instrumented allocation gap to that process. Borrow readiness into inline wait
   storage, compare complete indicator facts before cloning, borrow action views,
   and reuse output bounds and reservation storage. Skip encoding identical,
   previously validated launch contexts. Preserve service order, deadlines,
   revocation, retained claims and all descriptor interests. Apply shared fixes
   to both comparator wires. Address upload-buffer copies only if the remaining
   measurements justify that next slice; do not introduce a new scheduler or
   transport thread, raise limits, cache upload fids or change SDK/wire contracts.
4. Measure each change independently. Run affected partial-I/O, EAGAIN,
   cancellation, disconnect, Submitted ordering, stale binding, malformed
   chunk, pixel validation, deadline, saturation, slow-reader, retained-pixel
   and final-release tests. Preserve independent C/Go checks where affected.
   Publish signed SDK changes through the normal vendor gate and carry shared
   owner fixes to both comparator paths. Revert unproven or regressive changes.
5. Run a short matched native comparison before the full campaign. Then run
   native acceptance at 1080p, scale 1, VRR off, at 60/120 Hz, idle and
   under 16 bounded CPU workers with all CPUs available. Use three independently
   restarted pairs per condition/workload, files first in pairs 1/3 and IPC
   first in pair 2. Cost arms use 1,000 frames without warmup exclusion;
   latency arms retain 100 warmup plus 1,000 measured frames. Every pair must
   pass; pooling cannot hide failures. Retain failed arms, exact kernel joins,
   all timeouts and wrapper recovery. Do not replace failed runs under the same
   campaign identity. Keep all existing upload, permit and presentation budgets.

If these slices still miss the native target, report the remaining measured
costs and propose a separate architectural change. Do not relax the limits or
expand silently into t289. Native runs require coordinated attended windows;
the optimization plan does not authorize changing the installed desktop.

## Approved allocation and copying slice (October 16)

Keep upload ordering unchanged. Borrow enabled-head observations instead of
collecting temporary index lists; retain owned lists for mutation paths. Store
the frame-service reducer's sets inline under its existing 16-output limit,
preserving validation and effect order. Verify allocation changes independently.

In the SDK, advance cursors after ordinary sends and acknowledged upload bytes;
compact ordinary output only when an append needs room. Preserve the secret
write's reserved storage and scrubbing path. Move outbox payloads into the active
file operation while retaining their tickets and full admission charges until
settlement. Preserve all limits, retries, terminal classification and control
reserves. Keep the pipeline's owned copy required by its borrowed-slice API;
new payload-ownership APIs and vectored output are outside this slice.

Run deterministic byte/custody tests and the signed SDK vendor gate, then a
fresh matched native pilot with separate CPU and allocation arms. Shared native
changes must reach both comparator wires. Do not install or restart the desktop.
The [concurrency investigation](../investigations/gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md)
remains candidate research after the measurements, not an implementation step.

## Approved readiness handoff (October 16)

Reuse Session's actual poll result for a connected shell export with one admitted
socket and no listener. An opaque, consumable observation records the server,
connection, service generation, subscribed interests and returned wake/socket
flags. A changed identity, generation or interest set requires the existing
nonblocking poll. Turns, flushes, adoption and listener changes invalidate old
observations; exhausted generations disable the shortcut.

Session retains at most one observation per component, tagged with its slot and
grant. Immediate authority work produces none. The next input pass consumes it
once and discards leftovers, including when component service is skipped.
Negotiation and standalone service retain their existing polling. Authentication,
delivery guards, deadlines, budgets, output flushing and upload order remain
unchanged. Readiness grants no authority; later input and wakes stay level-ready.

Verify real poll counts with test-only instrumentation, stale observations,
replacement connections, multiple components, late input/wakes, cancellation and
backpressure. Carry shared changes to both comparator wires, then run eight fresh
CPU/allocation arms. Retain the optimization only if repeated matched results
show a benefit. This slice neither closes t331 nor authorizes upload concurrency.

## Approved journal scratch experiment (October 16)

The owner approved proceeding with bounded reusable journal storage after the
copy audit. Add an optional buffered-read export method with the existing read
method as its default. Enable it for shell event reads through the authentication
adapter. Retain at most one connection-owned payload buffer, with capacity no
greater than the negotiated read limit; release it on version reset and close.
Encode into the existing owned output before recycling the payload. Preserve
all admission, acknowledgement and delivery checks, including partial writes.

Verify warmed allocation counts, pending/error/oversized results, queued reply
independence, reset and revocation. Carry the same implementation to the
comparator. Account for retained scratch separately from journal and output
storage. This experiment does not authorize direct output encoding, SDK vectored
ownership, concurrency or an installed-desktop change. Native benefit still
requires a fresh attended comparison.

## Connections

- [Maintained Plan 9 comparison](../investigations/2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md): pinned rio, Go p9 and diod audit, completed-read allocation fixture and remaining copy candidates.

- [Bounded upload concurrency investigation](../investigations/gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md#t332--investigate-bounded-upload-concurrency): candidate research after t331 measurements; no upload scheduling change in this slice.
- [Measured evidence and pending accounting proposal](../investigations/lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#accounting-method-proposal-october-15).
- [Unchanged shell budgets](../../sophia-shell-files.md#budgets).
- [t252 acceptance](jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t252--join-and-accept-the-shell-path).
- [Later engine CPU work, t289](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#t289).
