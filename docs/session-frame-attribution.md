# Device-free Session frame attribution

Implemented from Sophia `134e74df931665834cfacadb9e088ddcb519f924` in signed
`aa1cd9000`, with reservation-attempt accounting corrected in `aacb8dfbe`.
This is a diagnostic, not an optimization or a native performance comparison.

## Measurement contract

Exercise the CPU owned-ring path with one output, one panel and two mapped
source slots at 1920×24 and 1920×512. Use the production connection, image
validation, component service and content owners. Authentication verdicts and
native submission/completion are supplied by existing deterministic fixtures;
do not count those supplied operations as hardware work.

Each release block runs 120 warmup and 2000 measured completions. At each size,
run instrumentation off/on/on/off. Keep the peer on a separate thread. Buffer
diagnostic records until the block ends, retain ordinary Session logging, and
record thread CPU separately from elapsed time. Worker jobs and owner completions
have separate counts/windows. Retain timer-only records without subtraction.

All measurements require matching pixels, protocol outcomes and release order.
Clock regression, arithmetic overflow, missing completion or incomplete accounting
invalidates the block. No graphics, compute or input device may be opened.

## Fixture and accounting boundaries

The conformance peer fills both registered mappings in pairs. The pinned SDK
chooses the first free slot and requires a control-write acknowledgement before
queuing another offer. During pair fill, the fixture serves only 9P owner turns,
deferring image-job admission until both offers are queued. Every pair asserts
slots 0 then 1 and waits for both Accepted/SlotReleased settlements. These extra
wire turns are counted separately as `fill_wire_turns`; they are a fixture
scheduling choice, not the native owner loop. The owner waits on wire/worker
readiness with a 1 ms control-channel cap. The peer yields while waiting.

The warmup boundary lies between pairs, after 120 Presented outcomes and the
preceding resource retirements. The peer waits while the owner starts the
measurement generation. No image job or timer scope spans that boundary.
Each block then completes 2000 frames. The first measured retirement releases
the last warmup resource; the final resource remains presented until teardown.
Pixel equality is checked on the separate peer thread using a cloned accepted
lease, dropped before that resource's retirement. The resource store and
presented frame still own it, so this oracle cannot perform its final disposal.

`service_until_submission`, `next_submission`, and `submit_bundle_to` share the
production resource/output/allocation/demand/candidate sequence, pending-output
predicate, dismissal checks, projection, admission and Prepared bookkeeping.
The supplied target callback replaces concrete native admission. Compared with
the older reconnect fixture, Prepared now uses the actual log, deduplicated
allocation list and persistent-catalog binding. Output facts are primed once in
diagnostic setup, not rebuilt by a fixture helper each frame.

Scopes are inclusive. Submission contains two Projection scopes and the target
admit path; Completion includes the Presented log; Logging includes formatting
and the installed stdout/stderr callback. FixtureTarget separately covers
`simulate_submit`/`simulate_completion`: the copied backing is made by
`Target::finish_render -> copy_submission`, not by target admission. Never sum
these stages or subtract their overlapping totals from the owner window.
OwnerService covers the shared pre-submission service visit. Visits include
idle passes and the tail while the peer exits. Report both time per visit and
time per completion; neither measures a native frame deadline.

The owner Explicit snapshot precedes teardown. Its duplicate Block snapshot and
warmup snapshots are discarded. The peer publishes on thread exit. Worker
snapshots retain the last generation observed at a job: their CPU windows run
from the first measured job to thread exit and include teardown; their elapsed
windows also include idle time. Counts must prove exactly 2000 validation/copy/
pixel jobs, source releases and submissions,
4000 projection scopes, and 2000 supplied target completions. Off blocks retain
outer clocks and disabled-entry counts but must have zero enabled stage counts.
Timer-only controls call an empty scope 2000 times in off/on/on/off order;
their observed overhead is retained without subtraction.

Reserve and Authority count attempts, including retained-offer retries after
worker-capacity or resource-budget backpressure; they must be at least the
accepted job count, not equal to it. The first release attempt exposed this
distinction at h512 (5000 reservation attempts for 2000 completed frames) and
was refused by the original equality assertion. Its evidence remains in
`~/.local/state/sophia-session-frame-attribution-01`; it is not a completed run.

