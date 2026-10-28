---
id: p4jd1ymh
date: 2026-10-28
kind: investigation
status: investigating
tags: [investigation, performance, shell, custody]
---
# Which complexity and repeated-work costs belong on the frame CPU critical path

## Question

Mason asked for quadratic loops and other optimization footguns during W2
qualification. This is a bounded read-only source review, not a repository-wide
complexity audit. Keep W2's frozen experiment isolated from any new candidate.

## Evidence

The source-file hashes and reviewed checkout HEAD are retained in
`~/.local/state/t331/continuous-cpu-optimization-01/complexity-review.json`.
The measured counts below come from `continuous-shell-work-cost-01`, whose two
arms passed postflight, cleanup, strict snapshot reduction and root saved-data
recomputation. Owner intervals contain 1199/1200 descriptive completions; worker
intervals contain 1792 validation jobs each. They are not a shared CPU window.

| Source mechanism | Complexity or measured repetition | Relevance to this strip |
| --- | --- | --- |
| `shell_content/candidates/validation.rs:validate_targets` compares each target with every later target | O(T²) overlap validation | The peer sends zero targets; no overlap-loop work here |
| `shell_content/candidates/native.rs:from_begin` checks each row against its prefix | O(R²), with R capped at 32 | Launcher absent from these bar-only arms |
| `validate_current` scans catalog entries for each launcher row; `allocation()` scans snapshots per surface/target/placement | O(R*C) and cross-product lookups | Potential larger-scene scaling; one surface/placement in this strip |
| `CpuImageJobs::reconcile` scans its fixed four slots | About 121 calls/completion, 99.17% empty | Frequency, not growing O(n²), is the measured issue |
| `ContentResourceStore::collect/expire` select entries before mutating maps | Full-map scans plus temporary selected-entry Vecs | Allocation frequency and occupancy are not measured; an empty Vec does not establish a heap allocation |
| File-wire service and per-file/image boundaries check authority | About 36 export polls and 91 custody checks/completion | Freshness is deliberate; frequency does not establish redundancy |

Owner inclusive custody CPU is about 100–101 us/completion; inclusive settlement
is 67–68 us/completion. These are separate, overlapping diagnostic scopes, not a
170 us savings budget. The diagnostic performs about 640 extra owner clock reads
per selected completion interval; do not subtract a guessed instrumentation cost.
Worker immutable-copy scope is 17–18.5 us/job, within validation's 43–46 us/job;
pixel validation is 23.7–24.7 us/job. Allocation and exact payload-read subscopes
are nested. This is measured stage attribution, not isolated memcpy cost or a
comparison with an uninstrumented build.

S1's ABBA is complete and not promoted: combined [0,+80], Session [+25,+65],
peer [-25,+15] us/frame. Sampled quiet re-waits were 35/10187 and 46/10177 attempts
(about 8.5 attempts/completion). Nonquiet does not identify domain work: ACKs,
disallowed requests and other quietness state can force fallback. Dividing the
CPU difference by attempts does not measure probe cost. S1 remains parked.

## Critical-path disposition

1. Finish the already-reviewed W2 same-binary off/on ABBA first. No extra patch
   enters it. Its predecessor screen passed the predeclared regression margins,
   which is not equivalence or a gain.
2. Next bounded implementation investigation: trace the callers of empty
   settlement and collection sweeps. Propose an exact ready/pending-state gate
   that avoids an empty scan without caching positive authority. Determine all
   transitions that can make work ready before writing production code. The
   measured counts rank this investigation; they do not authorize deleting calls.
3. Require device-free proofs for completion arriving across the gate, cancellation,
   expiry, channel loss, reconnect/grant replacement, backpressure and release
   after the last lease. Preserve retry ownership, response ordering and eventual
   service when a worker's last lease drops. A local gate must not become another
   speculative S1 probe that repeats full-pass work on almost every wake.
4. If a bounded candidate clears review, give it an isolated ABBA and a counter
   showing fewer empty reconciliation calls, with unchanged settlements/releases.
   Keep inclusive timing out of the claimed savings. Stop if no CPU benefit is
   resolved; do not widen the predicate merely to manufacture conversion.
5. Keep standing-credit/frame-stream design on the architecture track. For
   larger launcher or multi-surface workloads, first measure occupancy and time;
   only then consider bounded duplicate bitsets, indexed allocation lookup or
   overlap algorithms. Preserve all malformed-input and duplicate rejection.

The empty-work investigation is on the immediate plan after W2, not permission
to alter frozen sources, weaken freshness or start another native lane now.
No production code, builds or device runs were performed for this review.

## Connections

- [Optimization critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md): experiment order, completed results and stop gates.
- [Standing-credit frame stream](tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md): fewer logical control exchanges, separately gated.
- [Shell file contract](../../sophia-shell-files.md) and [image-channel contract](../../sophia-image-channel.md): custody and release boundaries remain mandatory.
