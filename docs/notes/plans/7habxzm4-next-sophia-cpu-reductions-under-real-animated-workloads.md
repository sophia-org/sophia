---
id: 7habxzm4
date: 2026-10-02
kind: plan
status: active
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

## Current assessment (2026-10-03)

T289 remains active. The CPU harness and repaint attribution are qualified, and
retained capture-config reuse has a measured benefit on both render nodes.
Capture process CPU fell 39–40% at unchanged 60 captures/s in three paired
samples per node. Whole-desktop savings remain unmeasured. The qualified CPU
branch starts at master `ae5999940`; integration and release are still pending.

| Original part | Completed work | Remaining evidence or decision |
| --- | --- | --- |
| 1. Profile and reduce owner work | Profile-guided shell service once per pass; native fd readiness and removal of the polling tail are on master. | Measure the combined savings and remaining deadline wakes; physical VT acceptance. |
| 2. Hidden Present pacing | Bounded pacing, clock scheduling, race repairs, clockless recovery and first-frame selection are on master; hardware binds were observed live. | Matched CPU and first-visible/timing acceptance on the final release. |
| 3. Aggregate diagnostics | Opt-in aggregation is on master; full evidence remains the default. | Off/on CPU comparison and verifier compatibility before any default change. |
| 4. Partial damage | Actual-age repaint causes and client-damage counters passed component, pixel and guest checks. | Reproduce the full-screen terminal case and identify an avoidable cause, or record a measured no-change result. |
| 5. Shared renderer worker | The optional path exists. | Compare private and shared workers, including fairness, latency, resources and recovery; no default promotion yet. |
| 6. CPU regression gates | Valid normal-session QEMU baseline, deterministic analyzer checks, and render-node capture/pixel comparisons. | Use these gates for subsequent candidates; keep final hardware acceptance. |

The three priorities below now have concrete outcomes: native readiness is
merged, repaint attribution is qualified, and capture configuration reuse is
qualified with separated before/after CPU and latency ranges. Source-import
reuse or destination pooling would need separate ownership proofs.

**Next experiment:** use the new attribution with a generic near-output-size
Present workload, comparing absent/full damage with small explicit regions.
The small-window guest already proves partial repaint works; it cannot explain
the terminal's full frames. Qualify any resulting change with pixel proofs and
matched CPU/throughput results. Remaining owner-wake attribution, aggregation
and shared-worker comparisons can also proceed unattended.

The separate W1 hardware admission cache still needs integration with lock and
the pending/committed placement rules, then real-vblank measurement. The current
virtio guest uses Unclocked and cannot qualify that benefit. Final matched live
CPU, latency and VT checks remain acceptance work after an audited release.
The fence-waiter scrap parity issue is tracked separately as t300.

Earlier checkpoints below retain their original evidence and decisions; this
assessment is the current status. Nothing here closes T289 or authorizes a live
install or configuration change.

## Three priorities from the niri/XLibre comparison (2026-10-03)

The operator approved proceeding with priority 1 and recording all three here.
They remain slices of t289. Priority 1 is merged in local master as `ae5999940`;
its combined-tree gate passed. CPU savings and physical VT acceptance remain
unmeasured. Priority 2 now has qualified attribution; priority 3 has qualified
capture-config reuse. The full-screen repaint cause and whole-session savings
remain open.

### Evidence and scope

The installed Sophia is `53b283370`, release
`niltempus-8eaea15d0d13f4a0be09`. Three active Kitty/Herdr/Codex samples with btop
on DP2 (`t289-live-cpu-02/SUMMARY.json`) give median Sophia CPU of **8.05% of one
core**, owner **3.64%**, render threads **2.23%**, and about 41 completions/s.
Gross process CPU is 1.96–1.97 ms per completion, not a render duration or a
keystroke cost. There is no valid same-session empty-DP1 subtraction: series 03
was interrupted and remains invalid. These are current-release observations,
not paired T289 acceptance or measurements of niri/XLibre.

Read-only comparison identities: niri `5f4469b6`, its pinned Smithay
`79bbed5e`, and XLibre `dd5edd03`. Full paths, digests, derived measurements and
pF's reviews are in `development-evidence/t289-niri-comparison-01/`:
`REPORT.txt`, `source-identities.json`, `derived-metrics.json`,
`smithay-sources.json`, and `pF/`. The local proof-only lock merge `c3e054585`
adds no rendering changes and is separate from the measured installed revision.
No speedup or cross-compositor superiority follows from source inspection.

### Priority 1: native completion readiness and bounded owner progress

**Finding.** Sophia's native work selects a 1 ms owner wait. The wait subscribes
shell descriptors and the owner ring, but no DRM descriptors. Page flips are
read by the owner during service. After work ends, a four-visit idle tail keeps
the short wait armed. Run 01 recorded 29,745 owner passes and 13,672 deadline
wakes for 2,484 completions: about 12 passes and 5.5 deadline wakes per completion.
The counters do not separate polling, the idle tail and other held work.

[Owner wait selection](../../../crates/sophia-session/src/live_session/owner_loop/authority_receive.rs),
[service and idle tail](../../../crates/sophia-session/src/live_session/owner_loop/authority.rs),
and the separate [service wait](../../../crates/sophia-session/src/live_session/owner_loop/authority_service_wait.rs)
are the implementation seams. The last also sleeps up to 1 ms, outside the
ordinary wait counters. Niri registers its DRM notifier with calloop; XLibre
modesetting registers DRM readability with `SetNotifyFd` and requests kernel
sequence events. Their existing mechanisms motivate this change.

**Implementation.** Subscribe borrowed native completion descriptors, retaining
one event consumer. Separate runnable work, event waits and deadline waits.
Preserve watchdogs, rendering, cursor, recovery and topology deadlines. Remove
the four-visit tail only when every successor obligation has a wake, immediate
service turn or deadline. Keep an explicit short fallback for fd-less work.

**Correctness constraints.** Subscribe only when the next pass consumes the
source; quarantine and paused/revoked devices must not leave a permanently ready
fd in the wait. Handle POLLERR/HUP/NVAL as a state transition, and remove a
signalled out-fence when consumed. Preserve service preemption under saturated
authority ingress: queued authority work bypasses poll, so descriptor readiness
alone cannot guarantee progress. The retired-owner path must retain its own
bounded shutdown contract. Include lock cover and VT release in validation.

**Validation.** Cover readiness before/during wait, multiple cards and mirror
heads, out-fence cleanup, lost-event watchdogs, renderer-only and cursor work,
quarantine/rollback/VT transitions, saturated ingress and hidden/Fake/clockless
Present progress. Count readiness with and without consumption, fallback/deadline
reasons and service waits. Idle must return to the 25 ms maintenance cadence
without a polling tail. Negative controls restore polling/tail or retain a
stale fence and must fail the relevant assertions. Compare owner CPU, passes,
wakes and latency per completion at unchanged throughput; savings are unmeasured.

**Native-readiness implementation checkpoint (2026-10-03).** The isolated
`performance/t289-native-readiness` worktree starts at `c3e054585`, including the
lock proof delta from installed `53b283370`. The later T294 merge is separate.
The implementation borrows one card fd per group with submitted work, plus
submitted singleton and mirror out-fences. Already-collected callbacks,
rendering, recovery and cleanup keep short service. Session subscribes only with
an active seat, an available runtime and frame service permitted by topology.
The completion pump remains the only reader; readiness grants no presentation,
Complete or Idle permission.

The wait list is built once per owner pass and dropped before native mutation.
Its deadline is the earliest outstanding 500 ms page-flip watchdog. Cursor and
worker retirement keep the named 1 ms fallback; `NativeRetirement::poll` only
finishes renderer shutdown after KMS ownership has drained, so it does not
subscribe retiring card fds. The service-only wait now listens to the owner ring
and native readiness without consuming authority traffic. Periodic preemption
still services native work when queued authority bypasses poll. Completed work
has no four-visit tail.

