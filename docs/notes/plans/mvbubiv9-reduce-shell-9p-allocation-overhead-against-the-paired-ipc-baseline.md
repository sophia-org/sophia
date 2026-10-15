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
4. Freeze an explicit accounting method before the acceptance run. The current
   proposal excludes foreign allocation calls and helper processes; adding this
   task does not approve that scope. Measure exact bytes, all offered frames and
   complete outcomes, with separate instrumented and latency runs.
5. Demonstrate allocations/frame and CPU/MiB within IPC +10% under that accepted
   method on matched optimized native runs. Preserve conservative bounds;
   overlapping bounds remain inconclusive. Verify the unchanged t252 upload,
   demand and presentation budgets, including both refresh rates. Record exact
   revisions, artifacts, environment, per-pair results and retained evidence.

The performance evidence may also satisfy t252's B9 gate. Closing t331 does not
close t252's separate lifetime/input, desktop-selection and rollback gates.
Task state and execution order remain in todo.md.

## Connections

- [Measured evidence and pending accounting proposal](../investigations/lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#accounting-method-proposal-october-15).
- [Unchanged shell budgets](../../sophia-shell-files.md#budgets).
- [t252 acceptance](jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t252--join-and-accept-the-shell-path).
- [Later engine CPU work, t289](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#t289).