The owner outer window includes fixture coordination and target simulation.
The peer window includes fill, polling, protocol handling and the pixel oracle.
No estimate here closes KMS, native rendering, authentication-verdict generation,
WM/input work, full `service_components` orchestration or paced owner-loop costs.
Normal Session Prepared/Presented lines remain inside the measured block; the
added diagnostic records are written only after the block. The diagnostic is
default-off and changes no SDK/protocol contract.

Run the ignored measurement test alone in a release binary with devices hidden,
stdout retained and no concurrent builds. The full test name is
`live_session::metadata_shell::component_session::reconnect_tests::frame_cost::frame_attribution_measure`;
use `--exact <name> --ignored --nocapture --test-threads=1`. Both diagnostic tests
also share a mutex to serialize an accidental combined ignored-test invocation.

## Existing coverage and gaps

| Evidence or production boundary | Existing coverage | Remaining obligation |
| --- | --- | --- |
| `continuous-shell-work-cost-01` | Inclusive custody, admission, settlement, release and worker stages; service/record counts | Overlapping scopes cannot be summed; old source and native windows are distinct |
| `lom-session-work-cost-audit-01` | h24/h512 validation, copy and pixel stages; owner CPU in its own window | Does not partition owner work or identify an irreducible fixed cost |
| `LiveContentSession::service_with_catalog` | Real resource, output, allocation, demand and candidate service sequence | Attribute calls and time without replacing admission decisions |
| `ShellComponentSession` and `service_components` | Existing real private-9P owner fixture and service entry | Keep synthetic peer work out of owner CPU; disclose fixture visit frequency |
| `SessionContentFixture` | Real projection, retained ownership and completion consumer with deterministic native target | No GPU, KMS, scanout pacing or native scheduling claim |
| Session logs | Ordinary prepared/resource diagnostics | Preserve their cost; added diagnostic emission belongs outside measured blocks |

The earlier instrumented owner windows are roughly 1075–1087 µs/completion at
h24 and 1050–1061 at h512. They describe those windows, not a fixed-cost lower
bound. Their worker windows must not be subtracted from process CPU or joined
to owner frames by overlap alone.

## Audited result, November 1

All eight release blocks completed: 16,000 measured frames plus 960 warmup
frames, four timer controls, and matching pixels/protocol outcomes throughout.
Root reduction passed 12 groups, 644 diagnostic records and eight mutation
refusals; the independent audit passed with zero findings. There are 33,920
ordinary Session records. Each enabled block has 2000 worker jobs, source releases, submissions
and supplied completions, and 4000 projection scopes and log calls.

These ranges cover the two enabled blocks in CPU µs/completion; scopes are
inclusive and cannot be added or subtracted:

| Scope | h24 | h512 |
| --- | ---: | ---: |
| OwnerService | 85.11–85.88 | 102.04–104.01 |
| Transport | 46.61–47.11 | 54.25–55.37 |
| Candidates | 14.84–15.16 | 16.45–16.92 |
| Submission | 14.63–14.97 | 16.59–17.24 |
| Logging | 5.96–6.08 | 7.18–7.41 |
| FixtureTarget, supplied work | 9.45–10.57 | 99.46–104.15 |

Enabled h24 blocks made 8.472–8.588 service visits/completion; h512 made 10.504.
Service CPU per visit was 9.91–10.14 µs and 9.72–9.90 µs respectively. Both
heights had roughly 2000 extra fill-phase wire turns per block. Reservation and
allocation-authority attempts were 2000 at h24 and 5000 at h512, including 3000
backpressure retries. These are repetition counts, not evidence of redundant
authority checks or empty visits.

Instrumentation is material: owner outer CPU at h24 was 162.77–167.12 µs/frame
off versus 295.49–297.28 on; h512 was 270.74–282.07 off versus 445.29–457.37 on.
The empty timer's outer loop cost 1.044–1.045 µs/iteration enabled, while its
inner scope recorded 0.519–0.521 µs. Owner blocks contained 263k–323k scopes.
Use off-block outer windows as the untimed fixture references; do not subtract
timer controls or on/off differences to invent uninstrumented stage timings.
Elapsed submission/logging outliers reached 1.4–2.4 ms with CPU maxima below
60 µs, so those delays are not CPU costs.