A native descriptor error wakes once. After seat/lifecycle processing, an error
on the same still-active owner is classified as
`native_completion_descriptor_failed`; suspension or replacement discards that
old owner's fault. A signalled out-fence is closed on presentation before its
submission becomes displayed custody. This prevents permanently readable
fences from entering the next idle wait. If a signalled fence remains submitted
because its synthesized callback was refused, its cached Signaled status selects
short service instead of another fd subscription. The watchdog and custody stay
unchanged. New singleton and mirror submissions reset the cached observation,
so the predecessor's signal cannot suppress their readiness. This adds no poll
syscall to wait construction (pF review F1).

The existing periodic `sophia_present_clock_service` record now also carries
`native_ready`, `native_ready_consumed`, `native_ready_idle`, `native_errors`,
`native_event_waits`, `native_short_waits` and `native_service_waits`.
“Consumed” means a successful card drain or an out-fence retirement on the same
native owner before the next observation; it is not a presented-frame count.
Wait classifications count attempts, and readiness counters count poll returns.
Shell fd readiness stays separate. A ready-but-idle count exposes repeated
readiness without progress. No new per-frame records are added.

Evidence is in `development-evidence/t289-native-readiness-01/`. Unit tests use
socket descriptors and real custody transitions without opening DRM devices.
They cover wake ownership, revoked fds, watchdog waiting, no polling tail,
ingress fairness, multiple card/mirror membership and out-fence closure. These
are component proofs; attended VT switching, hardware timing and CPU savings
remain unmeasured. The earlier selector-only test invocation that ran zero tests
is preserved as a non-result; named test runs supply coverage.

### Priority 2: explain full-repaint decisions, then improve damage

**Finding.** Run 01 composed 2,747 full frames and zero partial frames. All full
outcomes were `PlanFull`; missing/unknown buffer age and history counters were
zero. All 2,391 stable-geometry frames were full too. Output composition repainted
about 163 million pixels/s, excluding the additional capture copies.

This does **not** prove a damage bug. An absent X Present update region means the
whole pixmap. A broken precise-history chain, rebased candidate, noncanonical
variant, clipping, transform or the coverage/rectangle thresholds can also
legitimately produce full damage. Stable geometry is not a pixel-provenance proof.
See [source damage](../../../crates/sophia-x-authority/src/runtime/render_resources.rs),
[history](../../../crates/sophia-engine/src/frame/damage_history.rs),
[Present rebase](../../../crates/sophia-engine/src/runtime_driver/production.rs),
and [repaint planning](../../../crates/sophia-engine/src/compositor_graphics/frame_presentation.rs).
Smithay uses element identity/commit damage, opaque-region subtraction and buffer
age, and skips elements with no damaged visible area; Sophia already has history.

**Next step.** Use the implemented aggregate reasons for client damage, area,
rebase, precision restrictions, history identity and repaint thresholds on a
near-output-size workload. The counters attribute the age actually rendered.
Repair only a measured avoidable cause.
If the client provides full damage, record that outcome without inventing a
smaller region or disabling its animation. Preserve all identity checks.

**Validation.** Pixel equivalence on both render nodes for buffer reuse, partial
updates, rejected/coalesced candidates, history exhaustion, scaling/clipping,
rotation, transparency, mirrors and previews. Track repaint-area ratios and
per-reason deltas with reset handling. A measured no-change decision is valid.

#### Repaint attribution implementation (2026-10-03)

This slice preserves the existing reduction, identity checks and repaint
thresholds. Engine carries a bounded set of causes through the same reduction:
geometry/order/compositor/cursor changes; sampling restrictions; missing or
nonmatching history; generation, terminal identity and coordinate failures;
rectangle pressure; and Present rebase. A failed precision proof reports its
first refusal for that surface. Several surfaces can contribute different causes.
`rebased` means a rebased edge was walked, even if a later check refused
precision; it is not a successful-precision counter. The preparation identity
retains a rebase marker only for its affected surface.
Client generations or buffer handles still cannot substitute for pixel identity.

The native per-age table carries these causes plus the exact full-plan threshold
(capacity, rectangle limit or coverage). Counters advance only after a successful
render, for the EGL age actually selected. Unknown/unretained ages have no Engine
cause evidence. Each cause counts at most once per output frame, with a separate subset
for full frames; causes overlap and must not be summed as a frame partition.
The existing `damage_full_plan_count` is retained and equals its three new
subreasons plus the legacy unspecified subreason. Planning unused ages counts
neither rendered causes nor rendered frames; the older slot planning metrics
still count planned ages and remain a distinct measure.

At the existing five-second cadence, `sophia_live_damage_causes` reports those
rendered causes. `sophia_present_damage` reports an independent cohort of
successfully executed Pixmaps: absent update region, explicit region containing
a full clipped rectangle, other explicit regions, or empty effective damage.
It also reports pixmap area and clipped rectangle count/area. Rectangle area is
a sum, including overlap; fragmented regions that together cover a whole pixmap
remain in `explicit_regions`. No extra union calculation or per-frame log is
added. Prepared, scrapped, cancelled and rejected requests do not count as executed.

`tools/present_cpu/damage_attribution.py` compares records with identical
observation times in an explicit interval, rejects resets/missing fields and
checks frame/subreason accounting. Full frames without a usable reduction are
reported as a separate reason bucket, not inferred from an empty cause mask.
The caller must select a single uninterrupted Session/native-owner lifetime;
monotone counters alone do not prove owner continuity. It reports coverage and keeps the executed
Pixmap cohort distinct from composition. No compositor or client behavior is
changed to make the workload cheaper. The small core-pixmap guest is a control;
full-screen terminal damage and DMA-BUF capture still need their own evidence.
Qualification is recorded in `development-evidence/t289-repaint-attribution-01/`.

Qualification passed on freeze-02 `6ae323ac`: the full gate, 28 Python tool
checks, and partial/full pixel equivalence on both render nodes. Final build
`sophia c6edfe34` passed two ten-second normal-session QEMU probes. The aligned
five-second counter intervals contain 100 and 2,732 partial frames, zero full
frames, and 50 and 1,366 executed absent-region Presents respectively. The
only nonzero cause is `precise_surface`; repaint pixels are 1.27% of the summed
output target pixels. That denominator includes the unchanged second output;
its zero-damage partial frames carry no `precise_surface` flag. Causes count
output frames, independently of the executed-Pixmap count.
The three record streams agree, and one native-owner lifetime spans each run.
These small core-pixmap windows do not reproduce the full-screen terminal
finding. They establish the attribution path, with no CPU saving or capture
claim. `SUMMARY.json` and `GUEST-ATTRIBUTION.json` retain the scope and identities.

### Priority 3: reduce capture/import setup while preserving immutable pixels

**Finding.** Each measured Present created one fresh owned snapshot surface.
Run 01 recorded 2,484 captures, 5,228 imports and only three cache hits; capture
contexts already reused. The renderer probes a client DMA-BUF import, destroys
that EGLImage, captures through a fresh one-entry cache, clears that cache, then
imports the owned snapshot for output composition. Distinct Present transactions
name distinct snapshot images. Low hits therefore fit the ownership design.

See [capture](../../../crates/sophia-renderer-native-egl/src/gbm_platform/scanout/context/image_capture.rs)
and [owned composition](../../../crates/sophia-renderer-live/src/native_scanout/owned_mixed_export.rs).
Smithay retains imported textures by DMA-BUF lifetime; XLibre glamor attaches
imports to pixmaps. Sophia's owned images keep retained content immutable after
the client can reuse its buffer. A different cache key alone cannot replace that.

**Outcome and follow-up.** The measurements below identified configuration lookup,
and reuse removes most of that cost. A further experiment could evaluate reusing
the validated import for capture or a bounded source-import cache tied to backing
lifetime, descriptors and device identity. Pixel generation remains distinct from storage identity; fence
and transfer semantics stay intact. Destination BO pooling or capture removal
needs a separate proof covering every local/foreign reader and GPU operation.

