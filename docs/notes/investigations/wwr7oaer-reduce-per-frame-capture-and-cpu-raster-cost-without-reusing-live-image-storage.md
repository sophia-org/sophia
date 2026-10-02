---
id: wwr7oaer
date: 2026-10-02
kind: investigation
status: closed
tags: [investigation, renderer, performance]
---
# Reduce per-frame capture and CPU raster cost without reusing live image storage

## Question

Why does a terminal redrawing small updates cost several milliseconds of Sophia
CPU per frame, and can that cost fall without weakening immutable image custody,
damage correctness, input ordering or frame retirement? This is t278, selected
by niltempus after the t276 idle-wakeup work.

## Evidence

The implementation starts from Sophia
`9d3a41904ac2994bf00980b3399bffb27c43eb04` in `perf/render-presentation`.
Evidence is retained under
`~/.local/state/sophia/development-evidence/render-performance-01/`.

The exploratory live samples showed CPU use changing with the terminal's frame
rate. They did not isolate Codex, Herdr, the terminal and Sophia. The controlled
tests below isolate two Sophia paths; they do not establish a new live desktop
baseline or diagnose why a particular client redraws.

## Finding and resolution

Every new renderer image created an EGL context and pipeline, allocated upload
textures, copied the client image and destroyed the execution resources. The
copy is necessary for immutable image ownership; repeating context and shader
creation is not. The EGL owner now retains at most two compatible capture
contexts, one per XR24/AR24 format. Every capture still gets a fresh surface and
BO, imports the actual client descriptors, flushes, and clears the import.
Cross-device scratch reuse still waits for its explicit completion fence.

The CPU scene also treated GPU-only content generations as CPU raster changes.
Its retained raster key now follows CPU uploads and compositor content. Busy
backings remain immutable. Equal-generation CPU uploads and registry eviction
invalidate the retained baseline, including an evicted handle being inserted
again with its previous generation.

Engine owns a bounded damage journal: 16 transitions per surface, 32 rectangles
per transition and 8 MiB per journal or frame view. Precise damage requires a
complete chain from the retained slot through every commit. It is initially
limited to canonical, exact, unscaled, unclipped, Normal-transform sampling.
Unproved paths repaint the whole affected surface. X Authority provides source
rectangles only for top-level, unoffset DMA-BUF Present without a valid-region
override. A rebase discards partial damage; recovery replacements clear history.

Generation and buffer handle are insufficient pixel identity. A rendered but
rejected preparation can reuse both with a later commit. Each preparation now
has a private opaque identity carried through commit and into native and CPU
frame snapshots, including alternate variants and preview-only sources. Pending
endpoints cannot be evicted to retain older history. Alternate variants retain
the identity of their whole content set while remaining ineligible for precise
canonical damage. Missing chain evidence falls back to full damage.
The final edge must match the target identity too: a regressed, unassociated
view can reuse an old committed generation and buffer while carrying different
pixels. `59-terminal-identity-red` reproduces the incorrect partial result;
`60-terminal-identity-green` verifies the full fallback and the monotonic
partial-damage control. This is a defensive fallback case, not an observed live
failure.

The existing resource cadence now emits numeric render-work counters. Optional
stage timings use elapsed and calling-thread CPU time. A worker context shared
by several outputs is counted once from its newest observation; output slot
statistics remain per output. Transfer counters expose work whose stage timings
belong to a separate context. No client text or raw handles enter these records.
Context replacement can decrease cumulative counters; delta analysis treats
that as a reset.

## Validation and remaining work

The frozen source passed fmt, layout, generation, both SDK checks, strict
workspace Clippy and all-feature workspace tests (`45`–`51`). The workspace log
reports 6234 passes, zero failures and 82 ignored tests; re-executed child helper
output means this is a log total, not a count of unique tests. `53-models-02`
passes the pinned Alloy/Z3 gate. Its preceding red records the host Z3 version
mismatch; a private official 4.16.0 binary satisfied the gate without changing
the host installation.
After the terminal-identity correction, `63-xtask-full` passes the complete
repository gate, including strict Clippy and six retained archive checks, with
the source unchanged during the run. `62-fmt` retains the preceding formatting
failure in the new regression; the formatted source passes `63-fmt`.

