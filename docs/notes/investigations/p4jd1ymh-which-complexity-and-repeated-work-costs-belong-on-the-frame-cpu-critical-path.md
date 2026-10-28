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

1. W2's experimental performance gate is closed after independent effect blocks
   02 and 03 reproduced combined and Session CPU reductions. Keep their latency
   increase visible and their results separate in the linked critical-path plan.
   Production integration remains gated; sources, releases and seals stay frozen.
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

## Source closure: empty reconciliation and collection (2026-10-28)

Read-only review against baseline `d9116ff0` closes the positive-byte question.
The vendored Rust SDK's `shell/content/validation.rs:392-426` requires both
dimensions to be positive, checks width times four, and requires the exact
total byte count. `ContentResourceStore::begin_isb` calls this `layout` before
reservation. Capture admission also calls `begin_isb` (`resources/capture.rs`),
so both accepted-resource insertion paths inherit strictly positive bytes.
Explicit retirement adds that resource's bytes to `usage.retiring`; removal
subtracts them. A live, non-revoked store with `usage.retiring == 0` therefore
has no retiring resource.

The smallest collection candidate is a per-store early return when
`!self.revoked && self.usage.retiring == 0`. Baseline `collect` selects only
retiring or revoked resources; its second removal loop is revoked-only.
This guard needs neither a new counter nor a lease-drop notification. It never
skips a revoked epoch or a store with a retiring resource, including one whose
last lease drops silently. General dirty-bit suppression while retiring resources
exist remains unsound without a drop-visible mechanism.

Placement matters on the frozen W2 source: `collect` first calls
`forget_released_intents`. Any future combined implementation must preserve that
cleanup before the guard. This is not permission to modify the measured W2 lane.
Collection uses already-reserved response credits; journal capacity constrains
later publication. `confirm_isb_release` removes the accepted resource directly
after journal confirmation, rather than clearing a flag for a later collection.

For settlement, `CpuImageJobs::reconcile` tests `pending.result.is_some()` before
checking allocation currency. An allocation-version cache adds no necessary
invalidation for skipping the no-result case. Worker receive is not the only
producer: unsent `withdraw` synthesizes `Some(Err(Incomplete))`, including when
driven by `observe_store_stops`. A result can remain pending after a failed
settlement; `None` from reconcile is not proof that no result remains. Any
maintained ready state must cover both producers and retain retries. Scanning
the four slots first just to decide whether to scan them again is not a useful
candidate.

Next gate is a separate device-free proof proposal: reject zero dimensions on
both admission paths; preserve explicit retirement, silent last-lease release,
revoked non-retiring resources, publication blockage and confirmation; preserve
W2 intent cleanup if combined later. For ready-state changes also cover synthetic
withdrawal, retained errors and consumption. No implementation or build is
approved by this source closure. Measure actual scans separately from calls;
the diagnostic Settle timer surrounds every reconcile call, so its inclusive
67-68 us/completion cannot price a four-slot scan or its removal.

## Collection candidate: device-free correctness gate closed

Mason's subsequent "proceed on critical path" authorized the narrow candidate.
Signed local commit `75ff298ab46058f78b2ff0fc1454010116f537c5` on
`cpu/collect-idle` in `~/dev/sophia-cpu-collect-idle` has exact base
`d9116ff0905fab5c8bc01a71228ecc9e1a7a0a6d`. Four production lines add the guard
and its positive-byte invariant comment. Three new fixtures cover CPU and
capture zero-dimension admission and repeated collection with both a live and
a retiring resource. There is no new field, feature, timer or authority cache.
W2 sources, releases and all sealed packages are unchanged.

The new fixtures first passed on the unchanged collector in a focused suite
(228 tests). The candidate passed all runtime tests with capture enabled
(785 passed, 41 ignored) and default features (761 passed, 41 ignored), including
the existing capture eviction, revocation, epoch replacement and last-lease
tests. Capture-enabled all-target clippy with warnings denied, package formatting
and the source-layout audit passed. These are device-free runtime checks, not
Session/native qualification.

Two mutations were tested separately and reverted:

- Removing `!revoked` failed the existing revoked resident-only resource test.
- Replacing the guard with `!revoked && usage.resident != 0` failed the new
  mixed-store test: the retiring resource's release was missing.

After restoration the complete diff matched the reviewed bytes and all 15
resource tests passed. Claude and Muse independently reviewed the saved diff
without running tests or changing files and found no blocking source gap.
Deleting the optimization would preserve the old behavior and pass these tests:
they prove equivalence and catch unsafe broadening, not fast-path execution.

Durable receipts: `~/.local/state/collect-idle-proof-01/RECEIPT.json`
(`a88c2d9daa330874a1be68d419f60ef3919470410469d88bbc74054fca5acdf3`),
with inventory `SHA256SUMS`
(`7beefb313c61a9c61b2095bb6d1fab085f0fa9699b0285fddb20081425dfffc0`).
They retain the initial command error (nonexistent test target, before
compilation), corrected baseline, candidate checks, exact mutation diffs and
restored green receipt. Nothing was copied from a build slot.

Next is performance qualification, not adoption: establish actual scan/skip
movement and an isolated CPU comparison before claiming benefit. The scan has
only two accepted resources in this workload, so a correct guard can still have
an unresolved or negligible CPU effect. No release build or native package was
created at this gate. W2 integration must still place the guard after intent
cleanup. Any future admission path allowing zero charged bytes must revisit
the invariant before using this guard.

## Connections

- [Optimization critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md): experiment order, completed results and stop gates.
- [Standing-credit frame stream](tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md): fewer logical control exchanges, separately gated.
- [Shell file contract](../../sophia-shell-files.md) and [image-channel contract](../../sophia-image-channel.md): custody and release boundaries remain mandatory.