**Validation.** Client buffer and XID reuse, changed plane descriptors, eviction
with foreign readers, reset, capture errors, fences, mirrors and cross-device
transfer. Preserve exactly-once Complete/Idle and memory bounds. Measure imports,
allocations, copy cost and total CPU at fixed completed throughput.

#### Capture cost attribution and config reuse (2026-10-03)

`capture_cost.rs` now enables the existing stage timers in the offscreen fixture
and reports calls, setup/copy/cleanup, context reuse, imports and deadline misses.
Each capture still allocates a fresh image, promotes it and evicts it. The test
requires no surviving images or imports, exact capture/promote/evict counts,
and no capture or transfer failures. Comparison requires this exact instrumented
fixture; historical timing-off results are not comparable.

`t289-capture-attribution-01/` contains three ten-second samples per render node
at 60 captures/s after 120 warm-up captures. Both nodes passed, with zero late
completions. At 1280x1440 XR24, median calling-thread CPU was 320.19 and 319.94
microseconds/capture (`renderD128` and `renderD129`); process CPU was 340.20 and
337.29. Setup cost about 4 microseconds, copy 123–124, cleanup 30–32. Every image
still had a fresh surface and one counted import/eviction. Validation-probe
imports are outside that import counter and remain included in total CPU.

`t289-capture-attribution-02/` separates the public calls: promotion was about
0.6 microseconds and eviction 3.5–3.7, with 3.7–4.0 of fixture-loop CPU. The
uncovered 153–160 microseconds lay inside capture. Other process threads added
about 18–19 microseconds; kernel/GPU work and deferred driver cost are not
assigned by these calling-thread brackets. The two diagnostic probes are not a
before/after comparison. A permitted 30-second userspace profile of the offscreen
fixture collected 446 samples, none lost: `choose_scanout_config_for_format`
accounted for 55.38% inclusive. Inclusive shares overlap; the profile includes
initialization and warm-up and establishes a target, not a predicted saving.

The candidate in `t289-capture-config-01/` reuses the chosen EGLConfig from the
existing two retained execution contexts. The key includes format and the exact
13 config attributes, with the EGLDisplay fixed for that native owner. Equality
performs no EGL call. The slot dies before its display terminates; failed capture
cleanup does not retain it. Cold selection and modifier/surface fallbacks remain.
The attribute guard is defensive: current candidates use fixed attributes per
format, so ordinary tests do not exercise a changed attribute array.

This caches configuration only. Actual source-descriptor validation, source
imports, fresh destination surfaces/BOs, copy/flush, fences, transfers and immutable
snapshot custody are unchanged. The candidate was qualified against the
saved instrumented baseline binary with an unchanged timing fixture. The warm-path
regression requires zero extra selector calls after both format slots succeed;
cold attempts vary by device and are not incorrectly pinned to two calls.

The combined full gate passed on freeze `baba6daa`. Capture/custody, lifetime
and transfer pixel tests passed on both render nodes. Forcing a selector search
on every capture failed the named warm-cache assertion; the original source
was restored byte-for-byte. The first cold-count assertion and a missing
lifecycle-test environment variable remain in the evidence with their corrections.

Three ten-second baseline/candidate pairs per node (baseline first in two of
three pairs) all sustained 60 captures/s with zero late completions. Each sample
retained 600 new surfaces, imports and evictions, zero new pipelines or contexts,
and no remaining images/imports. Medians from `COMPARISON-METRICS.json`:

| Render node | Process CPU/capture before | After | Reduction | p99 wall time before → after |
| --- | ---: | ---: | ---: | ---: |
| renderD128 | 346.25 µs | 207.08 µs | 40.19% | 478.07 → 332.41 µs |
| renderD129 | 344.79 µs | 209.10 µs | 39.35% | 521.85 → 303.81 µs |

Calling-thread CPU fell 41.8–42.5%. All candidate samples were below all baseline
samples for thread/process CPU and p95/p99 wall time on both nodes. The uncovered
capture cost fell from about 160 to 21 microseconds. Configuration lookup is now
a measured optimization; source-import reuse and destination pooling remain
separate proposals. These are instrumented local capture results, not GPU time,
whole-session CPU savings, cross-device throughput or live acceptance. The
existing desktop was neither profiled nor reconfigured for these runs. This
slice is qualified on the CPU branch; master integration and release remain pending.

A secondary opportunity is unchanged CPU-layer upload: raster reuse does not
mean GPU upload reuse. The current scratch texture uploads each CPU layer again,
and each damage rectangle visits the layer stack. Measure uploaded bytes and
empty intersections before introducing a bounded content-generation texture cache.

### Comparison and acceptance rules

Implement and qualify one slice at a time. Use the same offered workload and
client features, plus generic fixed-rate fixtures. Keep completed throughput,
Copy/Flip/Skip mix, latency, errors and memory occupancy beside CPU and counters.
Cross-compositor comparisons require equivalent visuals and include any separate
X compositor's CPU. QEMU is a correctness/relative signal; hardware acceptance
stays with the operator. No live install or configuration change is implicit.

### Unattended qualification approved (2026-10-03)

The operator approved implementing the remaining slices without a live check
between each change. Build the repeatable headless CPU gate first, then attribute
full repaint decisions, measure capture/import setup, and qualify each justified
change. W1, aggregation and shared-worker experiments follow with their stated
boundaries. A valid measured no-change outcome remains allowed. The final live
acceptance is deferred until a tested release is ready; it does not block ordinary
implementation or unattended component/guest qualification.

The first slice reuses the reviewed generic core-pixmap client and proc sampler
from the W1 worktree without its cache implementation or hardware-only validity
rule. `tools/present_cpu/` adds a general CPU scenario with fixed-rate and
Complete/Idle-paced workloads. Two ten-second probes precede three 60-second
samples per workload. Store the kernel, guest, client, source and environment
identities. Comparisons require matching offered work and report throughput,
latency and event modes beside CPU. Missing counters, resets and protocol loss
invalidate a run; median CPU/Complete growth over 10%, throughput loss over 2%,
or p95 latency growth over 10% flag a valid comparison for review. These bounds
are declared before running the campaign, not chosen to accept its result.
Read-only review added whole-guest CPU and steal accounting: over 1% steal or
over 20% unaccounted CPU invalidates a run; unaccounted-share growth above five
percentage points invalidates a comparison. The preliminary campaign is retained
as diagnostic-only because it lacked these controls. Owner attribution uses an
explicit emitted TID; image packaging requires a source-bound build receipt.

Harness qualification exposed measurement and transport faults, preserved in
`t289-cpu-gates-01/`. Streaming full records through the emulated UART added
4.44 seconds of IRQ work in a ten-second probe. Direct tmpfs logging reduced
that to 0.07 seconds; all records are now exported losslessly after measurement.
An uncompressed export then timed out, so the export is bounded gzip/base64 with
host-side integrity checks. Neither change reduces the offered client workload.
The first long closed run then exhausted space on the unpacked initramfs during
export and panicked its test init. It remains INVALID. Log artifacts now have a
separate 256 MiB tmpfs; compression streams without a second copy. Workload and
size/hash metadata precede bulk export, and an export failure returns a failed
result without exiting PID 1. Both CPU snapshots record guest memory and log size;
less than 256 MiB MemAvailable or more than 128 MiB of logs invalidates a run.
Export wall time and final tmpfs usage are retained. A long export subsequently
showed console watchdog messages interleaved into the encoded payload. Bulk
evidence now uses a dedicated virtio-serial host file, separate from the kernel
console. Any kernel stall invalidates the run. These are harness changes;
the Session tracing volume and offered workload remain unchanged.