Evidence and the full scope table are in
`~/.local/state/sophia-session-frame-attribution-02/REPORT.md`.
The root `RECEIPT.json` hashes to
`1db655551a7023615f5a481697e047a076833d4f38c8503bd467b3b6fb619f55`;
`SHA256SUMS` hashes to
`62dd9389124b031fbfacc2e1b5eeb96561d55feb3ef51e94513df9c4d2685d1e`.
The independent audit is
`~/.local/state/sophia-session-frame-attribution-audit-claude-01`, manifest
`7a9b4e4ba3b59e1e9d424a248f66161cb7d53b1487357208fb64d68e0cc1aae5`.
Source gates passed 1063 Session and 163 runtime tests, strict Clippy for the
changed crates, formatting/layout, and feature-off/draw compilation. The final
release smoke and full run passed with devices hidden. Broader initial sandbox
refusals and the unrelated dependency Clippy warning remain documented in the
receipt rather than being reported as passing checks.

The fixture attribution step is complete; no optimization candidate is promoted.
The next evidence gaps are fresh 9P authorization/custody work still grouped
inside Transport, and full owner-loop/native scheduling work outside this
fixture. These results do not explain the whole historical native Session
fixed cost. S1, single-submit and empty-owner-work remain parked.

## Exit contract

Retain source/build/fixture identities, raw records, instrumentation controls,
scope nesting and independent review. Rank measured costs and repetition; list
unmeasured work explicitly. Only then identify a bounded mechanism with a
predicted counter change. S1, single-submit and empty-owner-work stay parked.
No SDK/protocol, authority, scheduling or release-policy change belongs here.

## Follow-up: sparse timers and fresh 9P boundaries

The follow-up worktree preserves the audited branch and its binary. Schema 2
adds `entries` to every stage, including timer-disabled entries, and records
`sample_period` and `sample_phase` on each snapshot. Each stage samples its
zero-based entry ordinal independently. `calls` and accumulated clocks describe
only the selected samples; neither is multiplied by the period. Sampling phases
are not paired across nested scopes. Inclusive rows remain independent: do not
sum them or subtract them from an outer window.
`enabled_calls` and `disabled_calls` count hook entries by timer mode; the
number of timed scopes is the sum of stage `calls`, not `enabled_calls`.

The coordinator changes sampling settings only between quiescent pairs. Each
thread latches settings when its generation changes; a worker's exit preserves
its last observed generation and settings. The compile-time feature-off path
still calls the supplied closure directly. Runtime timer-off blocks count
entries but read no per-entry clocks. Their counters therefore prove the same
completion, worker and lifetime invariants as timed blocks.
Threads register before recording work: registration resets their window.
Generation changes must not race a thread's registration or a measured call
and its outcome count. The owner rolls its own window outside all calls and
keeps the peer and worker quiescent across the sampling-settings update.

`frame_attribution_sparse_measure` runs the same workload with period 64 and
phases 0, 17, 33 and 49, in two off/on/on/off groups per height. It retains four
empty-scope controls at period 64. Compare raw owner CPU, visits and fill-wire
turns with adjacent timer-off controls before interpreting sample averages.
The initial perturbation target is less than 5% for both owner CPU and visits;
it is a gate to verify, not a result or an overhead correction. Changing phase
helps reveal alignment with repeated protocol work; it is not random sampling
and does not establish statistical representativeness.

Transport now counts request-service turns; WireFlush separately counts
publication flushes. Both still include shared-core work. The new inner scopes
are Authority9p (fresh authenticated-export checks), JournalRead, JournalAck,
SubmitCommand, OwnedRingWrite and TransactionWrite. OwnerTurn includes the
runtime connection visit; ImageChannel and DrainJournal identify channel
service and publication drain. Journal appends remain DecodeJournal. Authority
continues to mean the allocation-description lookup, not Authority9p.
All request scopes count attempts, including retries and refusals.
Journal read outcomes have separate clock-free counters for ready, pending and
refused returns. Their sum must equal JournalRead entries in every block;
pending means a read found no record yet, not an extra completed frame.
Shared-core socket I/O, framing and dispatch outside the export boundary remain grouped;
no clock subtraction estimates them. The experimental custody-dispatch path
remains outside Transport/WireFlush instrumentation.