All 15 deliberate defects in `44-mutants.json` fail their named regressions.
They cover stale provenance, alternate variants, missing intermediate damage,
budget eviction, oversized candidates, partial rebases, CPU raster churn,
equal-generation uploads, buffer eviction, CPU frame provenance, unsafe X
damage, duplicated shared metrics, repeated capture setup and lost import
statistics. Mutants ran in a separate source copy and target; the copy was
restored and compared with the candidate afterward.
The separate `61` control removes the terminal-identity check and fails the
regressed-view test. Together these cover 16 named deliberate defects.

`56-native-renderD128` and `56-native-renderD129` each pass four render-node
tests. The journal test renders 40 frames through three slots, deliberately
rejecting preparations that reuse generation and handle; its 32 partial frames
match full rendering exactly. Capture snapshots survive client mutation, local
eviction and destruction of the capturing context. Six captures create two
pipelines and six fresh surfaces, with six imports/evictions and zero retained
client imports. `57-cross-device` passes three tests, including actual import
refusals and exact retained XR24/AR24 pixels in both GPU directions. These tests
open render nodes only, without DRM master or display mutation.

The initial `43` measurements use three alternating baseline/candidate pairs
per path, each with 120 warm-up frames and 3600 measured frames at 60 fps.
Capture process CPU fell from 4.738–4.845 to 0.382–0.395 ms/frame; CPU scene
work fell from 0.367–0.377 to 0.010–0.013 ms/frame. Every pair has a lower p99
and equal throughput. Capture creates no new pipelines after warm-up. These
are path measurements, not a claim of the same percentage reduction in total
live Sophia CPU. The final `55` capture measurements repeat the three pairs
after the import-counter correction: 4.498–4.668 ms of process CPU per frame
falls to 0.385–0.387 ms (91.4–91.7% less), and elapsed p99 falls from
5.541–5.709 to 0.558–0.589 ms, at the same 60 fps. Each baseline creates
3600 pipelines; every warmed candidate creates zero. The first raw results and
binary hashes remain retained. Noncanonical variants continue to repaint fully;
these measurements do not claim partial-damage gains for scaled variants.
The rebuilt final capture executable is byte-identical to the measured one
(`65-bench-identity.json`).

At code qualification, the outstanding acceptance was to qualify the candidate
and measure the same live terminal workload at matched client frame rate. Check
the periodic counters, resource bounds, input responsiveness and clean teardown.
No live session, personal profile, terminal animation setting or Herdr server was
changed by this implementation. The separate held-switcher work is not included.
niltempus subsequently requested one installable release on master combining
this change with the separately reviewed held-switcher work (t279 and Hagia
h009-h011). Combined qualification and next-login preparation are required;
neither task is physically accepted by these device-isolated gates.

## Combined release qualification

Renderer commit `6e0f480946f769915bc42d78ea2f56b3a0bccbd7` and held-capture
implementation `55830ddae7d1cea6a2cfd2b69765eb7a48f5627d` merged mechanically
in `5a0f8121f63b1243235789e3730b1b853bf73103`. The sole shared test file
contains independent additions. The combined tree passes the complete xtask
gate, pinned architecture models and partial/full pixel equivalence on both
render nodes. Evidence is in `t278-t279-release-01` (`02`–`06`).

The held-capture plan and task rows from `bd87b77d` are merged separately.
Resolving the adjacent t278/t279 queue insertions changes no code. Production
and test files remain byte-identical to the combined qualified tree. Live
CPU, input behaviour and physical retirement still require the next-login run.

## Live CPU evidence, 2026-10-02