The guest's tick-sampled CPU totals also disagreed with task runtime: one closed
probe reported 37.44 seconds across four vCPUs during a ten-second interval.
All task ticks summed to 15.42 seconds, exceeding the guest's 13.26-second busy
tick sum. The harness now captures per-CPU and per-task scheduler nanoseconds,
using capacity minus idle, iowait and steal as the whole-guest busy estimate,
with scheduler totals as a cross-check and tick totals retained as diagnostics. Kernel workers and interrupts remain part of guest cost.
Attribution follows PID/TID plus start time, so changing a workqueue name does
not lose its CPU. The 20% unattributed limit is unchanged; a comparison also
rejects growth over five percentage points outside Session/client, attributed
or otherwise. A saving claim requires both Session and guest CPU to improve.
The first full open sample also exposed intentional proof-mode 1 ms owner
polling. The recipe now uses the existing normal-session startup/exit lifecycle;
the analyzer requires Session-emitted normal mode and successful app exit.
No production scheduler bypass is added.
Qualification is recorded in `t289-cpu-gates-01/baseline-05/SUMMARY.json`
(`BASELINE_RECORDED`): both probes and three 60-second samples per arm passed on
freeze-04 `057f36ed`, build-07 and image-09. All measured binds were Unclocked,
all completions Copy, with Complete/Idle custody and no outstanding buffers.
The earlier attempts remain rejected or diagnostic-only; none supplies a saving
claim.

Baseline medians for the two 320x240 core-pixmap windows:

| Workload | Completed/s | Session CPU ms/Complete | Whole-guest CPU ms/Complete | p95 latency |
| --- | ---: | ---: | ---: | ---: |
| Fixed offer, 5/s per window | 10.00 | 5.256 | 7.604 | 10.04 ms |
| Complete/Idle-paced | 272.95 | 3.833 | 5.854 | 7.71 ms |

These are reference costs, not physical display rates or a before/after result.
Whole-guest accounting retains kernel cost: 1.14–1.91% of busy CPU was
unattributed across the full samples, maximum per-vCPU steal was 0.0334%, and
minimum guest MemAvailable exceeded 740 MiB. Long closed logs were about 64 MB
and exported through virtio in about 0.1 seconds. Readiness with nothing consumed
was zero. The deterministic accounting/comparison/export tests now run in
`xtask check` and `xtask check present-cpu`; measured QEMU trials stay explicit.

Priority 2 attribution is implemented and qualified above: it distinguishes client
damage, precision/history restrictions and the exact repaint-plan threshold at
the age actually rendered. This guest already repaints its small ordinary surfaces
partially, so it does not reproduce the full-screen terminal finding. Priority 3
has separate DMA-BUF capture timing and pixel proofs above; this core-pixmap
baseline records no snapshot captures/imports. W1 hardware-cache benefit and
final live acceptance remain open.

The periodic clock and renderer records gain a monotonic observation timestamp
on their existing cadence, preserving schema and prefixes. This lets the analyzer
exclude startup counters. No frame-service or rendering behavior changes in this
slice. A software virtio guest can qualify the Unclocked path and relative CPU
changes; it cannot establish W1 hardware-cache benefit. GPU pixel/capture checks
use separate offscreen render-node proofs. Physical VT, mixed-refresh timing and
final desktop acceptance remain later checks. No live configuration or install
is part of this approval.

Evidence starts at `development-evidence/t289-cpu-gates-01/`; baseline-05 records
the qualified reference above. Future measurements use coordinated quiet windows;
source/build work follows the ordinary parallel build policy. Existing Cargo
targets and guest dependencies are reused because available disk space is limited.

## Task details

<a id="t289"></a>
**t289: next CPU reductions, admitted by the operator on 2026-10-02.** One task in six parts. Part 1
orders the experiments by measured cost. Any part may end in a measured
no-change decision; this plan does not pre-approve six production changes.

1. **Profile-guided owner-thread optimization.**
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
   - The operator authorized reducing repeated shell 9P service under this part.
     Use the retained `profile-01` userspace sample first; do not repeat live
     profiling merely to start implementation. Separate transport service from
     buffered role access, keeping readiness and deadlines authoritative.

2. **Present pacing for windows nobody can see.**
   - A hidden Ghostty kept submitting about 18 Presents/s while Sophia
     composed nothing of it. In that window the client's CPU was 3.53% of
     one core. How much of that, or of Sophia's owner work, the Presents
     cause is still unmeasured.
   - Uncaptured Presents are already parked to the head's refresh (the
     settle-uncaptured path). The proposal is a slower, bounded completion
     cadence for surfaces with no visible sampling output, restoring
     immediately when the surface becomes visible.
   - The operator approved a one-second background interval, with early release
     as soon as visibility returns. Reuse the existing bounded parked scheduler.
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

## Implementation and qualification decisions (2026-10-02)

The operator approved implementation of the six-part plan. The session-control
crash repair t290 is an independent prerequisite for further live qualification.
Driver: Codex, with source review before publication. Keep the accepted animated
workload unchanged and each measured no-change decision explicit.

Land bounded transport work first, then aggregate diagnostics and actual-render
fallback counters, background pacing, measured damage improvements and the shared
worker comparison. Seed generic regression fixtures before optimizing. Internal
APIs and diagnostic schemas may change; no WM or SDK wire change is planned.

Use three paired 60-second samples. Initial relative regression thresholds are
10% median CPU per completion and 2% visible completed-throughput loss; noise
requires investigation. QEMU is a relative signal. Keep hardware orchestration
in niltempus and generic fixtures in Sophia. Operator live acceptance remains
required. No client configuration, live install or session restart is implicit.

### Historical experiments and boundaries at the 2026-10-02 checkpoint