Run one exact ignored test per process with devices hidden and stdout retained:

```sh
<held-test-binary> --ignored --exact live_session::metadata_shell::component_session::reconnect_tests::frame_cost::frame_attribution_sparse_measure --test-threads=1 --nocapture
```

This is still the paired content fixture. Full component orchestration and
production cadence remain subsequent work. No optimization candidate follows
from these source changes alone.

### Period-64 attempt and next control

Signed `45260844b` passed 167 runtime and 1063 Session tests, strict Clippy,
feature-off/draw/custody builds, and device-hidden release smoke. Its release
run passed all 16 workload blocks and four controls, including the journal
outcome join. Root reduction passed 11 mutation refusals; the independent
saved-record audit confirmed source/build identity and counts.

The perturbation gate failed: three on-block owner CPU differences exceeded
5%, reaching 9.29%; visits stayed within 5%. Off-block h24 CPU also varied
163.19–192.26 us/completion, so this does not quantify a causal timer cost.
Retain `~/.local/state/sophia-session-owner-attribution-01` as a failed
perturbation attempt, not an accepted low-overhead baseline.

Before the next run, the declared control is period 256, phases 0/65/129/193,
8000 completions per workload block and the same 120 warmup. The exact test is
`live_session::metadata_shell::component_session::reconnect_tests::frame_cost::frame_attribution_sparse_long_measure`.
This reduces clock frequency fourfold while retaining about 31 samples per
per-frame stage. Require the two off controls in each ABBA group to agree within
5% for CPU and visits, as well as the on-block comparison to their mean. Retain
all blocks and refusals; do not correct or extrapolate sampled clocks.

The current Authority9p row uses the supplied authentication owner in
`image_test_authority.rs`; it includes real file-export checks but not the
production owner's per-operation poll of wrapper and peer pidfds. The current
fixture also lacks production's `subscribe_wait`/`observe` path, causing a
fallback poll inside each Transport turn. Those gaps must close before this
fixture can support a mechanism choice for production fixed-frame work.

### Long-block lifetime bound and reusable ledger

The first longer attempt at signed `f24dcad4b` failed after Presented frame
4096, before emitting a workload block. The peer saw a refused resource status;
its old assertion omitted the reason. Source inspection shows the fixture's
new-ID-per-frame strategy reaches the unchanged 4096 lifetime identity limit.
Budget admission retries until its deadline. The saved status does not itself
prove the timeout reason. Preserve attempt 02 and report no workload timings.

The next fixture uses six resource IDs, with generation incremented on each
reuse. Upload, candidate placement, pixel oracle, retirement and released
acknowledgement all match the complete `(id, generation)` key. The predecessor
is released before any ID is reused; both source slots still fill in pairs.
Production freshness, quota and release rules are unchanged. Smoke crosses
multiple reuse cycles, with six warmup and twelve measured frames per case.
Headers record `resource_ids=6`. This changes the fixture workload: compare
the new run's own off/on controls, not its costs against the unbounded-ID run.
The 8000-frame/period-256 experiment and its predeclared gates otherwise remain
unchanged.

### Audited sparse result

Signed `9ba5b0b3b` completed all 16 longer blocks: 128,000 measured and 1920
warmup frames, with exact pixels, jobs, completions and lifetime counts. The
root reduction and independent saved-record audit pass. Timer-on owner CPU
differs by −2.35% to +1.65% from each group's off-control mean; visits differ
by less than 0.66%. The off controls within each group agree within 0.58% for
CPU and 0.79% for visits. Both predeclared 5% gates pass.

Evidence: `~/.local/state/sophia-session-owner-attribution-03`, manifest
`3daf00063038d90194ae07413a47e100cd4ac0da47537fb9131fb13e875f5657`.
The independent audit is `sophia-session-owner-attribution-audit-claude-03`,
manifest `9488b8cca98abbcc7b486563d17e627e25d5f66ed60e7c917fffddf4643bca70`.
All 52 snapshots (1872 diagnostic rows) and 259,840 ordinary Session lines
are retained, with 11 negative-control refusals. The initial reducer, which
incorrectly required one ACK per ready journal read, is retained too. ACKs
are cumulative and may cross block boundaries; both counts are now reported
independently, without a tolerance. The event-sum and other exact joins remain.

