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
