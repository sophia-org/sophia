# Device-free Session frame attribution

Implementation in progress from Sophia `134e74df931665834cfacadb9e088ddcb519f924`.
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

## Exit

Retain source/build/fixture identities, raw records, instrumentation controls,
scope nesting and independent review. Rank measured costs and repetition; list
unmeasured work explicitly. Only then identify a bounded mechanism with a
predicted counter change. S1, single-submit and empty-owner-work stay parked.
No SDK/protocol, authority, scheduling or release-policy change belongs here.
