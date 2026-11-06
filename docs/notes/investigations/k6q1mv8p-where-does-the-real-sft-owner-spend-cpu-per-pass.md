---
id: k6q1mv8p
date: 2026-11-06
kind: investigation
status: investigating
tags: [investigation, cpu, sft]
---
# Where does the real SFT owner spend CPU per pass

## Question

Which actual owner phases consume CPU in ordinary SFT, after the bundled-copy
fixture failed to supply a faithful physical-loop scheduling proof?

## Method

The opt-in `diagnostic-owner-cost` build feature records CLOCK_THREAD_CPUTIME_ID
around the existing loop phases. It changes no scheduling, protocol, timers,
custody checks or input/work budgets. The default build uses no-op methods.
A fixed 32768-row buffer is allocated before entering the loop. Each row spans
one actual loop iteration, with CLOCK_MONOTONIC start/end and exclusive CPU
buckets: component service, both native-service sites, authority handling minus
nested native service, and everything else. Authority includes wait syscall and
control-handling CPU, not sleeping wall time. Other includes existing periodic
logging, WM, topology, lifecycle outside native service and remaining work.

Rows are emitted only after the loop exits, followed by an explicit footer with
complete/overflow, PID/TID and initial/final OwnerWake pass counters. Errors
produce complete=false; forced termination or panic can leave no footer. The
analysis must refuse those cases, overflow, malformed/duplicate/missing records,
identity mismatch and incomplete coverage. It binds the ordinary native
120..2120 completion window, selecting fully contained passes and reporting
conservative CPU bounds for overlapping boundary passes. Phase upper bounds
cannot be added: each conservatively includes all boundary-pass CPU.

## Limits and next gate

This is a single diagnostic run, not an ABBA or a production optimization.
Thread-CPU clock reads are observer overhead and can perturb scheduling; their
cost is included in phase totals. Do not subtract an assumed overhead, pool the
CPUwin artifact with uninstrumented runs, or label a phase removable merely
because it is expensive. Session worker and peer CPU are outside these clocks.
The real-loop phase measurement does not itself prove a scheduler change safe.

Use the corrected ordinary-SFT host as parent, with held peer/SDK/runner and
pixel workload. Keep current protocol/identity/cleanup checks and detached
SSH-visible completion markers. No install, merge or automatic repeat.

Source tests cover CPU partition, next-pass reset, nested phase restoration on
error, bounded-buffer overflow and the actual record/footer formatter. The
package reducer tests cover field shapes, identity, continuity, complete joins,
boundaries and explicit refusals. Evidence is recorded under
`~/.local/state/sophia-sft-owner-cost-source-01` and the fresh diagnostic package;
no native result is claimed at this source checkpoint.