This proves timer perturbation relative to the counter-only path, not the
counter path's cost relative to a compile-time feature-off build. That reference
is the next measurement gap, followed by real protected-peer authentication,
consumed readiness observations and full component orchestration. Sampled
scope clocks remain raw and inclusive. No optimization or native fixed-cost
claim is promoted.

### Counter-only reference against the compiled stub

The next reference uses the same exact ignored test,
`live_session::metadata_shell::component_session::reconnect_tests::frame_cost::frame_attribution_counter_reference`,
in two release builds of one signed source: `native-session` alone, and
`native-session,diagnostic-frame-cost`. Both run four blocks per height, each
with 120 warmup and 8000 measured completions, the six-ID ledger and identical
paired uploads. Timers stay disabled throughout. The peer loop is shared;
its pixel oracle remains on the peer thread before resource retirement.

Two outer CPU/monotonic clock readings bracket the same workload in both
builds: after the warmup boundary and before releasing the peer, through peer
join and completion checks, ending before diagnostic snapshot or teardown.
Feature-off runs retain protocol, pixel, completion and release assertions;
they have no diagnostic snapshots or fabricated stage counts. Counter runs
also retain every existing diagnostic count invariant. Both keep ordinary
Session logging in the measured window. The comparison includes any compiler
code-layout effect of enabling the feature; it is not an isolated instruction
cost estimate.

Run the binaries in stub/counter/counter/stub order with saved stdout. For each
matching height and block, require the two stub endpoints to agree within 5%
for owner CPU and visits. Compare each counter block to their mean, requiring
an absolute difference of at most 5% in both quantities. Keep every block and
refusal. This gate bounds counter-path perturbation in this fixture; it does
not replace the protected-owner, production-readiness or orchestration work.

The reference ran at signed `112c16d7b`. All four processes completed, with
256,000 measured frames, exact saved completion sequences, unchanged binaries
and no diagnostic clock samples. Root reduction passed ten mutation refusals;
independent source/build/record audit found zero findings. The predeclared
perturbation gate **fails**: run 2, h512 block 2 is +5.08% CPU over its stub
endpoint mean. All other comparisons and every endpoint agreement check pass.
No threshold changed and no workload block was removed or repeated.

Evidence is `~/.local/state/sophia-session-counter-reference-01`; its receipt
binds the independent `sophia-session-counter-reference-audit-claude-01` audit
(manifest `929de779b766ced5cb0857e39a545ad23c3c54a2ad275a75a10a2b320fdff5b2`).
The initial reduction missed the first Prepared line because libtest prepended
its banner; the retained correction recognizes that prefix and preserves the
exact count checks. Counter-path and combined timer/counter perturbation remain
unqualified. Proceed with production coverage, and requalify instrumentation
against the compiled stub on that workload before choosing any optimization.

### Protected authentication and observed readiness

The protected fixture reuses the existing supervised Rust conformance peer:
real launch authentication, role attachment, wrapper/peer pidfd custody and
the existing owned-ring frame loop. The child negotiates revision 10 and two
source slots. Output facts and allocation now come from the production
content service; the diagnostic panel limit is explicitly the requested
height. No authentication factory or allocation verdict is supplied on this
path. The native composition target and completion remain simulated.

A bounded control socket carries the same Measure/Fill/Filled rendezvous.
The root-side oracle thread checks each lease and drops its extra reference
before acknowledging Verify, so the child cannot retire early. It is the
recorded Peer thread; CPU in the protected child is not included in that
snapshot. The child retains the shell connection and authentication relay
until the parent closes the control stream after taking the owner snapshot.
Process and custody reaping happen outside the measured window. The small C
exec bootstrap is compiled before each block's warmup, outside its clocks.

Every protected service visit subscribes component descriptors, polls with
the fixture's 1 ms cap, observes readiness and immediately invokes the actual
authenticated component poll. Role service follows; unused observations are
discarded. The paired-fill pump still services only the wire, without image
admission or readiness subscription. This covers the production subscription
mechanism, not production's complete wait set or native pacing.