The operator arranged Ghostty with the active Codex pane in Herdr on DP1 and
btop on DP2. pF sampled `/proc` read-only after a 10-second grace period, for
60 seconds, using the same sampler as the morning baseline. The baseline was
release `niltempus-2adbe49302088d28c023` (Sophia `9d3a4190`); the new session
ran `niltempus-9de41ea905db10201b9e` (Sophia `f650e688`, PID 30884).
Evidence is under `~/.local/state/sophia/development-evidence/` in
`t276-t277-live-01/ghostty-dp1-01.json` and
`t278-live-cpu-01/ghostty-codex-02.json`, with the corresponding empty-DP1
samples. `t278-live-cpu-01/root-compare.py` recomputes the results from the
retained data; `ROOT-COMPARISON.json` records the inputs' digests and limits.

| Measurement | Morning baseline | New session |
| --- | ---: | ---: |
| Sophia CPU, percent of one core | 30.15 | 10.75 |
| Render-worker runtime, percent of one core | 12.49 | 2.78 |
| Owner-thread runtime, percent of one core | 12.31 | 5.47 |
| Ghostty CPU, percent of one core | 14.08 | 12.85 |
| Recorded Ghostty retirements per second | 53.58 | 51.80 |
| Sophia CPU with empty DP1 and btop on DP2 | 5.28 | 3.37 |

Sophia process CPU fell 64.3%; render-worker runtime fell 77.7%. This supports
a live reduction consistent with the isolated capture measurements. Diagnostic
budgets suppress retirement records, so the similar recorded rates do not
prove identical actual frame cadence. Client sets also differ: Sophia had 91
threads in the baseline active run and 70 in the new run.

Subtracting each session's empty-DP1 CPU and dividing by recorded retirements
gives approximately 4.64 and 1.43 ms per recorded retirement. These ratios are
not direct renderer timings: their denominators are incomplete and their idle
costs come from separate samples. The initial pF report used a static visible
Codex sample for the new idle subtraction; its appended correction preserves
that error and fixes the result. The animating Claude-pane run was not matched
to its morning baseline and does not establish a second matched comparison.

Before the operator decision below, this was supporting evidence rather than
completion of t278. There is one 60-second
sample per workload. Periodic render counters and resource bounds, input
responsiveness, frame pacing and clean teardown still need qualification.
Both releases already contain t276, and the new release also contains t279
and t284; these live samples do not isolate either t276 or t278. They establish
no separate idle-wakeup measurement. No build, configuration change or generated
input was used to collect them; the operator controlled the workload.

## Formal operator acceptance, 2026-10-02

niltempus stated, as relayed by pF: **"this is formal acceptance."** The
operator explicitly accepted the installed `niltempus-9de41ea905db10201b9e`
live CPU result and requested acceptance and status changes for t278 and t276.
Task t278 is closed on that decision, with the code qualification and live
evidence above. The installed Sophia revision is
`f650e68831f15a7e8b767dac88c598aebe515e51`.

The decision is retained in `t278-live-cpu-01/FORMAL-ACCEPTANCE.txt`. Its
accepted `ROOT-COMPARISON.json` has SHA256
`904765a10eff6f63404243aabd8d71307e7bd5e478e62c3206ef799ad7e190b6`.
The earlier open-status assessment in that evidence is preserved as history;
this operator decision supersedes it.

The operator accepts closure without the remaining planned repeats and live
checks. The single-sample, incomplete-cadence and attribution limits above
remain. No additional latency, pacing, resource-bound or teardown measurement
was performed, and none is reported as passing. Future CPU optimizations are
separate work; this acceptance does not close another task's required checks.

## Connections

- [Renderer import boundary](../../renderer-import-boundary.md) owns the current
  capture, damage and telemetry rules.
- [Idle worker waits](qvrk2298-remove-timer-polling-from-idle-desktop-workers.md)
  explains t276, which removes idle polling rather than per-frame rendering work.
- [Work tracking](../../work-tracking.md) keeps live acceptance distinct from
  implementation and render-node proofs.