- Part 2 includes a Present timing repair before its slower interval can ship.
  At the baseline, the completion clock was global, hidden NotifyMSC had no
  independent progress, and PresentPixmap dropped its requested target MSC after
  parsing. Divisor/remainder scheduling was absent. The WIP now schedules both
  requests; the component and socket proof boundaries are recorded below.
  A frozen per-window completion sample was rejected: it would strand hidden
  NotifyMSC waits. Valid nonzero target CRTCs are currently refused and remain
  an explicit audit item. UST is now explicitly refused rather than interpreted
  as MSC; UST capability stays unadvertised. No scheduling or slower cadence
  is qualified yet.
  The preparation slice now freezes regions and retains the original pixmap
  backing through FreePixmap and XID reuse, without changing window generations
  or pixels until execution. Fence handles are resolved privately; destruction
  detaches only those handles. Destroy and disconnect release socket reservations
  as well as prepared backings; a save-set survivor keeps its request. The shared
  per-client limit is 64, and preparation has a separate global limit of 256.
  UST requests and unsupported 1.4 MayTear options are explicitly refused with
  BadValue after resource validation. UST conversion remains a limitation.
  A pure clock and queue model covers source changes, wrap, modulus scheduling,
  and a one-field hardware preparation lead. Fake clocks, Skip and NotifyMSC do
  not get that lead. Each queued request retains its original source and offset;
  moving a window changes only the clock selected for a new request. A queued
  fake-clock request stays on the one-second clock after becoming visible.
  Only a new full update scraps queued pixmaps with the same target and binding.
  It signals the old private idle fence and releases the backing immediately,
  then owes Idle delivery and Skip completion at the old target. Scrapped
  completions remain charged to both bounds. Requests whose MSC event was
  serviced and which now wait on an acquire fence are not scrapped either.
  A Skip reports the first actual observation at or after its target, which can
  exceed the target when observations coalesce. A lost-source Skip can report
  below its target; neither case invents a target-time vblank or timestamp.
  Sophia deliberately settles queued work on a disabled/lost source at its last
  actual observation: pixmaps become Skip, and NotifyMSC retains its kind. This
  avoids migration or waiting forever; it is not a claim about kernel flushing
  of XLibre's queued events. The queue model and supplied-source service prove
  this disposition; the Session loss reconciliation is described below.
  Timed execution now uses a fresh ordered ticket, an atomic feedback-reservation
  transfer and the original private fences. A rejected execution publishes an
  empty ticket and keeps its Skip obligation. Raster and timed production have
  separate bounded egress slots, alternate first visit, and deliver by ticket.
  Executed requests retain their frozen binding, execution-time sample and recent
  accepted observations in the feedback reservation. While the source is live,
  a chosen-head retirement keeps its supplied sample. An off-head retirement
  uses the latest retained bound sample
  no newer than the event's monotonic UST, else the execution sample; a lost source
  uses its last accepted sample, including when a retiring owner's final flip
  still names the old source. Loss takes precedence over matching that callback
  and never revives the binding; Complete and Idle retain separate permissions.
  Accepted observations also maintain the persisted window clock while only
  executed requests remain. Loss freezes that continuity anchor under the same
  runtime lock as rebinding. A window rebound after loss therefore continues
  from the accepted sample (for example MSC 15), rather than an older execution
  sample. Reporting a later old-owner flip at MSC 19 could exceed that new
  source's starting point. A post-loss Complete deliberately can under-report
  its display time: UST and MSC both come from the same last accepted sample.
  A late old-source observation, including a supplied previous-source sample,
  cannot advance the lost anchor. Loss of an older binding does not rewrite a
  window that had already rebound; the independent-rate boundary below remains.
  Source observations use a small runtime-published interest index before
  acquiring the authority runtime lock. Queued sources need that lock; an
  executed request needs it only while its full binding still matches the
  persisted window binding. Otherwise feedback history updates alone. Index
  changes publish before admission/rebinding releases runtime. Queued interest
  ends at settlement; the window binding persists until destruction, with
  runtime updates gated by outstanding matching Complete obligations. The index
  mutex supplies acquire/release ordering. Readers follow feedback-to-index order and release both before
  waiting on runtime. A dependency created after a negative read starts from
  its admission sample; its next observation sees the published interest.
  The periodic `sophia_present_clock_service` record includes `observations`
  and `observation_runtime_locks`: cumulative Session source-delivery counts,
  excluding demand/loss lookups and the frontend's already-locked fake-clock
  service. Their interval deltas measure runtime acquisitions per observation.
  Checkpoint 95 already took runtime for every source delivery before checking
  for an empty queue; this guard removes that acquisition on irrelevant paths.
  Each executed binding keeps four distinct accepted observations in fixed
  storage, plus its immutable execution fallback.
  A pass-entry query newer than a queued retirement therefore retains the
  pre-event observation. Repeated samples do not evict history; an event older
  than all four falls back to execution. It never waits for a future clock tick.
  The checkpoint-82 next-observation draft is superseded by REVIEW-11's addendum:
  waiting changed old retirements' timestamps and allowed them to be overtaken.
  Clock observations and loss never create Complete or Idle. Delivery is serialized
  through event enqueue, in the adapter's retirement order; client serials are
  opaque. A newer request with an earlier target may complete first. Each request
  retains its source offset, as in XLibre: source switching is continuous at the
  switch, but independent source rates can produce later cross-source MSC regression.
  No global clamp is permitted. Native integration must preserve per-card event
  order and stably sort retirements gathered in one pass by UST across cards. It
  cannot promise UST order against an earlier event still unread on another fd.
  Clock demand ends at Complete; Idle stays independent and is issued only when
  renderer custody permits reuse (after every copy and its release fence, regardless
  of PresentOptionCopy, or after scanout releases a flipped source). Supplied tests
  cover saved samples, off-head completion, source loss, cancellation, racing
  observers, earlier-target execution, and the deliberate cross-source counter
  boundary. Clocked layout advice uses the existing runtime try-lock: an unavailable
  authority conservatively withholds Suboptimal advice without blocking completion.
  The ordinary completion path is outside the timed service's runtime lock.
  Supplied-clock tests drive the real idle frontend service through its fake
  deadline and hardware-observation wake. No software prediction of vblank is
  used. The scalar execution record joins original and executed transactions.
  Native completion sample capture and routing are implemented in the WIP,
  with the proof boundaries below. Session now opts into timed wire admission
  for Pixmap and NotifyMSC. The service's wait-fence check
  uses the exact retained descriptor and backs off from 1 to 32 ms per request.
  Client writes to xshmfence shared memory do not necessarily issue an X request
  or make a descriptor pollable; this bounded retry also covers those signals.
  Unrelated service wakes do not reset the backoff. This can add up to 32 ms
  between a signal and its next check; native latency is still unmeasured.
  Invalid clock samples are rejected and counted per window, with no partial
  update of that window and no frontend shutdown for other clients. Three
  consecutive rejected observations of a binding retire its queued requests
  as Skip at the last good count. Unrelated clock observations do not reset
  the streak. Timing/Skip service continues under output-channel pressure;
  a ready request contributes no immediate wake while its egress slot is full.
  The native query adapter reads the kernel's 64-bit CRTC sequence, only on
  demand and only after checking the monotonic timestamp capability. Its
  supplied-observation tracker separates heads and invalidates counter
  lifetimes across modesets, rollback, disable, replacement and regression.
  Re-observing the same sequence with an earlier timestamp retains the accepted
  observation; it does not restart the clock or publish a regressed UST.
  Session now reconciles source loss at owner-pass entry and again after WM/
  topology work before the authority wait, outside the frame-service guard.
  A vanished source sends no bad samples, so the rejection limit cannot settle
  those obligations. The bridge drops vanished/replaced bindings even during
  quarantine or with no native owner. Its injected-source tests drive the real
  queue to one Skip for modeset, rollback and replacement identities; invocation
  from the full owner loop is still source-reviewed, not an attended proof.
  Both production calls pass the unfiltered active native owner option. None
  means headless startup or explicit owner retirement, even while disposal
  retains the old owner; a temporary borrow or frame-service quarantine does
  not mask it. The filtered owner options used for authority production cycles
  are separate and never reach clock service. Modeset clock loss comes from
  explicit invalidation; native replacement has a new owner domain.
  Before the first X client binds the frontend runtime, the real clock router
  reports no queue demand; empty Session startup is covered by a bridge test.
  Only queued hardware requests still waiting for MSC schedule repeated queries.
  Ready requests blocked on an acquire fence or egress, and executed bindings,
  remain visible to loss reconciliation without periodic queries.
  Long targets use the period between two real observations to estimate the next
  query wake, one field early and capped at 60 seconds. Sophia can enable VRR:
  an idle head's observed period may be longer than the rate when flips resume.
  The estimate is therefore bounded by the selected mode's fastest field period
  (the shorter of its nominal period and exact pixel-clock/totals period, rounded
  down, with interlace accounted for). Double-scan/vscan can only make this wake
  earlier. Synthetic selections with no mode use their admitted nominal rate.
  New earlier demand interrupts that wait.
  Estimated time never advances MSC. A 1,000-field injected-clock test takes four
  queries to reach its preparation lead, with no per-refresh queries in between.
  A slow-idle/fast-resume regression checks the VRR wake bound. The callback pump
  now preserves raw kernel sequence separately from normalized retirement serials.
  Only an already queried clock with TIMESTAMP_MONOTONIC and
  CRTC_IN_VBLANK_EVENT verified on its owned card fd, in the same target lifetime,
  accepts a flip observation, extending its low 32 bits from the 64-bit anchor.
  The event CRTC routes to the physical head; two mirror members keep separate
  counters even when they share a logical output. Missing/failed event capability
  checks disable flip observations and retain the GET_SEQUENCE path. Synthetic
  out-fence completions and drm-rs's user_data fallback do not feed the clock.
  Stale events and ambiguous half-range jumps leave the query timer in charge.
  Session consumes accepted progress on every reconciliation pass and re-arms
  the estimate without another ioctl. Supplied flip and queue tests cover this;
  native physical delivery remains untested; completion-sample routing now has
  the component proofs below.
  Native retirement evidence now travels with the exact frame/cohort. Each
  mirror cohort retains at most one sample per member (MAX_HEADS_PER_OUTPUT);
  a straddled request joins the evidence from its separate output cohorts.
  Selection compares full source owner/incarnation, never just a head number.
  The source is frozen at KMS submission. A callback collected after a reset
  cannot advance or borrow the replacement incarnation. Out-fence completion
  contributes no MSC. Empty/single-head evidence and Session conversion allocate
  nothing; multi-head bundles are immutable and shared with software completions.
  Decode and live advancement are separate: an event from the submitted
  incarnation resolves to the nearest low-32 extension within half a range of
  the 64-bit anchor, including backward wrap. A half-range ambiguity or opposing
  sequence/UST directions supplies no evidence. Equal sequence keeps the event's
  own UST. An older decoded event carries an explicit historical flag through
  each cohort, output join and Session adapter. It can supply exact Complete
  UST/MSC even after a newer pass-entry query, but never enters completion
  history, advances native/window anchors or queued work, or changes rejection
  streaks. This also holds when the frontend lags the native query. Only a
  historical tally changes, without acquiring runtime. Loss still takes priority.
  At each native service/drain, collected logical retirements are stably ordered
  by the permission head's UST across cards, before the usual output-id reducer.
  Mirror members are also consumed in UST order. Ties retain the previous
  output/head ordering. This covers gathered real events only; there is no
  watermark or claim about earlier events still unread on another card.
  Before native Complete delivery, a matching, non-lost event advances the
  persisted window anchor and queued work together under runtime -> feedback.
  Older/equal observations are silent historical no-ops, with no rejection
  streak. Rebound executed-only bindings skip runtime through the interest
  index. The periodic counters add completion_runtime_locks and
  completion_historical_samples; they are separate from source observations.
  Session feedback diagnostics and cadence use the routed window UST/MSC.
  Supplied tests cover the slow-primary mirror, absent-nonprimary no-wait
  fallback, both GPU and software output joins, incarnation reuse, out-fence
  fallback, completion then rebind, historical event after a newer query,
  backward wrap, direction mismatch, unchanged history and a lagging frontend,
  loss then a late event, and the cheap rebound path. No KMS or attended
  timing proof is claimed. The broader Session gate also required updating
  reconnect fixtures to issue the part-1 owner input turn before role service;
  those test-only loops previously suppressed input after negotiation.
  Native latency and variable-refresh behavior are unmeasured. Kernel sequence
  events remain a future alternative requiring coordination with DRM event reads.
  The periodic `sophia_present_clock_service` record counts bridge queries and
  all routed Pixmap Complete settlements (including Skip and legacy requests).
  Initial source-selection queries are not in that counter yet; their adapter is
  not wired. Ratios require a matched workload and that boundary to be accounted for.
  Source selection uses the union of ordinary and clipped preview coverage
  in logical desktop coordinates, largest area first. Ties prefer the
  configured primary output, then lowest output ID. A mirror group uses its
  primary head's clock, falling back to an active member if that head is
  disabled or its query cannot provide a current clock. Existing requests
  stay frozen to their original source. Source review corrected the mirror
  premise: the primary flip grants logical completion; sibling cleanup remains
  per head. This permission rule is unchanged. A slow-primary component test
  retains each member's own sample through that primary retirement. If the
  frozen source is a nonprimary member which has not flipped yet, Complete
  uses its existing no-wait history fallback. Idle permission is unchanged.
  These are supplied completion/custody tests, not a physical mirror proof.
  Duplicate preview coverage counts once; an unsampled
  window selects Fake. The selection and clock-lifetime component tests pass;
  Request-clock admission has a bounded runtime/router component, enabled
  at wire dispatch for Session. Retained Pixmaps and resource-free NotifyMSC expose
  opaque requests with optional surface/geometry targets; binding is once per request, in order
  on a window. A later request can choose another source without rebinding older
  targets. The private service delivers ready NotifyMSC without renderer work or
  Idle: no hardware one-field lead, no equal-target scrap, divisor zero current
  field and nonzero divisor next matching field. Fake targets arm their 1 Hz
  deadline; source loss settles once at the last accepted sample. Both unbound
  and bound notifies share the 256 global/64 per-client preparation cap with
  unexecuted Pixmaps. Executed Pixmaps separately retain the existing socket
  feedback reservation. Capacity is a typed admission refusal translated to
  BadAlloc at the socket, without retaining a refused request.
  Destroy cancels without events. Connection cleanup now cancels before the
  retain/destroy resource fork, including SetCloseDownMode retention. Component
  tests keep retained resources alive while verifying timing obligations vanish.
  The Session clock visits now bind admissions outside the frame-service guard,
  including before startup, during quarantine and with no native owner. Every
  unbound request is listed: an unmapped or unresolved target selects Fake and
  cannot block later requests on that window. Selection uses the actual sampling
  table once per batch and shares one query result per output in that batch.
  Valid rootless X windows (including the setup root and authority-owned windows)
  pass preparation; genuinely invalid or inaccessible targets still fail access
  validation. A pixmap with no renderer route at execution releases its pixels
  and settles timed Skip without allocating a renderer ticket. NotifyMSC stays
  resource-free. Binding uses a newer already accepted same-source observation
  if frontend clock service advanced while Session selected outside the lock.
  A bind passes `previous=None`, so a source switch anchors continuity on the
  window's last accepted old-source observation, including executed/retirement
  observations. A clock refusal retries once with a fresh Fake sample. If that
  also fails, the request stays charged in its preparation record until terminal
  feedback: Pixmap gets Idle and Skip at its last accepted paired sample (Fake
  only if it never had one); NotifyMSC follows the same saved-pair order,
  using current raw Fake only if none exists. This error
  settlement makes no display claim and does not repair or reset the window
  clock. Destroy/disconnect cancels terminal records through the existing path.
  A failed idle signal while scrapping settles only that older pixmap and still
  releases its backing and visits every later scrap. Failed signals are counted;
  no successful signal of a broken fence is claimed. The new request remains
  scheduled. Queue capacity/duplicate refusals assert in debug builds; release
  builds contain them as terminal feedback. Transport poison or missing runtime
  still fails explicitly. Normal binding does not read another host clock.
  `sophia_present_clock_service` carries bounded periodic admission error, Fake
  retry, terminal settlement and idle-signal-failure counters. Terminal records
  add no new queue: they retain the same 256/64 charges until delivery/cancel.
  Session enables timed admission explicitly on its broker. Standalone callers
  without a Session clock service retain the immediate path. Socket dispatch
  reserves Pixmap feedback before taking runtime; the two-second capacity wait
  therefore cannot block execution or cancellation on runtime. Routed preparations
  are constructed unpublished; direct component preparations remain ready.
  Publication is one-way and returns false after publication or cancellation.
  Both routed kinds stay hidden from clock admission until their original authority envelope has
  been published and request outputs handled. The publication then exposes the
  request and wakes the owner. Binding wakes the frontend. An earlier unpublished
  same-window request still blocks later binding until publication or cancellation.
  Rejected requests cancel their provisional reservation; disconnect cleanup
  cancels unpublished and published preparations through the same lifetime path.
  Wire tests cover request/error sequence, publication ordering, FreePixmap/XID
  reuse, deferred execution, exactly-once typed feedback, rootless Skip, bounds,
  destruction and disconnect. A fake-clock NotifyMSC ripens without another
  socket request. An eight-client batch executes sixteen requests without an
  owner round trip per request; earlier targets execute first. Its service-pass
  count includes setup and wake visits and is not a CPU or latency measurement.
  Checkpoint 150 adds periodic counts of wire preparations, publications,
  notification attempts, bindings (including hardware), and Pixmap executions.
  Preparation-to-execution duration has a cumulative sum and lifetime maximum;
  it includes target, fence and publication waiting, excludes earlier socket
  capacity waiting, and counts entry to execution even if execution then fails.
  Requests that never execute (including scrap, cancellation or unroutable Skip)
  have no execution duration. NotifyMSC is included in admission counts only.
  Owner-local counts distinguish passes, polls, ring readiness, borrowed-fd
  readiness, deadlines and immediate channel items. Readiness counts can overlap
  and include descriptors already ready at poll entry. Notifications, poll
  readiness and passes are distinct from OS scheduler wakeups. All counts are
  cumulative; compare deltas with reset handling. No CPU saving is claimed.
  W1 remains open before live qualification: reuse of a window's accepted clock
  alone cannot prove it is still the correct sampling source after layout,
  preview, translation, mirror or topology changes. The proposed cache needs a
  current sampling proof, invalidated before those changes, and preserves the
  original-envelope barrier and future-target query wake. Its source audit and
  matched workload/latency comparison remain outstanding; see evidence
  `t289-implementation-01/150-W1-design.txt`. No cached admission path is enabled.
  A new request admitted while every candidate is inactive during modeset will
  bind to Fake and keep that binding through settlement. Using fresh page-flip
  samples instead of querying when a NEW request chooses its source remains
  an optimization to measure.
  Hardware timing, compositor retirement and physical latency remain
  unproven. The slower interval stays unqualified until that integration passes.