Raw readiness deltas report waits and each descriptor class. They do not by
themselves prove consumption. Two additional clock-free events record the
actual non-flush branch: `wire_observed` for a current observation and
`wire_fallback` otherwise. Their sum equals Transport entries in this path;
journal outcomes retain their separate three-counter join. There are now
38 rows per snapshot (summary, 32 stages, five events). The observer's duplicate
readiness validation is absent from the compiled stub and included in its
overhead qualification. The experimental custody-dispatch path bypasses
`service_turn`, so it has neither event, Transport nor WireFlush.

The protected owner also polls its launch coordinator and authenticated
component lifecycle. Its CPU window is therefore not comparable to the
supplied-owner fixture. Full component-service orchestration remains the next
coverage step. No protected timing run or optimization claim is recorded yet;
combined counter/timer perturbation must pass against this workload's stub.

### Complete bar component pass and next reference

The protected driver now configures one bar and invokes the complete production
component-service routine. Its launch coordinator, OwnerWake, children,
application-launch state and optional WM persist for the block. There is no
WM in this fixture, so the production unavailable-indicator paths run. The
native admission callback uses the same backend admission function and keeps
the target separate from the borrowed runtime. Dock and launcher production
paths are unchanged and unconfigured here. Dismissals still receive the native
argument; the fixture supplies no native scanout.

`ComponentPass` includes the full routine. `OwnerService` now lives directly
around the shared content service through candidate preparation; the former
driver wrapper is removed. FixtureTarget still covers simulated submit and
completion after the pass returns. The following production pass observes
that completion; there is no additional fixture transport visit to observe it.
The peer oracle publishes its last verified frame after checking and dropping
its extra lease. The owner reads that count only at the warmup boundary and
after joining, proving every presentation was received and checked.

For this single-output workload, ComponentPass and OwnerService entries equal
visits. Completion entries equal twice visits plus measured frames: the content
service's observation loop and the bar branch each check once per visit, with
one extra successful loop iteration for each completion between passes.
Submission and FixtureTarget each equal measured frames. Snapshots now have
39 rows: summary, 33 stages and five events. Coordinator polling remains in the
outer owner window outside ComponentPass. The complete physical owner loop,
other native wait descriptors, protected-child CPU and native pacing remain
outside this fixture's claim.

The next experiment compares combined sparse clocks and counters with the
compiled stub, not with counter-only operation. Both use this protected full
pass, 120 warmup and 8000 measured frames at heights 24 and 512. Period 256 is
fixed. For each phase 0, 65, 129 and 193, run stub/observer/observer/stub; each
process runs one block at each height. This interleaves controls more closely
than the earlier eight-block processes. Compare matching phase/height to the
mean stub endpoints, require their CPU and visit agreement within 5%, and
require each observer difference within 5%. Keep every run and refusal.

Use the exact test
`live_session::metadata_shell::component_session::reconnect_tests::frame_cost::frame_attribution_protected_reference`
with `SOPHIA_FRAME_SAMPLE_PHASE` explicitly set to one of those four phases.
The observer build retains empty-scope controls and all raw sampled records;
the stub has neither diagnostic stage records nor fabricated counts. This is
a predeclared experiment, not a result or optimization claim.

Correctness gates for this integration passed in the device-hidden namespace:
14 runtime instrumentation tests, 1,063 Session tests (73 ignored), strict
Clippy, and the protected smoke in both observer and compiled-stub builds.
Both smoke traces contain no graphics, compute or input-device opens. Source
logs are retained in `~/.local/state/sophia-session-component-pass-source-01`.
These checks establish behavior and accounting, not performance.

### Protected full-pass result: observer qualification refused

Signed source `26d2fd1f7` completed all sixteen release processes: 32 workload
blocks, 256,000 measured frames and 3,840 warmup frames. Root reduction and the
independent audit pass identities and exact counts, including all 519,680
prepared/presented records, 80 snapshots and 3,120 diagnostic rows. Eleven
root mutation checks refuse. All normal visits consumed current readiness;
the paired-fill pump accounts for the fallback turns.

