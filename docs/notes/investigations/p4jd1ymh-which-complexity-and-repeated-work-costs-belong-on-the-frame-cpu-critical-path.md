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

## Current resumption

November 2: niltempus approved the [performance-led critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md#current-resumption-close-the-matched-cpu-gap):
W2 integration and matched qualification, bounded W3 frame-stream credits,
then one distinct ready-work scheduling experiment. Combined Session + producer
CPU is the outcome; process breakdowns guard against cost shifting. t344 remains
open. Each mechanism ends in adopt, reject or a named contract blocker.

Attribution has supplied enough evidence to attempt bounded mechanisms. Missing
callers and window partitioning remain report limits, not implementation or
measurement prerequisites. Freeze instrumentation; use the qualified external
profiler only for a named blocked decision. Preserve all refusals and unknown
memcpy callers. S1, single-submit and collection stay parked. Required fresh
custody checks cannot be treated as redundant work without a separate proof.

## Historical attribution checkpoints

November 1: niltempus requested device-free attribution of Session's fixed
per-frame work before another candidate. The isolated implementation at signed
`aacb8dfbe` completes that fixture step: eight release blocks, 16,000 measured
frames, exact pixels/protocol/job counts, four timer controls, and independent
saved-record audit with zero findings. The
[report and evidence identities](../../session-frame-attribution.md#audited-result-november-1)
are authoritative for the new run; older native windows below remain separate.

At h24, inclusive OwnerService is 85.11–85.88 µs/completion and Transport is
46.61–47.11, at 8.472–8.588 service visits/completion. Instrumentation is large:
owner outer CPU is 162.77–167.12 off versus 295.49–297.28 on. These are raw
diagnostic scopes and fixture references, not an optimization budget. h512 has
3000 reservation retries beyond 2000 accepted jobs per enabled block, caused by
paired offers under backpressure; retry counts are not extra completed frames.

Keep the next investigation at the attribution boundary: separate fresh 9P
authorization/custody from its Transport scope and account for full owner-loop
work absent from the fixture. The Authority row measures allocation-description
lookup during ring reservation, not all 9P authority work. Do not infer empty
visits, cache freshness, subtract overlapping clocks, or propose a candidate
from the owner outer window. S1, single-submit and empty-owner-work remain parked;
the historical disposition below does not override this current sequencing.

The follow-up now has an accepted timer-perturbation checkpoint at signed
`9ba5b0b3b`: period 256, six reusable resource IDs, sixteen blocks of 8000
measured frames. Root reduction and independent audit pass all counts and
both predeclared 5% gates. Owner CPU differs by −2.35% to +1.65% from paired
counter-only controls; visits differ by less than 0.66%. Off controls are stable.
The [audited sparse result](../../session-frame-attribution.md#audited-sparse-result)
records sealed attempt 03 and its audit. Attempts 01 (perturbation refusal)
and 02 (the old fixture's 4096-ID lifetime bound) remain retained; production
limits were not changed. ACKs are cumulative, so the reducer's initial observed
ACK/read equality was removed with its source justification and old validator
preserved, not replaced by a tolerance.

The [compiled-stub reference](../../session-frame-attribution.md#counter-only-reference-against-the-compiled-stub)
at signed `112c16d7b` passed identity, protocol and saved-record audits, but its
predeclared perturbation gate failed: one h512 comparison is +5.08% CPU against
the 5% limit. Keep that refusal; neither counter-only nor combined observer
overhead is qualified. Evidence and independent audit are sealed separately.

The [protected authentication/readiness fixture](../../session-frame-attribution.md#protected-authentication-and-observed-readiness)
now passes device-hidden smoke with diagnostics enabled and compiled out.
It uses real supervised authentication and allocation, the shared frame loop,
and the production subscribe/observe/consume sequence. Recorded decisions
prove one observed turn per normal visit; fallback turns belong to the paired
fill pump. The oracle stays off the owner thread, with release ordering intact.
The 1063-test Session suite and 14 runtime diagnostic tests pass; these are
correctness checks, not protected performance measurements.

The complete protected bar component pass now runs at signed `26d2fd1f7`,
with only native admission/completion supplied by the fixture. Root and
independent saved-record audits pass all 32 workload blocks and prove actual
readiness consumption. Its [combined-observer reference](../../session-frame-attribution.md#protected-full-pass-result-observer-qualification-refused)
failed one predeclared comparison: phase 129 h24 is +5.187571% CPU against
5%, with stable controls and visit counts. Keep the refusal untolerated.
No Session candidate is promoted. The observer-only power-of-two sampling
fast path at `4b204fdbb` preserves all ordinals/counts, but its new reference
also fails: two h512 comparisons exceed 5% (+5.718584% and +5.774361%). Both
refusals remain sealed, with independent audit agreement. The
[masked result and next handoff](../../session-frame-attribution.md#masked-observer-result-and-external-sampling-handoff)
record the exact evidence.

On November 2 the owner completed the external user+kernel sampling reference
against the held compiled stub. The predeclared 5% gate passes all four
comparisons: h24 owner CPU +0.274/+0.493%, h512 +2.929/+1.527%, visits within
0.128%. Root and corrected independent audits agree on identity, all eight
blocks' protocol records, explicit cpu-clock:uk attributes and no sample loss.
The [result and evidence identities](../../session-frame-attribution.md#external-sampling-reference-november-2)
retain the earlier inline-observer refusals unchanged.

Owner threads contribute 480/2205 and 473/2192 whole-tree samples. Visible
stacks corroborate fresh custody checks, image-channel checks and polling,
but their inclusive occurrences overlap and are not an optimization budget.
The 100/101 owner memcpy/memmove leaves include 98/99 single-frame stacks;
their callers are unknown. Unresolved owner leaf IPs are 5/480 and 12/473,
mostly vdso. The independent audit's original zero-unknown claim is corrected
in its retained sibling manifest; leaf resolution does not prove full unwind.

Next resolve missing callers and measured-window partitioning before choosing
a production candidate. The owner profile combines heights, setup, warmup
and teardown; native admission/completion remains simulated. No cached
authority decision, savings estimate or native fixed-cost claim follows.
The CPU quiet hold is released, Mini's GPU worker stays disabled, and the
original baseline and physical-owner qualification remain separate.

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