- The pacing draft uses one global cap of 64 parked presentations and reclaims
  only parked debt against total renderer presentation occupancy before intake.
  Pressure is an explicit exception to the one-second delay. Source and fence
  registry exhaustion is a separate pre-existing limit, not solved by that cap.
- Part 3's bounded evidence aggregation is an **opt-in experiment** through
  `SOPHIA_PRESENT_EVIDENCE=aggregate`. Full remains the default. Diagnostic modes
  force full; `sophia_present_evidence` records the selected mode at startup.
  The physical verifiers that depend on log position and complete per-frame
  records need an explicit full-evidence launch/verification contract before
  aggregation can become the daily default. No such promotion is approved here.
  Emitted counters mean passed to tracing, not retained by the archive. X tails
  carry `observed_monotonic_usec`; native tails carry UST/MSC. Archive line order
  and archive timestamps are flush order/time in aggregate mode and cannot be
  used as a complete causal trace. Failures and lifecycle records stay immediate.
- Part 4 records fallback reasons for the **actual** EGL-selected buffer age,
  only after a successful render. Existing history-planning counters still count
  all planned ages and must not be used as frame counts. Full-only tables retain
  reasons but still draw one full pass; no provenance or damage rule changes.
  One reason per full render is recorded, with this precedence: no table or
  disabled/no history/missing snapshot, then unknown age, then age-specific
  plan or beyond-history fallback.
  The unchanged-geometry cohort compares consecutive slot renders: same output,
  display list and cursor, nonempty equal surface order, equal logical/raster
  geometry and source size, and unscaled raster dimensions. This is an opportunity
  cohort, not proof of usable client damage or canonical source provenance.
  It records full/partial counts and summed repaint/target pixel area separately.
  `tools/analyze_render_work.py` reports interval deltas, rebases on counter or
  uptime decreases, and refuses inconsistent frame counts. Overlapping rectangles
  count repeated drawing area. A counter reset masked by another context's growth
  cannot be detected from aggregate totals; intervals crossing context replacement
  are excluded from acceptance.