The predeclared combined-observer gate **fails**. Run 10, phase 129, h24 is
+5.187571% owner CPU versus its two compiled-stub endpoints, exceeding 5%.
Its visits differ by -0.205775%; stub CPU agreement is 3.074773% and visit
agreement 0.177616%. All eight endpoint-agreement checks and the other fifteen
comparisons pass. The refusal is retained without tolerance, exclusion or
rerun. Total observer overhead remains unqualified; no Session optimization
or native fixed-cost claim follows.

Raw outer owner CPU spans 227.73–237.52 us/completion for the h24 stub and
236.53–243.29 for its observer; h512 spans 332.36–348.89 and 336.85–351.77.
The sparse scope table is retained in REPORT.md as unqualified diagnostic
observations. Do not add, subtract or extrapolate inclusive clocks. These
protected full-pass results are not paired with older supplied-owner runs.
The target and completion remain simulated; other physical owner-loop work,
native pacing and protected-child CPU are outside the claim.

Evidence: `~/.local/state/sophia-session-component-attribution-01`, manifest
`1ec259df71223fcde5dc0ce5bf8b9fe76dd6289d31d9f23cbd55acc66c2e46ea`
(55 files). Independent sibling audit:
`~/.local/state/sophia-session-component-attribution-audit-claude-01`, manifest
`1cea49cd2ca4b7716b3b59c9612482254b97374a01eb1fff9477f243fea91c08`.
The correctness source gate manifest is
`86c92653cbd9470cd81c7f99a80def63a3fb47cde73f7fe064a23e483a373750`.
The next bounded step is observer implementation work before requalification;
production mechanisms remain parked. The measurement window is closed and
Mini's CPU-build hold released; its GPU worker remains disabled.

### Observer sampling fast path

The next observer implementation preserves the zero-based sample ordinal and
latched period/phase. Power-of-two periods use `ordinal & (period - 1)`;
other periods retain remainder. This avoids general division on unsampled
entries at period 256 without changing hooks, counters, records or feature-off
behavior. The real-measure phase-partition regression now checks every phase
for periods 1, 3, 8, 17, 64 and 256 over two complete cycles.

Requalification uses a new sealed identity and the same protected full-pass
plan and 5% limits. The prior refusal remains intact, and independent runs are
not paired to claim savings. This is observer work, not a Session candidate.

### Masked observer result and external-sampling handoff

The observer-only mask change at signed `4b204fdbb` preserves every sampled
ordinal and count, as confirmed by real-measure tests and the independent
saved-record audit. Its combined-observer reference still **fails**: h512
phase 65 run 5 is +5.718584% CPU and phase 193 run 13 is +5.774361%, against
5%. All control agreements pass (maximum 4.689102% CPU); the other fourteen
comparisons pass. No saving is inferred between these independent experiments.
The threshold and both earlier refusals remain unchanged.

All sixteen processes completed, with the same 32 workload blocks, 256,000
measured frames, 3,840 warmup frames, 519,680 prepared/presented records and
3,120 diagnostic rows. Root and independent counts/identities pass; all eleven
root mutations refuse. Evidence `~/.local/state/sophia-session-component-attribution-02`
is sealed by manifest
`ae812442059f9e5d1a5c7292d3aa62378e262d1e7951830df4ed2a918af21768`
(55 files). Its independent audit manifest is
`e6b4554769cc1551afae9c4a6d6abe7182add99c03b47758de22c5a02c3da806`.
The observer source-gate manifest is
`8fc52cba9dfc3aaf0c2372641fed8bf8d7fc6f2c6a0c9cf3c18c4c1e6fa0099c`.
No production mechanism is promoted.

External sampling is the next measurement method to qualify. This host has
`perf_event_paranoid=2` and `kptr_restrict=1`: explicit kernel-only stat and the
tested user-only recording command refuse without privilege. Default stat
silently changed its event to user-only; that is not evidence of kernel access.
The device-free feasibility probes are retained under
`~/.local/state/sophia-session-perf-feasibility-01`, manifest
`9f50e8274f90df7b45a6f0b378856859bea91bf8b6298cfcc6aeb7bd5a6ce5e4`.

