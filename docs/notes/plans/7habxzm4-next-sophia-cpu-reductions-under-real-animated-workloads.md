---
id: 7habxzm4
date: 2026-10-02
kind: plan
status: proposed
tags: [plan, milestone]
---
# Next Sophia CPU reductions under real animated workloads

## Scope and exit

t276 and t278 were formally accepted on release
`niltempus-9de41ea905db10201b9e`. On the matched workload (an active Codex
pane in Herdr in Ghostty), Sophia fell from 30.15% to 10.75% of one core
(`t278-live-cpu-01`, ROOT-COMPARISON.json). The remaining cost, as % of one
core:
- owner thread 5.47;
- render workers 2.78;
- session records 0.81;
- X11 client workers 0.74.
That is about 1.43 ms of Sophia CPU per recorded retirement, net of the
empty-workspace sample.

This plan collects the next reductions. The rule for all of them: Sophia
must handle real animated client workloads efficiently. A client feature,
such as terminal or TUI animation, is never disabled or tuned as the fix.
The accepted workload stays the benchmark.

Exit:
- each implemented change has named regressions, with counter assertions
  where the effect is deterministic;
- each investigated part records a measured outcome, including a no-change
  decision;
- live matched samples beside `t278-live-cpu-01`;
- an automated gate (part 6) that keeps future acceptance repeatable without
  hand-run live samples. The operator's live check remains the final
  acceptance.

## Task details

<a id="t289"></a>
**t289: next CPU reductions, candidate.** One task in six parts. Part 1
orders the experiments by measured cost. Any part may end in a measured
no-change decision; this plan does not pre-approve six production changes.

1. **Owner-thread profile (read-only).**
   - Sample the live owner thread for about 30 s on the accepted workload.
   - Rank its userspace costs before changing code: Present intake,
     scheduling, display-list lowering, damage, retirement and diagnostics.
   - The ranking requires a permitted userspace sampling profiler with
     usable symbols and unwinding (for example `perf record -e cpu-clock:u
     --call-graph dwarf`, given the existing `.symtab`).
   - `/proc/<pid>/stack` exposes only the kernel stack, so it cannot rank
     these functions. `/proc` stat and schedstat remain coarse thread CPU
     and wait evidence only.
   - If no such profiler is available, record attribution as blocked.
   - Never attach a stopping debugger and never change host permissions.
   - No live configuration change.

2. **Present pacing for windows nobody can see.**
   - A hidden Ghostty kept submitting about 18 Presents/s while Sophia
     composed nothing of it. In that window the client's CPU was 3.53% of
     one core. How much of that, or of Sophia's owner work, the Presents
     cause is still unmeasured.
   - Uncaptured Presents are already parked to the head's refresh (the
     settle-uncaptured path). The proposal is a slower, bounded completion
     cadence for surfaces with no visible sampling output, restoring
     immediately when the surface becomes visible.
   - Visibility is the union of actual sampling on all heads, including
     t284 previews, mirrors and, where applicable, transitions.
   - Preserve MSC, completion, Idle and buffer ownership, and measure
     first-visible latency.
   - Client CPU in the empty-workspace sample is an observation, not a
     proven Present cost.

3. **Diagnostics as aggregates.**
   - Several records are written per presented frame: submission, retire,
     scanout and delivery.
   - Suppressed-record deltas, from the before/after health files of each
     run in `t278-live-cpu-01`. Each health interval spans the 10 s grace
     period plus the 60 s sample (synchronized_boot_msec delta in
     parentheses):
     - accepted active `ghostty-codex-02`: 39503 (70283 ms);
     - static `ghostty-codex-01`: 7168 (70608 ms);
     - `ghostty-claude-01`: 6882 (70475 ms);
     - `empty-dp1-01`: 16520 (70573 ms).
   - Suppression counts alone do not show where formatting or filtering
     CPU is spent. Savings are judged on measured attempted, emitted and
     suppressed records together with the profile.
   - Replace per-frame records with periodic counters where measurement
     supports it, keeping exceptional records.
   - Retain typed failures and bounded lifecycle and correlation evidence,
     including the t279 incident trail.

4. **Investigate and improve partial-damage effectiveness.**
   - X Present update regions already become source damage
     (`sophia-x-authority` render_resources, `present_source_damage`).
     t278 added damage history.
   - Observation, not a diagnosis:
     - the `sophia_live_render_work` record at `uptime_msec=2067336`
       (about 34 minutes into the 9de41ea9 session) reports
       `composition_full_frames_count=60836` against
       `composition_partial_frames_count=761` (98.76% full);
     - that raw record is retained as
       `t278-live-cpu-01/render-work-full60836.txt`, copied from the
       session's rotated `events.2.log`;
     - these are cumulative context counters, and context replacement can
       reset them (render_metrics.rs keeps the newest context observation);
     - it is not an eligibility-adjusted matched-workload sample.
   - Measure interval deltas with context and reset handling, repaint-area
     ratios and per-reason counts for full repaints.
   - Compare eligible unchanged-geometry frames separately from legitimate
     full repaints. Candidate causes include buffer age, history
     invalidation, scaled or noncanonical variants, and clients sending no
     update region.
   - Keep the terminal-identity and provenance guards. Any fallback change
     needs pixel-equivalence proofs on both render nodes, never the removal
     of a correctness check.
   - A safe no-change result remains possible.

5. **Shared per-card renderer worker as the default.**
   - The worker is opt-in today (`SOPHIA_ENABLE_SHARED_RENDERER_WORKER=1`)
     "until promoted on physical evidence".
   - Measure thread count, context switches and CPU per frame with it on and
     off.
   - A shared store also makes same-card cross-head preview snapshots
     unnecessary (t284).
   - Promotion also requires checks of mixed-refresh fairness, latency,
     resource bounds, and failure and recovery.
   - Keep the private-store tests and the opt-out. Shared mode must not
     hide custody defects.
   - Promote it only on matched live evidence.

6. **Automated CPU regression gates.**
   - Counter assertions in a headless session, with a generic fixed-rate
     Present client per rule 13 (no named product): no capture-context or
     pipeline creation after warm-up, raster reuse, and the expected
     partial-damage extents.
   - A `cpu` scenario in `tools/qemu_session_harness.sh`: three 60 s samples
     against a stored baseline, giving owner and render CPU per frame. The
     virtual GPU gives relative regression signals, not hardware numbers.
   - The existing render-node benches on both cards, extended to whatever
     parts 2-5 change.
   - Fixtures keep their offered workload fixed and report completed
     throughput, so a reduced frame rate cannot masquerade as CPU savings.
   - QEMU remains a relative signal. Live acceptance stays with the
     operator.

## Connections

- [Renderer performance investigation](../investigations/wwr7oaer-reduce-per-frame-capture-and-cpu-raster-cost-without-reusing-live-image-storage.md)
  (t278, closed) and the
  [idle wakeup investigation](../investigations/qvrk2298-remove-timer-polling-from-idle-desktop-workers.md)
  (t276, closed).
- Evidence: `development-evidence/t278-live-cpu-01` (FINDINGS-01 with its
  correction, ROOT-COMPARISON.json, FORMAL-ACCEPTANCE.txt).
- t284 cross-head preview custody
  ([investigation](../investigations/qrmqf4gy-alt-tab-exposes-missing-cross-head-retained-image-custody.md)),
  for part 5.
- t050 measured rendering follow-ups, which absorb any residual scaled-variant
  work from part 4.