Qualification evidence is under `t289-implementation-01`. Targeted tests include
real intake at the shared presentation cap and a 64-group reissue after timer
settlement. Pixel-equivalence and counter-conservation tests passed separately
on renderD128 and renderD129 (30b logs, including the ignored journal test); these do not prove physical Present timing,
owner-loop latency, or CPU savings. Parts 5/6 and live acceptance remain open.

The release-build geometry-comparison fixture (33) reports about 18 ns per
comparison for four surfaces and 4 microseconds at the 1,024-surface bound
(three runs, 100,000 comparisons each). These are elapsed costs for the metadata
comparison only, not frame CPU or live acceptance. The global X evidence mutex
is retained pending contention measurement; it is not claimed to be free.

### Scrap sample invariant containment (J1/K1)

A failed scrap idle signal normally settles from the request's accepted paired
UST/MSC. If that observation is missing, the fallback uses the window's accepted
pair, then the pair frozen at that request's binding. Neither samples a host clock.
If all pairs are missing, backing and fence custody are released immediately.
The existing bounded preparation retains its truthful Idle event and terminal
Skip obligation. At the next service visit, Skip takes that visit's current Fake
pair (from its supplied monotonic time, without another clock read). Idle is
delivered before Complete, releasing buffer and swap waiters; the reservation
ends through the normal feedback lifecycle. The failed fence signal stays counted
and is not retried or reported as successful. This exceptional Fake completion
makes no physical-display or continuity claim, and does not reset a window clock.
Window/client destruction still cancels the same record without feedback, and
repeated service emits nothing further. `scrap_sample_fallbacks` counts the
invariant violation in the periodic record; debug builds assert after containing
it. Timed wire dispatch is now enabled for Session; socket admission and
cancellation checks are recorded above.

NotifyMSC terminal admission refusals also preserve the request's accepted pair,
then the window-mapped last pair; raw Fake is used only when neither exists.
A lost source's last accepted pair remains usable for this terminal result.
Neither result resets the window clock, adds Idle, nor claims a fresh hardware
observation. The high-offset regression keeps window MSC 1000002 while the raw
Fake counter is 50, then proves the next hardware binding continues from 1000002.


### Wire proof boundary

The socket tests use the real routed frontend, Unix sockets, ordered authority
egress and supplied hardware clock observations. The fake deadline test uses
the frontend's monotonic clock and wake path. They do not execute a native
owner loop, KMS, real vblank scheduling or renderer completion. Session opts
into the path in source; its existing clock-service fixtures cover binding with
no native owner and during quarantine. Physical timing, multi-client CPU cost,
first-visible latency and live acceptance remain open.

The pixel test verifies that GetImage does not change before timed execution,
and checks the retained pixels in the emitted CPU presentation update after
FreePixmap and XID reuse. A pre-existing software Present limitation surfaced:
execution updates the toplevel presentation raster, while GetImage reads the
separate core-drawing backing. This change preserves that behavior and does
not claim to repair post-Present readback or later core drawing. Source and
failed-test evidence are retained in `149-core-readback-scope.txt` under
`t289-implementation-01`; a separate repair needs readback, CopyArea and child
composition coverage.

### Part 2 intermittent-test repairs (A/B/C)

The repair evidence is under `t289-intermittents-01/repair-01`. These changes
address three distinct causes; they do not change Present clock semantics.

- **A: fixture ordering.** Diagnostic run `06-wip-x` matched an obsolete worker
  Wait at ticket 4. The raster request then took ticket 8 and correctly completed
  with transport room before the new draw waited at ticket 9. No service Wait
  or unwind was owed. Fixtures now require a new Wait from the same client,
  beyond the first draw's transaction, before routing the raster requirement.
  They derive its generation from the observed draw and assert the service Wait
  explicitly. Production raster behavior and timeout bounds are unchanged.
- **B: admission after receipt.** The controller formerly read client admission
  before waiting for batches. A client admitted during that wait could have its
  first surface transactions rejected as stale. Received batches now enter the
  bounded intake first; the controller releases the bridge, refreshes admission,
  then stages and pumps against that reading. A failed refresh retains the
  received batches for retry. The deterministic control rejects transaction 4
  on archived `b6ad18cf` and passes with the repair. Its positive assertion names
  the surface applied in that same waited call. Separate controls cover FIFO,
  exactly-once retry and a client that really disconnected.
- **C: idle runtime contention.** A conservative atomic demand bit avoids the
  timing service's runtime mutex on empty visits and deadline checks. Producers
  set it under runtime before publishing work. Service clears it under runtime
  then feedback only when preparation maps, schedules and incomplete Fake-bound
  feedback are empty. Tests cover producer/clear races, unpublished requests,
  cancellation, and Fake feedback after the preparation queue empties. Empty
  visits acquire zero runtime locks; one final cleanup visit is allowed.

