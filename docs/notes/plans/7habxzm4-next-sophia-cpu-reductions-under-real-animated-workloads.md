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

### Current experiments and boundaries

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