The owner-attended preparation lives at
`~/.local/state/sophia-session-perf-reference-01`. Its fixed control/profile/
profile/control sequence uses the held compiled stub, explicit user+kernel
99 Hz samples and DWARF stacks. A privileged smoke precedes measurement;
recording and saved kernel symbols remain private. Both control and profile
workloads drop uid/capabilities before the device-hidden namespace. There are
no sysctl, sudoers, installed-capability or service changes. The whole launched
process tree is sampled, including setup and warmup; owner-thread and window
attribution require saved-data validation. The same 5% outer-CPU/visit gates
apply. This was the sudo handoff; the completed capture is recorded below.
No GPU or native display qualification is part of this step.

### External sampling reference: November 2

The owner-attended capture passes the predeclared 5% overhead gate. Identity,
saved protocol records, explicit user/kernel events and loss checks pass root
reduction and the corrected independent audit. The compiled stub remains
`758b883a…`, built from `4b204fdbb`; source HEAD at capture was the docs-only
successor `3fe193cb2`. All four processes completed: eight blocks, 64,000
measured frames, 960 warmup frames and 129,920 prepared/presented records.
Four root record mutations refuse.

| Height | Profile 1 CPU / visits | Profile 2 CPU / visits | Control agreement CPU / visits |
| --- | ---: | ---: | ---: |
| 24 | +0.274% / +0.124% | +0.493% / +0.128% | 0.284% / 0.058% |
| 512 | +2.929% / +0.075% | +1.527% / -0.008% | 0.004% / 0.243% |

The two profiles contain 2205 and 2192 software `cpu-clock:uk` samples at
99 Hz, with monotonic timestamps and 8192-byte DWARF user stacks. Neither
contains LOST or LOST_SAMPLES records. Workload stderr is empty. This
qualifies external recording on this workload; all earlier inline-observer
refusals remain intact.

The recording covers the whole workload tree. Exec/COMM/FORK metadata and
`exercise_peer` stacks identify owner threads 6842 and 6987, contributing
480 and 473 samples. Their profiles span both heights, setup, warmup and
teardown; exact measured-window partitioning is unavailable. Compiler
descendants belong to fixture bootstrap, not an external build.

Visible owner stacks contain `LaunchCustody::is_live` in 65/69 samples,
`ImageChannelPeer::check` in 32/29 and `do_sys_poll` in 63/53. These inclusive
occurrences overlap: do not add them or interpret them as savings. They
corroborate fresh custody and image-channel checks as investigation leads,
without justifying cached authority decisions or skipped checks.

Unresolved leaf IPs are 30/2205 (1.36%) and 29/2192 (1.32%) overall, and
5/480 (1.04%) and 12/473 (2.54%) on the owner. They are vdso except one
usbcore and one cc1 sample. Caller recovery is a separate limit: of 100/101
owner memcpy/memmove-family leaves, 98/99 have only that leaf in the stack.
Do not charge those copies to a Sophia caller or the fixture. A symbol-sorted
perf report hid unresolved vdso leaves; the independent audit's initial
zero-unknown claim is superseded by its retained correction. `perf script`
requires `ip` in the field list to emit sym/dso; strip addresses after decoding.

The original package/results are unchanged. Root reduction and report:
`~/.local/state/sophia-session-perf-analysis-01`, manifest
`5539bd6b3b2c86b59c5a63fbf417dd89a12ce5a237ec5f91984e6cc5d2fc1eb0`
(28 files), receipt
`a6bc47b60426f3a50bb8e09deb009f3ff47156f43c0dbe24d06de2a7a3ff1925`.
It hashes all 41 original results files. Preparation manifest:
`301fc8232ac3289b204de18d7bef3ba1110cafb3e05480fb120577b62ab4fe36`.
Corrected independent audit:
`~/.local/state/sophia-session-perf-reference-audit-claude-01/SHA256SUMS.2`,
`6c5d46c41df281d4a1c12d6480a49b8f10f6ef7c67410dd62403a8d1d0791e2b`.
The original audit and its manifest remain retained.

Next close missing callers and window partitioning with a separately declared
observation plan before selecting a production candidate. Native target and
completion remain simulated, other physical-owner phases remain absent, and
no native fixed-cost or savings claim follows. Private kernel maps and raw
instruction-address decodes stay local. The CPU quiet hold is released; the
Mini GPU worker remains disabled. No new build, launch or policy change was
needed for saved-data reduction.