Thirty matched pairs ran the Session and X library suites concurrently, with
32 test threads each, alternating revision order. The baseline had A/B applied;
the WIP had A/B/C. All 86 selected repair and fixture-family tests passed every
run in which they exist, with no held-unwind, raster-egress or empty-harvest
admission stall. Per-test durations and full raw results are retained, including
five failures of the unchanged single-turn startup assertion: baseline 2/30,
WIP 3/30 (two-sided Fisher exact p=1). This does not establish equal rates or
make those failed suites green. The assertion expects all three budgeted steps
in one service turn; a future fixture repair should check bounded cumulative
progress. A/C negative controls fail immediately when their fixes are removed.

Final repository-gate results are recorded in `repair-01/FINAL-GATE.json`.
This evidence qualifies the repairs at the tested load; it does not establish
live CPU savings, hardware timing, first-visible latency or t289 completion.
The admission-cache optimization remains separate and disabled.

For the t034 lock-lane merge, whichever branch merges second must include
`session_lock` in background visibility and Present source selection. Locked
surfaces count hidden and new requests bind Fake; unlock must provide the
hidden-to-visible release edge. The field exists only on the lock branch.

### Visible heads without sequence counters (release prerequisite)

The QEMU virtio probe exposed a correctness defect in the initial Part 2 timing
path: a visible surface on an active head with `GET_SEQUENCE = EOPNOTSUPP` was
bound to Fake. Synced Presents then waited for the 1 Hz background clock. The
old guest binary reproduced this with two non-overlapping visible CPU-pixmap
clients: 10 and 9 completions in a 10-second measured interval. This is a
correctness experiment, not a W1 or CPU-performance qualification.

An active head with monotonic timestamps and a definite unsupported sequence
query (`EOPNOTSUPP` or `ENOTTY`) now has an **Unclocked** disposition. Its identity
and first errno are cached for that native target lifetime. Modeset, owner loss
and replacement invalidate the identity; it cannot revive. A working mirror
member is preferred. Hidden surfaces, absent native owners and quarantined
outputs keep Fake. Permission errors, transient failures and `EINVAL` remain
query failures rather than cached unsupported answers.

- Pixmaps are eligible immediately, regardless of target or modulus. Publication,
  acquire fences, ordered egress and renderer/scanout custody still gate execution
  and feedback. Complete takes its UST from actual retirement; MSC stays at the
  window's accepted plateau through the existing frozen offset. A client's
  `last_msc + interval` can remain numerically ahead forever on this source;
  eligibility deliberately ignores that target. Idle remains independent.
- NotifyMSC waits one minimum mode field period from its actual binding time.
  The frontend arms a deadline only while work is queued. Settlement uses the
  service's monotonic UST and the plateau MSC; no vblank is invented. This bound
  avoids immediate-reply loops on idle heads. Executed Pixmaps create no timer.
- Source switches use the existing window offset. This preserves switch
  continuity; the earlier boundary for overlapping frozen requests on different
  sources still applies. Loss settles queued work once. An already executed
  Unclocked request retains actual retirement UST and its plateau even after
  loss, without reviving the source.
- The service reads time once, immediately after taking the runtime lock, and
  reuses it for the entire visit. Reading before the lock could precede a newer
  binding and falsely count three stale observations as source loss. The
  deterministic negative control restores that ordering and counts three stale
  observations; the corrected test counts zero, including on Fake.

`DRM_CAP_TIMESTAMP_MONOTONIC = 0` is a distinct `UnsupportedClock` result:
`current = None`, no Unclocked identity, and the existing Fake fallback. The
capability answer is cached per owned card-fd lifetime. Raw realtime event UST
must not enter this monotonic completion path. A pre-4.15 kernel that answers
`EINVAL` for the absent sequence API still falls back to Fake at 1 Hz. That is
an explicit kernel-floor residual: `EINVAL` is ambiguous with an invalid CRTC
on newer kernels, so it is not treated as definite unsupported.

The periodic evidence includes `unclocked_bound` and
`unclocked_notify_settled`; the first unsupported answer per native head lifetime
records its errno and minimum field period. The admission cache remains a
separate change: Unclocked cannot satisfy its Hardware-only proof.

Tests cover source selection, hidden Fake, a clocked mirror alternative, source
replacement, mode deadlines, no query polling, loss before and after execution,
exactly-once Complete/Idle, retained-pixmap release, and acquire-fence gating.
Two full-content Unclocked requests remain independent when the first waits on
a fence. Sophia's existing rule excludes requests already serviced for MSC
from equal-target scrapping; XLibre can still scrap a fence-waiting vblank.
This repair preserves that difference.

Evidence and source manifests are under `t289-clockless-01`. Guest and final
repository qualification results are recorded there as they complete. T289's
CPU comparison, physical timing and live acceptance remain open; this repair
does not close the task or qualify the W1 fast path.


### First Present before composed coverage (release prerequisite)

The guest comparison exposed a second timing regression: admission used the
backend's presentation order before the first frame had populated it. A mapped
visible window therefore bound Fake and waited zero to one second for its first
Pixmap. The old guest showed about 720 ms between the preceding composition and
the first warm-up frame; the pre-t289 comparison was about 55 ms. These are
individual correctness observations, not a latency distribution.

Session now supplies ordinary placement independently of pixel availability:

- A managed window uses its committed WM projection and Session's reconciled
  content layer, including the assigned output. Omitted or minimized is hidden.
- An open layout epoch overlays that placement only for admissions it owns.
  AdmitSurface can already have made such a window Viewable while a sibling
  resize or presentation-state acknowledgment holds the epoch open. Its pending
  layer supplies geometry and output. Expiry removes that choice; previously
  bound requests keep their frozen source. Ordinary managed moves keep the
  committed placement until the epoch commits.
- Direct mode uses geometry. A client-positioned popup uses its fresh X Viewable
  target for its own mapping, then follows known owner mapping and policy
  visibility. A new Viewable target with no Session role is client-positioned:
  externally managed windows must first pass Session's AdmitSurface. Unknown
  ancestry is allowed only until the authority observation arrives; a known
  hidden owner and ownership cycles are hidden.
- The compositor still applies the lock cover, replacement tier, actual preview
  union, viewport clipping, translation and explicit output routing. An off-edge
  policy column cannot select a neighbouring output merely by intersection.

There is no coverage hold: a readmitted surface with existing pixels can owe
PresentedBuffer evidence to the same epoch that would supply its coverage. Such
a hold would create a cycle. There is also no missing-pixels visibility heuristic:
a mapped window can be hidden before its first frame retires.

The component regression prepares a real Pixmap with the Fake boundary 900 ms
away, selects both a hardware and an Unclocked head, and checks immediate timing
eligibility for committed and sibling-held admission placements. Restoring the
old selection fails with Fake (`28-first-placement-mutant`, exit 101); the source
was restored byte-for-byte. Controls cover hidden/minimized windows, popup map
and remap, hidden owners, expiry, lock, output ownership and replacing tiers.
Final source, repository gate and guest checks remain in `t289-clockless-01`.


<a id="t300"></a>
### t300: fence-waiter scrapping parity (candidate)

Source review of the clockless repair identified an existing ordering difference.
Sophia marks a Pixmap serviced for MSC before its acquire fence signals. A later
full-content Pixmap with the same source and target can therefore execute first;
when the older fence signals, its older content can be shown last. Unclocked
requests reach this state immediately. This requires a wait-fence user and is
separate from the visible-head 1 Hz repair.

XLibre keeps that fence-waiting vblank eligible for scrapping. A matching newer
full update releases the older pixmap with Idle, leaves its completion obligation
queued, and reports Skip at its target. Partial updates remain independent.

Proposed scope: keep clocked and Unclocked fence waiters scrappable until actual
execution, preserving frozen source/target equality, publication order, private
fence custody and exactly-once feedback. Do not cancel an executed buffer or
scrap NotifyMSC. Cover an unsignalled older full update followed by a newer one,
a later fence trigger that executes nothing, partial updates, distinct targets
and sources, and destroy/disconnect. The current
`unclocked_full_updates_do_not_scrap_a_fence_blocked_request` control records the
existing behavior and must change with the parity fix. No implementation is
included here. Review: `t289-clockless-01/pF/FRONTEND-REVIEW-02.txt`.
