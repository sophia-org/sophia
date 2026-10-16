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

## Connections

- [Bounded upload concurrency investigation](../investigations/gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md#t332--investigate-bounded-upload-concurrency): candidate research after t331 measurements; no upload scheduling change in this slice.
- [Measured evidence and pending accounting proposal](../investigations/lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#accounting-method-proposal-october-15).
- [Unchanged shell budgets](../../sophia-shell-files.md#budgets).
- [t252 acceptance](jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t252--join-and-accept-the-shell-path).
- [Later engine CPU work, t289](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#t289).
