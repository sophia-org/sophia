---
id: ig4obtxu
date: 2026-10-05
kind: investigation
status: investigating
tags: [investigation, rendering, x11]
---
# Copies and rasterization dominate after KMS mapping retention

## Native cache result refused on composition counts (2026-10-07)

Fixture04 completed the excluded C smoke and all eight **BC CB CB BC** arms.
Native compatibility, fixed geometry, library identity, application cleanup
and declared timing guards passed. The frozen verdict is **NOT_ACCEPTED**:
candidate completed compositions were four fewer in pairs 2 and 4.

| Pair | Baseline desktop CPU | Candidate desktop CPU | Raw saving | B/C whole-Session compositions |
| --- | ---: | ---: | ---: | ---: |
| 1 | 1.53 s | 1.41 s | 7.84% | 1948 / 1948 |
| 2 | 1.63 s | 1.33 s | 18.40% | 1952 / 1948 |
| 3 | 1.62 s | 1.41 s | 12.96% | 1944 / 1949 |
| 4 | 1.53 s | 1.35 s | 11.76% | 1948 / 1944 |

Median raw paired saving was **12.36%**, with lower owner CPU in every pair.
Each arm measured 1,800 frames at 60.03 fps and captured/promoted/evicted 1,920
snapshots including warmup. No pending supersessions occurred. Candidate
maximum submit-to-flip was 16 ms in every pair, baseline 17/18/16/16 ms; depth
was two throughout. Report-only in-flight owner ticks were B 0/0/0/0 and
C 2/2/0/0. These are service visits, not elapsed latency. No causal explanation
of that tick difference is established. The raw improvement does not waive
the failed work rule; the cache remains unmerged and uninstalled.

The extra baseline work is not explained by startup/teardown spread alone.
In pair 2, two additional two-head queues follow client retirements 216 and
407 during animation; pair 4 has the same pattern after 208 and 409. DP-1
repeats the preceding mixed Present's logical checksum and DP-2 has the empty
checksum. Queue admission is not a worker-completion ledger, and the checksum
is not independent pixel proof. Also, `scene_generation=642` on a mixed
Present is its transaction ID, whereas `216` on the retained source set is
the committed surface generation. Their numeric order does not prove older
pixels. Claude's startup-only and older-generation readings were withdrawn.

Source narrows the next control to ordinary cadence repaint admission. A
non-Present authority batch is cadence-eligible even with displayed GPU
content; GPU preservation can report `composed=false`, which arms the pacer.
Once a Present retires, the ordinary repaint draws retained sources through
`OrdinaryScene`, without the retained queue's checksum suppression. This can
produce the observed `head_composition` records. In both refused pairs the
baseline has two more cadence repaints, but no per-repaint reason identifies
the exact triggering batches. It remains a hypothesis, not a diagnosis.

Next add a CPU/runtime control for a nonvisual batch over a displayed DMA-BUF
surface, with controls that preserve genuine CPU, chrome and layout damage.
Then distinguish required client work from optional recompositions inside the
CPU bookends. Do not add a tolerance from observed spread or repeat the same
series hoping its totals match. Any changed acceptance rule needs a fresh
declaration; this verdict stays unchanged.

Evidence: `t289-native-kms-property-cache-04/qualification-20261007T224213.594340Z/RESULT.json`,
`RUN.SHA256SUMS` (1,442 original files), `DISPOSITION.txt`, `QUEUE-SPREAD.json`
and `SOURCE-TRACE-01.txt/json`. The 13 traced source files are identical in B
and C. No new GPU test or measurement was used for the source follow-up.

### Subsequent hardware change and login failure

After the series, the operator moved the 1080p monitor from discrete DP-2 to
integrated HDMI-A-2. Sysfs associates discrete DP-1/renderD128 with PCI
`0000:03:00.0`, and integrated HDMI-A-2/renderD129 with `0000:16:00.0`.
Fixture04 describes the preceding two-head, one-card setup; it cannot be
reused unchanged. Render-node-only correctness tests can select the iGPU,
with the discrete nodes hidden. Concurrent full Session/KMS tests require
explicit card and seat/VT isolation, and performance comparisons still need
quiet host CPU and memory activity.

The next installed login returned to greetd on release `825d91460`, before the
cache candidate. Both heads reported ready, then startup failed 109 ms later;
TTY recovery was clean. No cause text was saved: the installed wrapper sends
uncaptured stderr to `/dev/null` in daily diagnostic mode. Evidence is copied
with hashes in `igpu-login-exit-20261007`.

The first bounded local-TTY retry stopped before takeover because the input
guard was not armed. The second armed, reached Session and saved
`Error: UnknownConnector("DP-2")`; recovery was clean. The installed profile
still names disconnected DP-2. The startup projection enumerates lit heads,
so that connector is absent rather than present with `connected=false`.
This refusal does not require a second GPU. The retry uses the installed
proof route with a watchdog and raw capture, not the ordinary supervised
login route; it explains that retry, while the original daily cause remains
unrecorded. Neither run establishes a cross-GPU renderer defect.

The operator chose the iGPU monitor for development. Niltempus `83c34a45e`
therefore names only DP-1, assigns workspaces 1 through 6 to its policy key,
and sets output `inherit-sophia #false`. Naming HDMI-A-2 as disabled would
still require it in the startup snapshot, so it stays unnamed. The desktop
build passed with the existing lockfile and unchanged Sophia/Hagia/kleis
binaries. The operator installed `niltempus-f18fc2ed5aa55e0f6132` and reported
being back in the live session. At 23:25 UTC the ordinary supervised login
was running the new release, DP-1 was enabled/On and connected HDMI-A-2 was
disabled/Off. Diagnostics were recording with zero storage errors and no
fatal record in the observed snapshot. This accepts startup on this profile,
not a complete-session, lock or hotplug gate. This excludes the iGPU output
at profile reconciliation, not at device discovery: card/seat isolation
remains separate work. The read-only receipt is
`profile-dp1-release/LIVE-LOGIN.txt/json`.

Evidence: `ATTEMPT-01-DISPOSITION.txt`, `ATTEMPT-02-DISPOSITION.txt` and
`profile-dp1-release/` under `igpu-login-exit-20261007`. The daily record's
missing refusal reason is tracked as [t309](../plans/queue-05-3-make-failures-diagnosable.md#t309).

### t310

The approved [runtime policy and GPU admission plan](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md)
extends the accepted startup fallback below. niltempus chose safe fallback for
unsupported settings; strict profiles retain their refusal contract.

#### Two-output workspace check stopped on stale click focus (2026-10-09)

Temporary release `niltempus-642f4b984163c5317d49` (Sophia `96cb6d6d8`,
niltempus `7fec163d0`) reached a two-output desktop. niltempus then reported
"my session just crashed" before the requested cable-removal step. Session
`44402f87-20d7-4e3d-a079-c76fcb33324b` ended with exit 1. Its `failure-cause`
is `pointer focus target is missing from the live layout`. The last committed
policy transaction, 24, made the active output empty; the runtime fatal follows
at the same boot millisecond, 46843012. Cleanup drained native presentation.
The capture stopped with no discarded/suppressed records or storage errors.

`LiveWmSession::enqueue_focus` treats a missing current layer as an error, and
the physical ClickFocus dispatcher propagates it out of the owner loop. Input
hit-testing uses presented layers, while the policy layout may already have
committed a workspace change. Queued clicks can likewise outlive their target's
placement. Either yields ordinary stale input at this admission boundary. The
durable record does not identify the rejected surface or distinguish those two
interleavings; the precise physical gesture is not reconstructed.

The repair drops only this missing-layer request, with a bounded-field
`request_rejected reason=stale_target` diagnostic. It cannot reactivate the
hidden window, retarget a reused surface index, or change the active output.
Current targets still queue normally. Hidden-tab activation uses its separate
validated path and is unchanged. The input router already drops a held button
sequence when the target leaves presented layers, or on its existing timeout;
the repair adds no replay or input-delivery bypass.

Two admission regressions fail on the parent with that exact error: a click
queued across an empty-layout commit, and an old generation whose index has
been reused. Both pass with the repair, including current-target and duplicate
controls. A separate router control verifies that a held click is discarded
when the client becomes hidden and cannot replay on its return. All 45 focused
focus tests pass. These call production admission, layout commit, queue and
routing seams; they do not drive a live owner-loop input poll or reproduce the
operator's exact timing.

Frozen failed-session manifest: `t310-two-output-attended-01/failed-session/`,
`7611b40676308905f923410ad04436db3cce240d860e1e587904d1ee6e503b5d`.
CPU records are under `t310-stale-click-focus-01`. The failed attended run is
not workspace or hotplug acceptance. Rollback to accepted one-output release
`niltempus-01fa4c74950b5a998db9` was provided, not observed. A gated successor
and a fresh attended workspace check remain necessary.

Signed repair `a41140cfdc52ffd874f9bb813e943ee22bae181e` subsequently passed
the full isolated `cargo xtask check`: 538 Rust summaries, 7,520 passed, zero
failed and 101 ignored, plus the gate's tooling checks. Source head and clean
state held throughout. The frozen CPU manifest is
`5c4ce90be5ba7bab0df383cf12a1b4d08657039d707372a9bba5b4854f9aa287`,
with CLOSURE `cdcda8668e851fea6a63d6becc79a8443730f8d774dcbcc6e490aefea1cc96da`.

The matched two-output desktop `niltempus-8d6e5625f4bb8dec290e` built and passed
profile preflight and package checksum checks. Only Sophia differs among the
ten packaged executables; the profile and peer pins are unchanged. Its source
pin also includes the previously gated passive handoff/lock diagnostics from
master. Frozen release records are `t310-stale-click-focus-release-01`, manifest
`bf4e8d1de6938c71a4e9ad2e5083b8336a31dbcc28069e08ea1b5a2db1aadbc8`.
Nothing was installed. The fix was promoted to Sophia master; the superseded
source/profile worktrees were removed after checking for ignored artifacts.
The new niltempus qualification worktree remains for the attended check.

#### Two-output first-frame admission blocks workspace input (2026-10-09)

niltempus installed `niltempus-8d6e5625f4bb8dec290e` (Sophia `a41140cfd`)
and reported that Super+4 did not switch focus and the keyboard remained on
the original window. Session `04101117-775b-443c-905d-c8a9d22426d4` stayed
alive. Action 14 committed transaction 10 and cleared focus for empty output
2. A new terminal on that output then supplied a 2542×1398 first frame,
larger than HDMI's 1920×1080 allocation. Its admission settled, but subsequent
layout transactions waited for a smaller frame and timed out every four
seconds. Super+1 later timed out in transaction 60. No cable was removed.

The retained records contain one first-frame submission for the new surface,
its selected safe extent, and repeated preserved-layout timeouts. They do not
prove every internal Present ownership transition: daily capture is bounded.
Source explains a circular wait that the CPU regression reproduces. Ordinary
reconciliation drops a recovery extent that exceeds the output bounds, so it
requests the smaller size before presenting the retained first frame. A client
waiting for that Present's completion cannot supply the requested successor.

With niltempus's explicit approval, only the newly launched terminal was closed.
PID, executable and start ticks were checked against a pidfd before each signal.
SIGTERM did not end it; SIGKILL did. Transaction 122 immediately committed the
remaining window. Niltempus pressed Super+1, transaction 123 committed and
focus was applied, then confirmed keyboard recovery. This recovered the live
session; it did not qualify the failed two-output workspace check.

Frozen evidence under `t310-two-output-attended-02`:

- `workspace-focus-stall-01`, manifest `9ae5c676ecca54abd903032fdf5612774f3ea11ac7edf200fc421fcee885c63e`;
- `workspace-focus-stall-02`, manifest `f688b7239653b058a3f6ac5dd4cdebf3b4e35629d660c467afce32a3853ab85d`;
- `terminal-close-01`, manifest `3967ea6a168da02068dfced8875c73bfc8231ca8cfcf61b2ec02090ed924e847`.

The narrow repair lets an ordinary first-frame admission retain its measured
extent when the exact candidate is still held or awaiting its own retirement.
It keeps the assigned output; that output clips the temporary placement.
Engine requires the surface, transaction, buffer, selected extent and pending
admission to agree. Ordinary managed recovery and fullscreen retain their
existing bounds rules. An intervening policy answer cannot demand a smaller
frame before the selected first frame retires. Actual retirement releases the
temporary extent and drives the standing resize through the normal exact-size
layout transaction; no completion or pixels are invented.

Device-free tests drive map, policy admission, oversized Present, reconciliation,
layout commit, an intervening reflow, exact retirement and the smaller successor.
They preserve output ownership and defer the new focus intent until retirement.
Controls cover missing retained pixels, foreign candidate identities, managed
recovery, sibling surfaces and coordinate overflow. Restoring the old Session
reconciliation path must fail the first-frame progress assertion. Records are
`t310-oversized-admission-01`; early zero-test and incomplete-fixture runs are
preserved separately and are not regression evidence. These checks exercise
layout and admission code, not a GPU or physical clipping. The matched release
still needs ordinary two-output workspace acceptance before any cable test.

#### Admitted discovery and startup boundary (2026-10-09)

Signed `5fabe9f61` introduces stable GPU admission and a revalidated fresh
render-node opener. The next boundary candidate adds the pure adaptive resolver,
GPU-qualified connector selectors, identity-changing reload refusals and passive
backend discovery. Session resolves that inventory before constructing selected
heads. Disabled connectors cannot consume their CRTCs or planes. Native requests
retain the exact advertised full mode timing. Startup waits without launching
policy or applications when adaptive resolution has no usable output, while
servicing seat release and host-admin logout. The same Session resolver is the
handoff to t306's retained-image continuity hook.

The isolated backend/config/Session suite passed: 124 test-result summaries,
2,499 reported passes, zero failures and 51 ignored (summaries include nested
child-process tests). After the startup activation code was split by ownership
and GPU-qualified lookup corrected, Session lib tests passed with 896 passed and
26 ignored; native output topology passed 12/12, including two GPUs sharing both
a connector name and connector number. Three-crate all-target/all-feature clippy
passes with warnings denied. Source layout passes with no new debt, including the
test-module layout correction Claude found in the opener commit.

Evidence is `~/.local/state/sophia/development-evidence/t310-runtime-20261009/`:
`config-01`, `admission-01`, `admission-tests-01`, `focused-02`, `discovery-01`
and `replacement-01`. Failed intermediate checks remain recorded. The exact
boundary commit and source hashes are recorded in `replacement-01`.

This qualifies the boundary for integration, not a live release. Runtime hotplug,
seat reacquisition and recovery still need the continuity hook; committed
geometry/key publication, bounded conservative hardware retry, durable output
policy diagnostics, the combined gate and attended acceptance remain open.
The installed release and master are unchanged.

#### Runtime publication integration (2026-10-09)

The runtime branch now includes Claude's signed `937696c19` resolver hook,
`65b4e5d7e` resume-at-viewports implementation and `6da0e59a6` seat/startup
recovery routing. Hotplug, seat return and native recovery construct replacements
through admitted discovery and the shared profile resolver. Waiting preserves
retained images and lock custody; retries use 250, 1000 and 4000 ms, then wait
for another notice. Resolved viewports are installed before retained-image
demand, restoration and the first replacement presentation.

The publication slice binds speculative realization to transition, monitor
notice, native owner and desired-profile identity. WM geometry and realized keys
enter one scene; reusing an output number for another connector advances its
generation and invalidates older proposals. A surviving connector keeps live
focus. The published hardware capability view remains separate until the
replacement presents. A profile change during that wait publishes the current
owner's physical facts, then schedules fresh reconciliation rather than
mislabelling the old realization with the new profile generation.

A static replacement requests a repaint even without a WM layout change.
Releasing input after the presentation deadline is not presentation evidence:
a later flip can still settle the parked snapshot once, while a new notice
invalidates that observation. Startup realization is checked once against the
presented settings, avoiding repeated capability probes on idle owner passes.

Device-free evidence is under `t310-runtime-20261009/publication-01`; the
directory records the candidate identity and source hashes. It includes ledger,
WM affinity/geometry, stale-proposal, same-connector focus, late-presentation and
normalized-layout controls, plus native topology, startup, lock coverage,
warnings-denied clippy and source layout checks. Intermediate fixture failures
are preserved: one lacked opaque head IDs and another omitted the production
head-mapping projection. Neither is recorded as a product failure.

This is integration progress, not release qualification. Reload resolution over
the complete admitted inventory, bounded conservative hardware recovery,
durable policy diagnostics, the combined gate and physical acceptance remain.
Master and the installed release are unchanged.

#### Reload reconciliation and durable evidence (2026-10-09)

Reload now resolves against the complete admitted inventory, including dark
connected heads, without allocating CRTCs or planes during preflight. A result
with no usable output declines the reload while retaining the live owner.
Changes to the enabled connector set, mirror graph or head mapping enter the
shared continuity rebuild; settings on the same groups use the existing
test/apply/rollback transaction. A reload during hotplug quarantine folds into
that rebuild instead of preparing against a suspended owner.

The realization ledger tracks each reload transaction separately from its
hardware publication. It commits only after settlement and presentation with
the same desired-profile identity; a newer reload remains pending if an older
candidate is cancelled. Runtime policy transitions increment the transition
identity before staging, so transition zero uniquely identifies startup.

Claude's bounded capture slice `d457d55a5` and this slice's producers persist
resolution and adjustment records without connector names or free-form errors.
Device-free evidence is under `t310-runtime-20261009/reload-01`: Session library
915 passed and 26 ignored, native topology 12, replacement seam 2, diagnostics
5, plus warnings-denied clippy, formatting and layout checks. Claude independently
reviewed the reload boundary and the phase invariant. Bounded conservative
hardware recovery, the combined gate and attended acceptance remain open.

#### Bounded hardware recovery integration (2026-10-09)

The next slice carries one adaptive recovery allowance across TEST_ONLY,
construction, the first real startup render and runtime replacement resume.
The conservative attempt keeps one admitted logical group with its complete
mirrors and affinity, uses an advertised timing nearest 60 Hz, unit scale,
normal transform and VRR off, and leaves saved preferences untouched. A second
refusal waits for another topology, seat or profile event; availability timers
cannot reset the allowance. Durable `status=waiting reason=hardware attempt=2`
distinguishes the exhausted adaptive attempt from ordinary missing-output wait.

Claude's signed `de806765e` returns a failed replacement to suspension while
preserving the caller's original retained-image handoff and lock cover. Session
retires that exact owner before another attempt. The published topology is
adopted only after resume succeeds; failed preparation restores the scene's
published descriptors. An undispatched startup output transaction is settled
as stale before its native owner disappears, so old head identities cannot
block the replacement. Dispatched transactions keep their rollback contract.
Waiting without a native owner no longer requests a permanent 1 ms service
loop; ordinary control and monitor maintenance continues.

Focused evidence is in `t310-runtime-20261009/recovery-01`, including advertised
mode and key controls, whole mirror groups, bounded attempts, partial resume
abandonment, lock proof on the next owner, startup ordering and cancellation.
The combined gate and physical acceptance still follow. A failed ownership
disposition or worker retirement beyond the existing two-second bound remains
a terminal custody failure; this slice does not claim recovery from a wedged
GPU or relax retirement ownership checks.

The first full gate on signed `3a36ea6ac` stopped at the new startup-ordering
source guard: it assumed `physical_input_loop.rs` was included directly by
`owner_loop.rs`. The actual chain starts at `session_control.rs` and passes
through the policy, lock and physical-input phase fragments. The corrected
guard checks the whole chain, preserves the recovery-before-dispatch check,
and passes all three seam tests. `gate-01` remains failed and frozen; its
source stayed unchanged. A fresh full gate follows the test correction.

#### Combined deterministic gate passed (2026-10-09)

Signed `83c68c7c47d344aa07b53535bd59b5c4c7928d1d` passes the full isolated
`cargo xtask check`: 534 Rust test summaries report 7,438 passes, no failures
and 101 ignored. Formatting, warnings-denied clippy, layout, both SDK checks,
verifier controls and six direct-scanout archives also pass. The run used eight
build jobs, a private target, hidden devices and no network, from 17:20:51Z to
17:25:10Z. The source remained clean and its pins verified at the end.

Evidence is `t310-runtime-20261009/gate-02`, manifest
`88bab0358e8a3ec1e364cc40155824005d034d6ad750ea076fb25614a2035d3e`.
The focused development logs, including their intermediate failures, are frozen
in `recovery-01`, manifest
`21d23268dcd85251376ee526a72a0d5b7d95fe3ef38dc3f68c896365d74550f9`.
The failed first gate is retained separately. Native pixel and retained-image
proof, the matched desktop release and attended acceptance remain outstanding;
t310 stays open and the installed desktop has not changed.

#### Completion authority across output-policy installation (2026-10-09)

The patched-Mesa diagnostic `t306-01/213-qemu-patched-repaint-diagnostic-series`
stopped during mirrored startup at frozen source `0f84dcb0c`, before any client
Present. Both heads had retired frame 2 by out-fence. Installing the startup
output policy then reset their completion mode and callback serial, while the
card's event routes remained alive. The next mirror callbacks had no submitted
logical generation, ending the session. Unread kernel events for frame 2 fit
that ordering, but their identity is an inference: the error record did not
carry callback provenance. This is a Sophia accounting defect, not Mesa evidence.

The repair retains each physical head's completion source and last callback
serial across policy apply and rollback. Once out-fence completion is
authoritative, subsequent kernel events remain late events; a successor retires
through its own fence. It does not guess which submission a kernel event names.
The event and fence serials have different bases. In-place plans keep connector,
CRTC and plane, and every event-bearing commit on that CRTC requests its
available out-fence. A newly constructed owner still creates fresh heads with
page-flip completion preferred and no callback serial.

Device-free evidence is
`~/.local/state/sophia/development-evidence/t310-runtime-20261009/mirror-completion-01/`.
The extracted old reset compiles and fails three named assertions: late event
before successor submission, late event after submission, and preserved serial
basis. With the repair, all 364 backend library tests pass. The tests drive the
production head reset, callback filter, fence synthesis and runtime intake;
they do not execute a card pump or a full topology installation. Claude's
independent source review found no blocker. The full gate and a new diagnostic
image remain required. The stopped 211 and 213 series retain their dispositions;
no guest or physical acceptance follows from these tests.

Signed repair `e2b4201eb087863083a744be60a955a0e950bc42` was merged with the
qualified recovery controls as `cb5234a65`. Its first full gate, `gate-03`,
passed the Rust tests but stopped at Clippy: the shared test fixture was loaded
as two modules. Signed `9384f001383233064729577ecff02a993a936158` registers that
fixture once without changing production behavior. The full isolated `gate-04`
then passed on that exact clean head, 19:49:44–19:54:21 UTC: 535 Rust summaries,
7,447 passed, zero failed, 101 ignored, plus Clippy, layout, tooling and archive
checks. Its frozen manifest is
`5f7079ffc31e6a8f04a6fa0a304e703f78158b8416dd0a7779b2fc3529225c92`;
the focused red/green manifest is
`c67249e6cd6c88cb2619bf42a6263d8f3f83aa801c4f77555ea131c88296f609`.

The next diagnostic uses detached source `9384f0013` and a newly built image
with the declared patched Mesa, keeping 213's workload, readiness, verifier and
acceptance rules unchanged. The broader t306 fixture work remains separate.
This gate does not qualify the guest environment, merge to master, install a
desktop, or close t306/t310; those remaining steps retain their existing exits.

#### Cancelled and deferred renderer content (2026-10-09)

Diagnostic 215 reached startup and its hold workload on `9384f0013`, then failed
during clean client exit: `renderer worker started while another content identity
was rendering`. It remains INCOMPLETE, with only boot 1 run and no qualified
verdict. Its launch closure is
`2216b9deca60cedf7d124e4f5d7ce8fbfa381e340c22b545bfa89d41350cdb8e`.
The earlier missing-logical-generation error did not occur in this run.

Two source paths can retain that obsolete content slot. When a newer mirror
generation cancels an already-prepared worker buffer through the composition
installer, cancellation releases or transfers its resources but previously
left `rendering_content` and its damage snapshot behind. The mirror tick's
own cancellation branches cleared those fields; the shared path did not.
Separately, a worker returning Deferred becomes idle and requeues its frame
(or keeps a newer queued frame), while the head previously kept the returned
frame marked as rendering. Either path makes the next real worker start fail.
The retained INFO log cannot distinguish them: it records neither the sibling's
preparation nor worker slot deferrals. Both are independently reproducible
accounting defects, not a demonstrated Mesa mechanism.

Cancellation now settles the content and damage at the successful shared custody
boundary; capacity refusal retains both. Consuming a prepared submission also
clears its preparation flags on success or failure. Deferred completion in both
singleton and mirror ticks validates the exporter's queued native identity,
retains the latest queued content, and returns its damage to pending state.
When a successor supersedes a deferred mirror frame, the old cohort records
that head as skipped, allowing its generation to release after siblings settle.
The competing-content invariant remains unchanged.

CPU evidence lives in `t310-runtime-20261009/prepared-cancellation-01` and
`renderer-deferral-01`. The old cancellation settlement compiles and fails three
named tests; the omitted deferral settlement compiles and fails three, including
both unchanged-frame and superseded-frame retries with the exact fatal message.
The first deferral compile attempt is retained as invalid evidence (method
visibility), separately from that valid red run. With both repairs the backend
library has 374 passing tests, with another 317 integration tests passing and
one ignored. A further named red proves the superseded cohort would otherwise
remain unreleasable. Tests drive the real head settlement and renderer
start reducer with supplied resource/poll outcomes; source guards check the
installer/shared cancellation and both tick call sites. They do not execute a
card, GPU worker or complete owner loop.

niltempus's revised exit is the combined CPU gate, a matched release with rollback,
and an attended bare-metal KVM check with driver/Mesa identity and session logs.
QEMU successors and patched-Mesa qualification no longer gate t306. t307 remains
separate, and physical failure is diagnosed from its captured evidence. Neither
this change nor the stopped guest series closes t306 or t310.

#### First bare-metal return fails; retry and evidence ordering (2026-10-09)

The full combined gate passed on `3ef3d5b77` (7,472 tests, zero failures,
101 ignored). niltempus installed `niltempus-6f3b5ab0755b9d84b418`, Sophia SHA-256
`1a9cb3ddbbb2f03e00e7382f143faa29dd1def95db0fa5b16f4da9becfd47c90`.
The physical DP-2 unplug/replug left a black desktop. Session
`00000001791579500572-96e48315-4379-453b-be1e-d52b8eb284ef` remained alive until
a subsequent VT away/back attempt ended it with exit 101. The two preserved
snapshots are under `~/.local/state/sophia/session-investigations/`, with that
session prefix and suffixes `c29d7c2d-7aee-4695-90ce-fcfd13de5c79` (black screen)
and `1b20b40c-61eb-4745-90b0-fc4ca623a83e` (after the panic).

Durable events show owner 1 drained on disconnect, four unavailable observations,
then owner 2 adopted on reconnect notice 3. Within about 120 ms it was retired
for notice 6. Two refusals spent the hardware allowance within milliseconds;
the process then waited with no native output. The exact refusal was logged
only to the discarded console stream and cannot be recovered from this capture.
The notification burst is observed; unchanged physical topology during that
burst is an inference, not established by these reduced records.

The retry repair quarantines input immediately but coalesces notices in a
250 ms window that later notices cannot extend. Runtime refusals use the bounded
250/1,000/4,000 ms series; adaptive retries retain conservative settings after
the first failure, strict retries preserve desired settings. A constructed owner
whose resume fails cannot reset that counter. Startup's allowance is unchanged.
Durable refusal records now preserve a fixed stage, approved failure code,
numeric OS error and bounded TEST outcome. No connector or arbitrary error
text is admitted. CPU controls and retained legacy-scheduling red are in
`t310-runtime-20261009/physical-return-01`; they drive the scheduling helpers
with source guards for their owner-loop use, not a real DRM owner.

The VT panic is concrete: `native_session_evidence.rs:483` asserts that a closed
owner was opened. The failed-resume branch closed an adopted replacement before
the success-only evidence open. Claude's `38f99daf3` moves the open and owner/head
join immediately after retirement admission, retaining the duplicate-close
assertion. Adoption is not proof of presentation. Physical acceptance failed;
neither this repair nor the earlier CPU gate closes t306/t310.

#### Second bare-metal return: sleeping GPU and incomplete validation (2026-10-09)

Gate 218 passed on signed merge `28b76f7ef` with 7,480 tests, zero failures and
101 ignored. niltempus installed release 219, `niltempus-281431925697eb28f7c1`,
with executable SHA-256
`fe0be70bf1fa27c56c89425e3dd5775a3c9f5c9dbb2b1f216f308db5c5617b8c`.
The profile and Mesa closure matched the preceding release apart from release
paths. Session `00000001791580847533-da0dcfb4-42ec-40d3-b7cf-ed559217b399`
was marked before the same-port DP-2 test (`671d7045-57d2-498b-8d2a-c3ec7368e828`).

The user confirmed that the cable was reconnected and the monitor powered on.
Sophia remained alive, but cached sysfs status stayed disconnected with no modes
or EDID; the GPU was runtime-suspended in D3hot. The four unavailable probes ended
at boot millisecond 34303585. No subsequent return notice reached the recorder.
An attended VT away/back reached the seat controller and woke the GPU. DP-2 then
reported connected. A read-only udev monitor captured HOTPLUG sequence numbers
14664 and 14665, each as a kernel/processed pair, at approximately boot milliseconds
34540132 and 34540224. Owner 2 resolved at 34540244 and reached ready at 34540304,
then was closed at 34540671 for transition 3, notice 6. Four refusals followed at
the declared delays, each `stage=validation validation=rejected errno=0`.
There was no panic, and the session stayed alive with a black desktop. Recorder
health showed no discarded records or storage errors.

Snapshots are under `~/.local/state/sophia/session-investigations/`, with that
session prefix and suffix `c5e39d9c-2e41-4bfc-bc0e-7124c5fb2a6a` after the VT
attempt. Sysfs and udev observations are in
`t310-runtime-20261009/physical-return-02`. The VT action, rather than an agent
device open or write, preceded the wake.

The source explains two defects. Linux v6.18's
[amdgpu ioctl wrapper](https://github.com/torvalds/linux/blob/v6.18/drivers/gpu/drm/amd/amdgpu/amdgpu_drv.c)
resumes the device for an ioctl and releases that runtime-PM reference afterward;
holding a descriptor is not an active reference. An event-only waiting path
therefore needs a slow admitted probe when a sleeping device cannot signal a
return. Separately, the
[amdgpu CRTC check](https://github.com/torvalds/linux/blob/v6.18/drivers/gpu/drm/amd/display/amdgpu_dm/amdgpu_dm_crtc.c)
refuses an enabled CRTC without its primary plane. Sophia's preflight supplied
connector/CRTC properties only, leaving the previous plane state implicit.
Retiring owned framebuffers can invalidate that implicit state. The exact errno
of this incident remains unknown: the submitter discarded it before the durable
refusal producer received a synthetic error.

The waiting repair retains one probe every five seconds after the short series,
only through the existing active-seat, admitted-device discovery path. A return
starts a fresh activation allowance once; failed resumes cannot reset it on each
new owner. CPU controls and two failing-without-fix variants are retained in
`t310-runtime-20261009/zero-output-wait-01`. They exercise scheduling helpers and
source guards, not a live GPU. Complete-plane validation is the coordinated
companion repair, signed as `2831c9e3b`, with exact-card resource cleanup and
preserved EINVAL/EAGAIN/EBUSY identities. Its retained evidence is
`t306-01/220-complete-plane-validation`: backend 5/5, Session 6/6, with two
plane-less and five blob-only-cleanup negative controls failing as declared.
The root integration wires that errno into runtime refusal, waits for strict
profiles' missing connectors without changing their settings, and separates
Waiting cadence from failures of unknown availability. Repeated Waiting logs
stop after entry to the slow cadence. The five-second interval deliberately
favors return latency over runtime-PM residency.

Focused integration evidence is `t310-runtime-20261009/continuity-01`; the tests
exercise policy, retry helpers and errno transfer, with structural owner-loop
guards. They do not execute a GPU hotplug or prove physical recovery. Duplicate
notices can still request another rebuild; this slice does not claim
fingerprint-based suppression. t306/t310 remain open.

Combined candidate `ada93fd4b` passed gate 221-02 (7,498/0/101) and is packaged
as release 222, `niltempus-0f783c805f8040c77adc`. Its executable SHA-256 is
`f911cd6646ba09df289e81819e45bb2d227a11a419d0e7eb2798e73a65681f58`.
The runtime tree equals `415bc88f0`; the successor only restores an SDK-owned
documentation copy after gate 221 stopped on drift. Release evidence is
`t306-01/222-bare-metal-release`, manifest
`693993554239cf3d54c8c71cd7dd08e252ff7df382097a79ee3e43ae098a15b3`.
The [plan](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md#integration-and-proof)
records install/rollback identities and the deferred zero-output VT handoff
issue. At artifact preparation, no physical pass or installation was claimed.

#### Release 222 attended cable return and port move (2026-10-09)

niltempus installed release 222 and relogged. The live executable at PID 7778
has the release hash `f911cd6646ba09df289e81819e45bb2d227a11a419d0e7eb2798e73a65681f58`.
Both checks kept Session
`00000001791584192075-efb96437-173b-4184-9372-57e199d4edcf` alive. The operator
was asked to wait fifteen seconds unplugged and ten seconds after connecting,
without switching VTs; these requested durations are not measured cable times.

For same-port return, niltempus reported that the desktop survived. Owner 1
settled at boot millisecond 37781016; unavailable observations ran at 37781026,
37781282, 37782291 and 37786311, then the slow cadence was silent. Owner 2
resolved at 37799512 with Unavailable/Fallback adjustments, was ready at
37799574, and its shell content presented at 37799774. The saved snapshot
contains 515 subsequent application Present retirements. Its suffix is
`f182f2ff-d23b-48eb-87a5-c715e61038da`; its manifest SHA-256 is
`6415937882e4c498c940d2949d0f6267c5293168feb084d57ddf4029d731b759`.

The same-port return also recorded `uncommitted reason=stale transition=2
notice=4 owner=2`. Source review explains the equal-epoch case: Waiting keeps
the previous topology, the same realization returns as TransportReplaced, and
the queued publication is marked not already published. The publication check
rejects an epoch equal to the current one, leaving the realization pending
without a commit or WM authority republication. This did not prevent display
recovery. Read-only follow-up found that a fresh owner allocates head/output
numbers from one in selection order, so the same connectors and order reuse
the same numeric identities and configured mappings. The unchanged topology
epoch therefore does not misbind this same-port case. TransportReplaced does
not compare advertised capabilities, however: another monitor on the same port
with the same realized mode could leave the WM's old mode or VRR capabilities.
The proposed follow-up compares authority snapshots excluding epoch: equal
contents settle the ledger without republication; changed contents advance the
epoch and publish. It needs regressions for both cases. No such repair is in
release 222, and the cable result does not close this publication obligation.

niltempus then moved the cable to another port on the same GPU and reported
that the desktop returned. Owner 2 settled at 37891968; Waiting again reached
attempt four. Owner 3 resolved at 37912773, was ready at 37912989, and committed
at 37913006 with zero adjustments. Its shell bindings report 120000 mHz and
subsequent presentation. Read-only cached sysfs status after return shows DP-1
connected/enabled and DP-2 disconnected/disabled. The snapshot suffix is
`904b8ea3-7342-4979-8090-e88b1983ae21`; its manifest SHA-256 is
`71d4558ca62169d00addec4c0c98da3713a0dcf032ce55522bb92091bbf685f7`.

Both snapshots are under `~/.local/state/sophia/session-investigations/` with
the full session ID followed by the suffix. Their checksums verify; recorder
health reports zero discarded records and storage errors. There is no retained
seat transition, runtime fatal or refused resolution in either test interval.
The snapshots preserve a running session, not a clean termination. Graphics
maps still name Mesa 26.2.3 and libdrm 2.4.134. The bounded extraction, snapshot
identities, markers, process hash and read-only graphics observations are frozen
under `t310-runtime-20261009/physical-return-03`, manifest
`32683bb2d2857ac0b89710d12ebce238c73eca1007f02b4d5245b042fabcf852`.
These observations prove neither the GPU's runtime-PM sequence nor independent
pixel checksums, KVM input return, pointer routing, lock/unlock, or workspace
restoration. t306/t310 remain open for their remaining physical checks and the
publication follow-up; the masters remain unchanged.

#### Release 222 locked cable return (2026-10-09)

After the first two cable tests, niltempus confirmed that the pointer and usual
keyboard shortcuts seemed normal. The next instruction was to lock, unplug
for fifteen seconds, reconnect to the same port, wait ten seconds, verify that
the lock screen remained, then unlock without a VT switch. niltempus reported
that it survived. This is an attended survival report, not a measured cable
duration or an independent pixel/lock-coverage proof.

The same Session and PID 7778 remained on release 222. Lock epoch 1 appears at
boot millisecond 38694031 before owner 3 settles at 38701323. Waiting reaches
attempt four; owner 4 resolves at 38721972 with zero adjustments and is ready
at 38722197. The known same-topology `uncommitted reason=stale` record follows
at 38722222. Lock epoch 1 appears again at 38728070 and 38731376–38731388,
including input epoch 7, followed by application Present retirements and shell
presentation. No retained fatal, refused resolution or seat transition occurs
in this interval. Capture strips the lock status and coverage fields, so these
records alone do not establish the covered topology or successful unlock.

Preserved snapshot: the same full Session ID with suffix
`bef7027b-841f-41dc-a49e-5cc8cab3850e`, manifest SHA-256
`d99270dcbe8a6676f9814a607d8cab5751edd8988b60c35ead3193707475d8d5`.
The manifest verifies and recorder health reports zero discarded records and
storage errors. Markers `9553066b-80d9-456e-b9ce-62ae4d84517f`,
`9980a20d-239c-4bca-a543-977bc2e07bd4` and
`c9ea6e32-2ecb-457d-b8e8-ba67f15524b1` delimit the input confirmation, test
instruction and survival report. No actual KVM/USB acceptance report has been
received; cable return and normal input without device removal do not supply
that separate evidence. No task closure, new build or master merge occurred.

Read-only follow-up identified two lock evidence gaps on release 222. The
generic capture filter drops lock status and coverage counts; a future specific
reducer should admit only closed status/source/verdict sets and bounded numeric
fields, retaining no free-form errors. Separately, session_lock_coverage dedups
on lock epoch and topology epoch without native owner identity. A same-topology
replacement therefore emits no new cover record even with complete capture.
Recording and keying coverage by owner as well needs its own regression; no
coverage failure or exposure of unlocked contents is established by this
missing record. The attended result and the absent machine proof stay distinct.

#### Release 222 unlocked KVM return (2026-10-09)

niltempus next followed the actual KVM away/back instruction, including display,
pointer and shortcut checks, and reported survival. PID 7778 and the same Session
remained live. Input devices 262–269 were removed at boot milliseconds
38844895–38845387. Owner 4 settled at 38845235, and Waiting reached attempt four.
Owner 5 resolved at 38874238 with zero adjustments and was ready at 38874448;
the known same-topology uncommitted record followed at 38874464. Devices 270–277
were added at 38876415–38877162 with keyboard/pointer capabilities. Device 275
produced a key observation at 38877815, and the ensuing routing records report
key_observed_count=1 and key_routed_count=1. Presentation continued without a
retained fatal or seat transition. The requested fifteen-second absence is not
an independently measured physical switch interval.

Snapshot suffix `8e9ec065-c0fe-491f-bebc-c82cd4ac0dde` under the same full
Session ID preserves both `events.0.log` and `events.1.log`; analysis includes
both rotated segments. Its verified manifest SHA-256 is
`346d427cd41bf014c0041e7ab3bbfdcb673e3e05a22dd0bfb79ef32de3960973`.
Recorder health reports zero discarded records and storage errors. Markers
`93af0d2c-45a0-49d5-b4af-ca47b0214f83` and
`21d35291-16d3-40bc-8ac5-004767069a78` delimit the instruction and report.
This supplies attended unlocked KVM survival plus recorded device return and
key routing. Locked KVM return remains separate; no task closure, code change,
build or master merge follows from this result alone.

#### Release 222 locked KVM acceptance (2026-10-09)

niltempus completed the last requested combination and explicitly reported
survival after lock/unlock. Session `efb96437` and PID 7778 remained live on
the exact release executable. Lock epoch 2 precedes loss; devices 270–277 are
removed, owner 5 settles, Waiting reaches the slow cadence, then owner 6 resumes
and devices 278–285 return. Application presentation and routed keys follow
the lock records with input epoch 11. The interval from owner 5 closing to
owner 6 becoming ready is about 98 seconds; this is a log interval, not a
measurement of the physical KVM switch. The known same-topology uncommitted
record recurs. No retained fatal or seat transition occurs in the test.

The final snapshot suffix is `c34ea252-b15e-4a33-8173-dd7ddcc2a4ca` under the
same full Session ID. Both rotated event segments were inspected. Its verified
manifest is `b14a12f679741f1da22e0e42a77ab4a9ae2a35321dc1620a55a6527ea3e5ad59`,
and recorder health has zero discarded records and storage errors. Markers
`f434c921-76e8-4993-9cdf-3b3d6c8ab9a3` and
`20894f11-3a51-4c01-9bf6-9d1f5c50ff76` bind the request and report.
The [t306 acceptance account](kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#release-222-physical-acceptance-2026-10-09)
combines this with the exact gate and release. t306's attended recovery is
accepted on this desktop. The operator's lock observation is not an independent
per-owner cover record; t297, t310 and the separate virgl investigation retain
their remaining obligations. No new runtime code was added during these tests.

#### Same-topology publication repair (2026-10-09)

The replacement path now compares its fully projected authority snapshot with
the published one before deciding whether topology changed. It normalizes only
the incoming topology epoch to the owner's current epoch; head descriptors,
mode inventory, VRR support, logical groups, mappings and primary output all
participate in equality. Missing or older published authority requires a new
epoch. A changed realization still forces publication even when its public
snapshot is equal.

An identical return retains its epoch and marks the pending snapshot already
published. It still owes the replacement's presentation barrier and the exact
transition, notice, native owner and profile binding before ledger commit.
Changed capabilities advance the epoch and publication generation and take the
existing authority-publication path after presentation. The equal-epoch stale
guard for genuinely new publications is unchanged.

Device-free evidence is `t310-transport-publication-01`. The first five tests
run with the previous decisions retained through the extracted production seam
fail three named assertions: equal-epoch settlement, changed capabilities, and
missing/older published authority. All five pass with the repair. The final
six publication tests additionally exercise incoming-epoch normalization and
install a changed mode/VRR/mapping snapshot through the real authority reducer.
All fifteen filtered realization tests pass, including the existing stale
binding, profile churn and affinity tests. These drive the production owner,
publication decision and ledger directly; they do not drive the complete
owner loop or a DRM card. Release 222 is unchanged.

The repair is signed `ea5ba9d95`. Integration `96cb6d6d8` also includes the
t322 custody repair `792885659` and bounded lock capture `1162d2f81` by exact
ancestry. Its full isolated gate passes: 537 Rust summaries, 7,513 passed,
zero failed, 101 ignored, with lint, layout and tooling checks. Start/end source
identities agree. The evidence manifest is
`722d1514d0e714175755681afce700c9c5a090318e19954296f564f9d98edb4a`.
The earlier partial gate was deliberately stopped for a test-only Copy-value
lint correction and is retained with exit 143; it is not a passing gate.
Device pixels and physical VT recovery are not established by this gate.
The later renderer-handoff reducer `969658fba` and owner-aware lock coverage
`0b1d7ab60` were reviewed separately and are outside this integration.

The matched desktop is built and verified, not installed:
`niltempus-01fa4c74950b5a998db9`, signed niltempus integration
`481fdc0abe4dca57d12e1730860ea957b3761f30`, Sophia `96cb6d6d8`.
Store path:
`/nix/store/bnir9wxayhybkkns1sddy9l2183hcfr6-niltempus-desktop-niltempus-01fa4c74950b5a998db9`.
Only Sophia changes among the release binaries. The profile differs from
accepted release 222 only in four release-path substitutions; the Mesa/libdrm
store paths and other flake inputs are unchanged. The build's profile preflight
and release checksum verification pass.

Frozen evidence is `t322-t310-release-01`, manifest
`807492cb17da556d3818c4d5f0066548a03fee61f8ea927de6e2f28c0c462bdd`.
Its `READY.txt` provides install, status and rollback commands and the remaining
attended checks. After installation, ordinary rollback returns to accepted
release 222. The candidate's GC root and local niltempus release branch are
retained; niltempus master and the installed session are unchanged. Sophia's
integrated source is merged and pushed, and the merged publication worktree
and branch have been removed. Physical VT recovery and the remaining t310
policy checks remain separate from this build result.

niltempus subsequently installed this release and accepted the zero-output
VT away/back sequence with keyboard and mouse working on reconnection. Owner 2
committed its realization after presenting under topology epoch 3; the old
stale-publication symptom did not recur. This observes a successful replacement
commit, not every branch of the snapshot comparison or the remaining affinity
policy exits. The exact session, positive records and capture limitations are
in the [t322 attended record](kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#attended-zero-output-vt-return-2026-10-09).

#### Matched desktop artifact prepared (2026-10-09)

Signed niltempus candidate `6d50e38cc5014d248d8110e8c82f928b349cecd0` pairs the
gated Sophia pin `83c68c7c4` with `exclude-gpu "pci-0000:16:00.0"`, retaining
adaptive affinity 1 and the HDMI-A-2 exclusion. The Nix build produced
`niltempus-565e7cc7483dab088875` at
`/nix/store/y99wcv7pimkkv3ivpk5kk7mfsmzbf3bi-niltempus-desktop-niltempus-565e7cc7483dab088875`.
Packaged profile validation reports `accepted policy=validated`; every release
checksum verifies. Sophia's executable digest is
`9b504a0e100280452a124f2971ea3253075b95314c19fc717eb973dbdb2ac8ea`.
Only Sophia differs among the release binaries shared with the installed
desktop; all clients and factotum binaries remain byte-identical.

Evidence is `t310-runtime-20261009/release-build-01`, manifest
`ca5f875320f1a672bd5590a19c2de05070296a56a0cd1392b8be943d03a3be7a`.
Its indirect GC root retains the candidate. The Nix daemon uses its configured
sandbox, with eight requested build cores. This is a built artifact, not an
installation or native acceptance. Current remains `niltempus-e3e6a9c375a1bfa4c7bc`
and previous remains `niltempus-19efce64b00ae803d566`. The t306/t307 native proof
and attended t310 checks remain outstanding before closure.

#### Port change after reboot (2026-10-09)

niltempus moved the main monitor between ports on the discrete AMD card before
rebooting and requested a durable startup repair. Both normal logins failed
on installed Sophia `19403a511`, release `niltempus-087445319affcb9bfc53`, with
`output_profile_unknown_connector` and the retained cause `unknown connector
"DP-1"`. Both reached native head readiness. Their diagnostics report zero
suppressed, discarded or lost records. Sysfs shows discrete PCI `0000:03:00.0`
with DP-1 disconnected and DP-2 connected; the integrated card at
`0000:16:00.0` retains HDMI-A-2. The udev rule assigns that integrated card and
render node to seat1; the discrete card remains on default seat0. The installed
profile still requires DP-1. This establishes a configuration availability
refusal, without requiring a rendering-failure hypothesis.

Evidence is `~/.local/state/sophia/development-evidence/t310-startup-port-20261009/`.
Its `incident/` retains both failed sessions, the installed profile, udev
records and connector snapshot with `SHA256SUMS`. Session IDs end in
`19724b5a-ac06-4bea-9b7e-7154eb41f9e5` and
`02bedb1b-176a-4d8a-8a89-4194567bd4d4`.

The repair is on `fix/t310-startup-output-fallback` in
`~/dev/sophia-output-fallback`, based exactly on installed `19403a511`.
Signed implementation `0a95d308d` adds explicit adaptive availability while
strict remains the default. Missing ordinary preferences are skipped; if no
output remains enabled, the least connected unnamed connector receives a
preferred-mode desktop, automatic scale, normal transform, VRR off and focus.
All named connectors and mirror members are excluded from the fallback pool.
Mirrors and unsupported settings on present named outputs remain strict.

The explicit fallback policy key transfers workspace affinity to that port
for the session and removes every previous claim to the same key. Reloads
that change configured keys, availability or fallback key are declined before
staging a replacement WM. Review found that the previous publication error
propagated out of the owner loop and ended the session; the new early check
returns `Declined` and preserves its generation, profile, bindings and WM.
Topology reload also refuses a change to the realized startup binding.

Signed follow-up `838d5b16acad9a6afd119ce25b8622c068b32646` moves the five
prepared shell accessors unchanged into their own module, preserving the
public facade. `full-gate-01.log` passed tests and clippy but refused the
profile parser at 1,002 lines. The split resolves that failure without
relaxing the debt ledger. `full-gate-02.log` exits zero on the final signed
source, including workspace tests, both SDK checks, clippy, layout, verifier
controls and six retained direct-scanout archives. The source matches
`candidate-02-source.SHA256SUMS` after the run. Nineteen new controls cover
configuration, startup affinity and safe reload settlement.

The gate ran with no DRM/input devices, no network and a private target.
Earlier focused runs exposed fixture files made group-writable by the host's
umask 0002. `isolated-private.py` sets umask 0077 before launching the checks;
the original wrapper and failed logs are retained. No unrelated fixture
permission patch remains. Device-dependent pixel checks explicitly report
that they are not proved in this environment.

The matched integration commit is signed
`73498befe6c656707c34f83ecfdce6b4ca3c30c3` on
`fix/t310-daily-output-fallback` in `~/dev/niltempus-output-fallback`. Its
profile enables adaptive fallback key 1 and explicitly disables HDMI-A-2,
including when absent. Its lock binds Sophia to the exact local signed commit;
preserve the source worktree until that pin is replaced by a published source.
The prior t306/t307 packages and unqualified master changes are not included.

Nix built `niltempus-19efce64b00ae803d566` at
`/nix/store/6lhvb1x9a531gbavv93qpxf6iykm1spa-niltempus-desktop-niltempus-19efce64b00ae803d566`.
The packaged profile passes `status=accepted policy=validated`; all packaged
checksums pass. Sophia SHA-256 is
`96f7f48f5008b5f9a5a980746330814c17b5eabf10f10205c504fc7a590cc16f`.
All client and factotum binaries match the installed baseline byte for byte.
`release-build-01.log`, `release-binaries.json` and `RESULT.json` retain the
build and verification identities.

Installation was first attempted through the reviewed `tools/desktop install`
command but sudo required a password and the tool terminal had no graphical
authentication agent. No release copy or current/previous switch occurred.
The initial `RESULT.json` retains that pre-install disposition. On DP-2,
fallback uses the EDID preferred refresh rate with VRR off; the saved 120 Hz
and VRR preference is still specific to DP-1.

Automatic hotplug reconstruction still does not rerun profile reconciliation.
Runtime exclusions, workspace migration, all-head suspension and retained-image
recovery remain separate obligations with t306. This startup repair does not
close t310 or change the prior shutdown checkpoint's unqualified evidence.

#### Startup accepted on DP-2 (2026-10-09)

niltempus subsequently reported being back in the live session. Read-only
checks confirm `current` is `niltempus-19efce64b00ae803d566`, with the prior
`niltempus-087445319affcb9bfc53` retained as `previous`. The normal Session
process, PID 10449, runs the new installed path on seat0 with Hagia over
9P2000.L. Session `00000001791549817928-0337103c-3efd-4c02-b52a-977f4b9caed3`
records commit `838d5b16a` and the exact verified binary hash above. Sysfs
shows DP-2 connected/enabled, DP-1 disconnected/disabled and HDMI-A-2
connected/disabled. This accepts startup fallback for the reported port move.

`LIVE-LOGIN.json` and `LIVE-LOGIN.SHA256SUMS` bind this observation to a
checksummed preserved session under `~/.local/state/sophia/session-investigations/`,
suffix `-0dbccf62-3e7b-4646-b88c-d4092c3f09dd`. The running-session snapshot
reports zero discarded records and storage errors, but 20,317 records were
suppressed by per-kind volume limits. It is incomplete event evidence, not a
whole-session verdict. No live topology, reload or lock intervention was made.
Runtime loss/return and card admission remain open under t310 and t306.

The operator expects the desktop to handle changing monitor combinations at
login and during a session. The DP-1 profile is an immediate configuration
repair, not that capability's acceptance. Define saved output settings as
preferences for available connectors: an absent preferred monitor should
not prevent login on another eligible output. Explicit exclusions still
apply, including the operator's iGPU development monitor. Do not silently
reclaim an excluded device when selecting a fallback.

Specify startup fallback and runtime reconciliation together, including
workspace and window reachability on surviving outputs, reconnect affinity,
and suspension with retained state when no eligible output remains. Keep
required-output semantics explicit for proof fixtures that depend on exact
geometry. Resolve how policy keys and unnamed connectors are assigned before
changing the current strict reconciliation rule.

Acceptance needs startup and loss/return controls across different monitor
sets, no outputs, and an explicitly excluded GPU. Include static DMA-BUF
clients so losing a head cannot discard their only retained source. The
runtime continuity repair is t306; neither these profile edits nor a passing
startup check close it. Full concurrent development sessions also still need
separate card and seat/VT ownership.

Card admission must be specified separately from output policy. Excluding a
connector from the layout does not prevent Session from opening its DRM card
or render node. The iGPU passthrough preparation exposed this distinction:
the live Session retained card1 and renderD129 while HDMI-A-2 was excluded.
Today admission follows udev's seat assignment; a missing `ID_SEAT` means
seat0 only for an initialized record.

Decide whether the profile exposes a card exclusion or allowlist whose
meaning is "do not open this card". Use stable PCI/udev identity, such as
`pci-0000:16:00.0`, rather than `cardN`. An absent excluded card must not
prevent login. Profile selection may only narrow the cards assigned to the
session's seat; it must not grant access to another seat. Specify the same
boundary for render-node discovery and shell GPU selection so neither can
reopen an excluded card. Controls must cover changed card numbering, absent
exclusions, foreign-seat cards and a shell render-node request that conflicts
with admission. This remains t310 design work, not a promotion or a device
ownership change. The separate udev seat rule used for iGPU preparation does
not establish profile-level card selection.

## Native cache comparison reached the workload (2026-10-07)

Fixture03's explicit local-session lookup worked. The candidate smoke and
first baseline/candidate runs completed at the exact DP-1 geometry with
radeonsi, clean application groups and restored TTY modes. The comparison
stopped at its frozen requirement that `pending_frame_supersessions` be zero:
the paired candidate recorded one. The original result remains **FAILED**;
there was no replacement run and no cache merge or installation.

| Run | Desktop CPU over 1,800 measured frames | Completed compositions, whole Session | Observed pending replacements |
| --- | ---: | ---: | ---: |
| Candidate smoke, excluded | 1.41 s | 1,953 | 0 |
| First baseline | 1.53 s | 1,949 | 0 |
| First candidate, refused by validator | 1.32 s | 1,949 | 1 |

Each measured interval was about 30 seconds at 60.03 fps. Baseline and
candidate both captured, promoted and evicted 1,920 snapshots including
warmup, with six imports and no renderer failure, slot deferral, exporter
replacement or direct-scanout work. The paired candidate had 1,941 cache hits,
two discoveries, zero discovery failures and zero invalidations. These are
adapter counts, not a count of kernel ioctls saved. The raw CPU difference is
13.7%; one refused pair does **not** establish a repeatable performance gain.

The replacement counter measures pending exporter frames overwritten before
worker submission, plus latest-wins deferrals and direct fallbacks. Those
last two paths are excluded by this recipe. A pending replacement is not
itself a failed client Present. The owner samples a maximum of the summed
counters per tick, so the terminal value can miss later overwrites: it is
an observed lower bound, not an exact offered-frame ledger. Engine damage
history is based on committed states; a superseded candidate does not advance
the next candidate's damage baseline.

Fresh fixture04 therefore reports replacements by arm and pair while retaining
all client/snapshot checks and requiring candidate completed compositions at
least equal to baseline in every pair. CPU is never normalized by this count.
All pairs must improve, with a median saving of at least 5%. It also explicitly
requires zero page-flip phase and overlap rejections, and adds pairwise bounds:
candidate maximum submit-to-flip no more than baseline plus one reported
millisecond, and no increase in maximum in-flight depth. Deferral-decision
counts and maximum in-flight owner ticks remain descriptive; they are not
elapsed time. These whole-Session maxima do not prove latency percentiles
over the measured interval. A late candidate startup flip can refuse a pair;
a late baseline flip can loosen that pair's limit.

Seventeen validator/driver controls pass, including rejection of fewer
completed compositions despite supersessions and cheaper CPU with worse
timing. Three mutations are detected; the first runner's assertion-only
detector stopped on the expected old-rule exception and its log is kept.
Both argument/profile checks pass. Arm binaries, settings, wrapper and
workload are unchanged. Fixture04 has no native result yet and requires a
fresh quiet window and local tty3 launch.

Evidence: `t289-native-kms-property-cache-03/DISPOSITION.txt`, the original
`qualification-20261007T112207.476147Z/RESULT.json` and
`FAILED-RUN.SHA256SUMS`; successor `t289-native-kms-property-cache-04`, frozen
manifest `45826909111a3dcdf67c8c41a00c891607bf3a40bdefc82b59d91c495f112e78`.
Claude's independent source check agrees that observed supersessions can be
reported with these work guards. His concrete fixture review found no blocker.
Its requested descriptive tick field and timing-limit wording are in the final
freeze `07ef8edc0c2f68f44a6e5c85d4036c7d4786c086d12d7c7a5f645cb23b7b5965`;
the reviewed predecessor is kept in `reviewed-freeze-01`. All 17 controls pass
on the final source.

Separate cleanup follow-up: the wrapper kills its watchdog subshell but leaves
the current `sleep 270` child until expiry. Three such idle orphans were
observed after these runs and expired naturally; no Sophia, benchmark or perf
process remained. Fix watchdog-child cleanup separately and exercise normal
completion and forced shutdown. The application-group receipt does not cover
all wrapper descendants.

## Native comparison refused session lookup (2026-10-06)

The corrected record parser in `t289-native-kms-property-cache-02` passed review,
but its candidate smoke at `20261007T014040Z` failed before native graphics
startup: `libseat open failed: No data available`. No benchmark frames or
comparison pairs ran. TTY modes and the application group recovered cleanly.
The failed result and logs remain unchanged, bound by `FAILED-RUN.SHA256SUMS`;
`DISPOSITION.json` records the interpretation. The candidate bytes are identical
to fixture01's successful native smoke. This is no performance result.

Read-only queries reconstructed a concrete login-selection failure. On this
elogind host, tty3's shell belongs to session 32 with cgroup `0::/32`. Host
libelogind identifies it correctly. The Nix libsystemd used by pinned libseat
returns `-ENODATA` for the same PID, then its user-display fallback returns
session 69, an SSH session with no seat; querying that seat also returns
`-ENODATA`. Both libraries accept explicit session 32 as active on seat0.
The pinned seatd 0.9.3 source, `libseat/backend/logind.c:607-667`, first honors
`XDG_SESSION_ID`, otherwise uses PID lookup and then the user-display fallback.
Our minimal benchmark environment omitted the explicit ID. The failed process
did not record which ID it chose, so these are a post-run reconstruction and
source explanation, not a recovered trace of its lookup.

Successor fixture03 resolves the launcher's own PID through the pinned host
libelogind and refuses unless UID, local/user/tty class, seat0, active state,
TTY, VT and exact cgroup agree. Two observations must retain the process and
session identity; an inconsistent inherited `XDG_SESSION_ID` refuses. It records
the session file and passes only that validated ID into both arms. This happens
before the wrapper can change TTY state. It never chooses the user-display
fallback, changes seats or opens a device during preflight. Normal launches
that already carry the correct PAM session ID use libseat's explicit-ID path;
the Nix/elogind mismatch matters when that variable is omitted.

Seven new seat controls and all twelve existing comparison controls pass. The
real host query accepts the tty3 shell and rejects the remote tool process;
both argument/profile checks pass. Claude reviewed fixture03 ACCEPT, verified
the same controls and host queries, and supplied a fresh quiet ACK. Frozen
manifest SHA-256 is
`129181bd4fa653d6c20bb03f9de1e952550e7ec39b34470f7b8befa0cca126a2`.
The stale copied `REVIEW-REQUEST.txt` remains frozen; `REVIEW-REQUEST-03.txt`
explicitly corrects it. The candidate/baseline binaries, wrapper, workload,
acceptance rule and comparison order are unchanged. Native execution remains
operator-initiated on the active local TTY: one C smoke then BC CB CB BC, stop
first failure, no replacements. No cache merge or install is implied.

## Question

After retaining Mesa's software framebuffer mappings, what accounts for the
remaining desktop CPU cost? Does the evidence justify SIMD or assembly work,
or is there still unnecessary work to remove?

**Moving the owned raster command into its journal reduced median CPU by 6.3%**
in the fixed software workload, accepted on October 6. The
[measured result below](#owned-journal-move-accepted-2026-10-06) records the four
pairs and their limits. The [native DMA-BUF profile](#native-dma-buf-attribution-2026-10-06)
now puts the owner loop first: 0.86 of 1.49 desktop CPU seconds over 30 seconds.
Repeated KMS property discovery is the next concrete candidate. The separate
owned-upload candidate still needs measurement on the software recipe.

## Evidence

One Sophia profile followed by one unprofiled control completed on October 5
local time. Both used the unchanged wmbench `47fdb6b` bundle, Sophia
`825d91460`, and the measured Mesa mapping-retention candidate enabled in both
arms. The host was logged out at greetd, with other development work paused.
Each guest had four CPUs, 3072 MiB and a virtio 2D GPU running llvmpipe. The
1160×680 SHM client ran 120 warmup and 300 measured frames on a 1280×800 output.
No host GPU was passed through.

| Run | Desktop CPU per 300 frames | Elapsed |
| --- | ---: | ---: |
| Profile | 1.47 s | 39.0 s |
| Unprofiled control | 1.42 s | 38.9 s |

Both guest and host wrappers exited zero, both passed compatibility and the
48-frame Mesa pixel/cleanup helper, and loaded binary identities matched.
The control agrees with the earlier ON range of 1.41–1.44 seconds. One pair
does not establish repeatability or a stable profiler overhead. The workload
name says “render 60 fps”; achieved throughput was about 7.7 fps in this guest.

The profile recorded **zero minor and major faults** over the measured desktop
interval, with 1.38 seconds user CPU and 0.09 seconds system CPU. There were
713 CPU-clock samples at a 2 ms period, none lost: 1.426 seconds of sample
weight against 1.47 seconds of process accounting.

| Thread group | Samples | Share |
| --- | ---: | ---: |
| llvmpipe raster workers | 307 | 43.1% |
| X11 client worker | 188 | 26.4% |
| Session owner | 159 | 22.3% |
| Renderer group worker | 59 | 8.3% |

These groups partition the samples. Symbol costs below overlap them:

- `memmove`: **233 samples, 32.7%**, comprising 170 on the X11 worker,
  43 on the owner and 20 on the renderer worker.
- Owner `memset`: 23 samples, 3.2%.
- Mesa `util_fill_rect`: 49 samples, 6.9%.
- Unresolved Mesa JIT code: 209 samples, 29.3%.
- Kernel code: 48 samples, 6.7%.

Whole-session work matches between the two runs: three targets, 429 target
reuses, 432 worker requests and completions, 421 exact-nearest draws, zero
snapshot captures/imports and zero COW splits. Those counters include startup
and warmup. They must not be divided by the measured 300 frames as though they
cover only that interval.

## Finding and resolution

### First candidate: move the command already owned by the journal

At the measured source, in
[raster_variants.rs](../../../crates/sophia-x-authority/src/software/raster_variants.rs):

- `XOwnedImagePixels` owns a `Vec<u8>` and derives `Clone` (lines 134–138).
- `from_put_image` retains the accepted pixels with `to_vec()` (line 231).
- `SurfaceRasterStore::record` takes its command **by value** (line 609),
  then pushes `command.clone()` in both accumulation and replay paths
  (lines 660 and 676).

The proposed small change is to finish coverage calculation or variant replay
through `&command`, then move the command into the journal after its last use.
This should eliminate one payload allocation and copy for a retained PutImage
without changing the payload type or upload interfaces. Every validation,
budget, poison, coverage and baseline-reset decision must stay intact.

The profile supports prioritizing copies, but does not isolate this clone's
share. Only seven X11 copy callchains recovered a caller: three reach
`from_put_image`, two reach SHM dispatch and two reach `packed_patch_region`.
One owner copy chain reaches `compose_layer_clipped`. The measured Sophia
binary contains a `memcpy` call inside `from_put_image` at ELF address
`0xdf0732`, confirming that copy survived optimization. No percentage saving
is assigned to the proposed journal move.

### Follow-up: share immutable upload ownership where it is safe

If the small slice is useful, inspect the next ownership boundary. SHM dispatch
already holds an owned normalized upload, but passes a slice through drawing;
`from_put_image` then allocates another retained buffer. Separately,
`SurfaceRasterStore::satisfy` clones the journal when staging an atomic change
to retained density variants (line 744).

An immutable owned payload shared across those retained readers could remove
further copies. The initial client-memory snapshot remains necessary. Shared
bytes must never alias mutable client or canonical drawable storage. Preserve
format conversion, crop/stride checks, byte order, graphics-context semantics,
journal budgets and atomic requirement satisfaction. This is a separate,
broader candidate; do not combine it with the first measurement.

The earlier whole-image crop candidate remains inconclusive and unmerged.
The accepted shared CPU-patch forwarding already avoids queue-to-queue copies;
patch packing and applying rows into the destination are distinct remaining
operations. Zero COW splits here gives no reason to weaken snapshot isolation.

### SIMD is already used in the largest named copy routine

All sampled copies use glibc's `__memmove_avx512_unaligned_erms`. Disassembly
at sampled instruction offsets shows ZMM vector loads and stores
(`vmovdqu64`/`vmovdqa64`). Replacing this with handwritten SIMD is not the
first candidate. There are no hardware bandwidth, cache-miss or IPC counters
in this capture, so these instructions do not prove memory-bandwidth saturation.

llvmpipe is the largest thread group, but its generated routines lack symbols
in this binary. Mesa's `lp_profile` symbol/assembly dump is compiled behind
`PROFILE`; its identifying dump-path and environment strings are absent from
the measured Gallium ELF. A later renderer investigation needs a separately
qualified diagnostic build with JIT symbols before choosing a specific loop.

`util_fill_rect` uses `rep stos` and library fills in the measured binary.
Its 6.9% share is an upper cost bound, not a forecast for vectorization.
The last periodic record attributes 419 full repaints to the coverage decision;
the client covers 77.0% of the output. This is not evidence of an incorrect
damage decision. The measured clear-coverage candidate remains rejected.

## Original validation plan

The October 5 profile proposed this slice under t289:

1. Implement only the owned-command move. Test allocation identity in both
   journal paths, exact variant replay, full opaque baseline resets, partial
   coverage, budget/unsupported refusals and independence from later writes.
2. Run the focused tests and required full gate on the frozen candidate.
3. Compare against this accepted Sophia baseline with the **same Mesa ON in
   both arms**, unchanged client bundle and frame count, and four balanced
   pairs (BC CB CB BC). Keep one attribution profile per arm. Coordinate the
   quiet window; stop on a failed run and keep it.
4. Apply the existing requirement that every pair improves, with no pixel,
   lifecycle or work-count regression. If the change is within noise, retain
   that result and move to the next measured cost. Source simplicity alone
   does not establish a performance gain.

The profile itself changed no implementation. Reviewed production crates
on master `a7e4aedf9` are identical to measured `825d91460`.

Limits: 468 samples have no callchain and 54 have only one frame; the remaining
191 have multiple frames. Per-callsite copy percentages are unavailable.
This is the **SHM/software guest path**, with no DMA-BUF captures. It does not
measure Kitty's hardware path, whole-machine energy or laptop battery life.
XLibre was not rerun, so no new cross-server ratio follows.

Evidence: `~/.local/state/sophia/development-evidence/t289-remaining-hotpath-01/`:
`READY.json`, both guest trees, `ANALYSIS.json`, `SOURCE-RECEIPT.json`, raw
`perf.data`, self/callchain reports and disassembly. `python3 analyze.py`
rechecks identities and recomputes the attribution. `RESULT.txt` SHA256:
`584dbc7b9b05e444e24aeac9d59ff0004d7c40c6f45a6e2bd7a3d9c857998232`.

## Owned journal move accepted (2026-10-06)

Signed production commit `97a9e4ce6` moves the command after its last coverage
or variant-replay borrow in both journal paths. Allocation-identity controls
fail on the old clone and pass with the move; they also check replay pixels,
generations, prior snapshot independence, partial coverage and full-baseline
reset. Focused tests, strict clippy, formatting, layout and the full isolated
`cargo xtask check` pass on the clean candidate. Peer source review accepted it.

The comparison used the recipe above, unchanged `wmbench 47fdb6b` and the same
Mesa mapping-retention candidate ON in both arms. Baseline Sophia `825d91460`
has the same production crates as the candidate's parent, `650e5c62b`.
Only Sophia changed in the two VM settings; companion binaries and the loaded
Mesa hashes match. The host remained logged out at greetd with competing work
paused. All ten fresh guests passed their workload, identity and Mesa
pixel/cleanup checks. Four unprofiled pairs ran in BC, CB, CB, BC order:

| Pair | Baseline CPU | Candidate CPU | Baseline elapsed | Candidate elapsed |
| --- | ---: | ---: | ---: | ---: |
| 1 | 1.44 s | 1.33 s | 39.0 s | 39.3 s |
| 2 | 1.42 s | 1.30 s | 39.2 s | 39.2 s |
| 3 | 1.42 s | 1.33 s | 39.2 s | 39.2 s |
| 4 | 1.42 s | 1.34 s | 39.2 s | 39.2 s |

Median desktop CPU per 300 measured frames fell **1.42 → 1.33 seconds (6.3%)**,
or 4.73 → 4.43 ms per frame. Every pair improved, with separated ranges and
exactly 432 worker compositions in every run. Both elapsed medians are 39.2
seconds; this establishes no throughput or latency improvement.

Whole-session work also matches: 420 CPU updates, comprising one replacement
and 419 patches, 1,325,184,000 payload bytes, three targets and 429 reuses,
421 exact-nearest draws and no captures, imports or COW splits. Every update
was bound and accounted for. Two runs per arm presented all 420 updates; the
other two presented 419 and released one at teardown. That terminal split is
balanced across arms and did not reduce rendering work. There were no pending
worker supersessions, slot deferrals, worker failures/hard stalls, exporter
replacements, topology events or direct-scanout work.

The separate attribution pair measured 1.44 → 1.40 seconds CPU and zero minor
or major faults. CPU-clock profiles contain 695 → 685 samples, none lost.
X11-worker samples fell 182 → 160 and its `memmove` samples 168 → 139.
Total `memmove` samples fell 232 → 219, while owner and renderer copy samples
varied upward. This supports removing an X11 copy, but one sampled pair does
not assign an exact CPU share to the journal callsite.

### Failed measurement attempts retained

`comparison-01` and `comparison-02` remain **FAILED** and contribute no CPU
observations to acceptance. Their guests passed; the added validator made two
overly specific assumptions. The first required 429 target reuses even when
one pending frame was replaced before reaching a worker (431 requests and
428 reuses). The second required 419 presented plus one lifecycle-superseded
update, and refused a baseline that presented all 420.

The successor checks source-backed accounting and records both terminal counts.
It excludes direct scanout, worker deferrals, exporter churn and topology
changes before checking requests plus pending replacements. It conservatively
requires 432 offered frames and at most three terminal lifecycle outcomes for
this recipe; those are qualification bounds, not generic source invariants.
Thirty-nine controls pass. Offline archive checking accepted eleven matching
logs and explicitly excluded one with 433 offered frames. The final fresh
series used these frozen checks, stopped on any failure and allowed no
replacement runs. It met the stricter criterion that every CPU pair improve
with candidate worker compositions at or above baseline.

Evidence: `~/.local/state/sophia/development-evidence/t289-raster-journal-move-01/`:
`RESULT.txt`, `comparison-03/RESULT.json`, `12-attribution.json`,
`07-preflight4.json`, `05-gate.json`, RED/GREEN controls, peer review and both
failed-series dispositions. `attribution3.py` recomputes the sampled attribution.

This result is limited to the SHM/software guest path, with Mesa retention ON.
There is no new XLibre comparison, hardware DMA-BUF, battery or live-session
claim. No install was performed. t289 remains open; the next candidate is the
immutable upload-sharing boundary described above, measured separately.

## Native qualification started (2026-10-06)

The operator asked to move as much measurement as possible onto crunch's real
hardware. Both DP-1 (2560×1440) and DP-2 (1920×1080) are connected on the discrete
GPU; the desktop is logged out at greetd. The earlier software guest percentages
do not establish the cost distribution on this hardware.

One bounded render-node invocation passed all six ignored `snapshot_reuse`
tests in 1.67 seconds on renderD128 (PCI 0000:03:00.0, AMD Navi31). The sandbox
exposed only that render node. This checks snapshot independence, imports,
reuse and resource reclamation on the physical GPU; it measures no Session CPU,
KMS presentation, latency or power. Evidence:
`~/.local/state/sophia/development-evidence/t289-native-render-node-01/`
(`RUN.json`, `test.log`).

A native wmbench compatibility fixture is prepared in
`t289-native-wmbench-01`, using the published `97a9e4ce6` binary and unchanged
wmbench. Its private profile keeps both physical heads active, with the benchmark
on DP-2. It checks the actual client geometry after 120 warmup frames, before the
upstream measurement gate opens for 300 frames. The intended client is 1800×960;
the output rates are 60 Hz and 120 Hz. These differ from the software guest
recipe, so native CPU results will form a new baseline.

The exact profile and Session arguments pass parser-only checks. Native launch
uses the existing TTY wrapper with a real seat, independent input recovery and
a 270-second watchdog; Session is bounded to 240 seconds. Configuration and
runtime state are private to the attempt. A surviving benchmark application
group is cleaned up and fails qualification. Nothing is installed, no service
is stopped, and a failed attempt is retained rather than silently repeated.

The first native Session ran from the operator's tty3 and failed after applying
the output layout, before wmbench began. Selecting DP-2 as primary made the CPU
scene size disagree with the first composition descriptor. The
[startup mismatch investigation](bxeem6rg-changing-the-primary-monitor-mismatched-the-cpu-scene-descriptor.md)
owns that repair and its CPU regression. Console recovery passed, and the failed
run is preserved. It provides no native CPU measurement.

After qualification, profile the same hardware workload and use its largest
costs to select the next source change. Keep real DMA-BUF and software upload
results separate, based on the renderer and transport actually observed. The
remote control process remains outside the local seat, so a fresh native launch
still needs a process started from an active local TTY.

## Owned upload candidate prepared (2026-10-06)

While the native retry awaits a local seated launch, branch
`performance/t289-owned-upload` prepares the next copy reduction on top of the
startup repair. This candidate is **unmeasured and not accepted for promotion**.

Core PutImage decoding and SHM extraction already produce private, normalized
pixels. The drawing path now carries that ownership through the canonical
writer into `from_put_image`, instead of borrowing it and allocating another
copy for replay. Cross-drawable copies likewise already own their extracted
pixels. Public borrowed callers still receive a private snapshot. Client SHM
reads, format conversion, crop packing and canonical drawable writes remain.

Only a Vec whose length and capacity exactly equal the retained prefix moves
into the journal. Borrowed data, trailing bytes or excess reserved capacity
take the bounded copy path, preserving the existing byte charge. An
`Arc<Vec<u8>>` shares immutable journal pixels during atomic density staging;
each command still incurs its full logical charge. Variant backing keeps its
existing copy-on-write behavior. Refusals and baseline resets are unchanged.

The planned 1800×960 native window has a 6,912,000-byte tight upload, larger
than the journal's 4 MiB payload cap. The current path copies that supported
upload before the journal reports `JournalCapacity`. A regression proves the
candidate can move it and retain that same refusal without copying its pixels.
This is source and allocation-identity evidence, not a claim about the native
client's observed transport or CPU savings.

Controls cover allocation identity, immutable command clones, borrowed input
independence, prefix/capacity bounds, staging and failed-demand atomicity, budget
charges and last-owner release. A wire-level SHM control rewrites and detaches
the client segment before requesting a density variant, then changes the
drawable and checks the earlier snapshot in both byte orders. Existing core,
SHM, GC and replay suites remain part of validation.

Evidence: `~/.local/state/sophia/development-evidence/t289-owned-upload-01/`.
Keep this slice separate from the inconclusive whole-image crop change.

Signed candidate `b02b47c45` has read-only peer acceptance and a full isolated
gate at exit 0, with 7,179 printed Rust passes and zero failures. The focused
authority, core wire, SHM and replay suites pass. Three mutants fail their named
assertions: copying the owned payload, deep-copying it during density staging,
and retaining excess Vec capacity. The gate excludes device tests and provides
no native performance result.

The release build passed from the same clean commit. Candidate Sophia is
`539d507768abae4864b4444f2c717fd9a0ea67ce6165ae5b8c7bbb21c6b55a3c`;
the corrected baseline is
`61b8ec422dbed189281a87f19bdf78004be1fcdd365e098b8c0f278263b05e11`.
Both are retained in the Nix store. Factotum and PAM match byte for byte.
`PAIR-IDENTITIES.json` binds the builds to the frozen native fixture;
`candidate-settings.json` changes only Sophia's path, revision and digest.
Candidate profile and argument parsing passed without launching a Session.
The baseline fixture itself is unchanged.

Next, qualify that baseline on the real seat, then compare the candidate with
the same geometry, client, transport and accounting scope. Record native work
counts before choosing the comparison checks; the guest's observed composition
count is not a hardware invariant. Use balanced unprofiled pairs for CPU time
and separate profiles for attribution. Preserve failed attempts, and refuse a
performance claim if the candidate did less work. This candidate remains
unpublished pending those measurements; no install or native run was performed
while preparing it.

To reduce console handoffs, `t289-native-owned-upload-01` now prepares one
command for baseline then candidate compatibility. It copies the reviewed
native launcher unchanged into two frozen arms; settings differ only in the
Sophia path, revision and digest. A failed baseline prevents the candidate
launch, and any failure is retained without replacement. The existing Session
bound, watchdog and cleanup remain responsible for each launch. Six CPU-only
driver controls and both actual profile/argument checks pass. Read-only peer
review accepts the pair driver; it now prints each arm and the recovery key
before launch, then the arm verdict. At that freeze, neither arm had run on the native seat.
Its verdict covers compatibility and cleanup, not native scanout pixel
equivalence or a performance improvement. After qualification, inspect the
native work records before freezing the performance comparison checks.

## Connections

The [framebuffer mapping investigation](djo84ohx-repeated-kms-software-mappings-account-for-the-wmbench-fault-storm.md)
establishes why this profile uses the opt-in Mesa candidate. Its 30.6% software
CPU saving and remaining device/suspend limits still stand. The
[CPU plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#remaining-hot-path-after-mapping-retention-2026-10-05)
owns this next experiment, and [todo t289](../../../todo.md) remains the queue.

## Native qualification stopped at GLX loading (2026-10-06)

The operator ran the frozen pair. Baseline B applied the two-output policy and
continued composing, so the repaired CPU-scene mismatch did not recur on this
path. The benchmark then failed with `no visual` from `glXChooseVisual`, before
creating a window or reaching its measurement checkpoint. Candidate C stayed
held. Application cleanup and TTY restoration passed. Keep the original failed
run: `t289-native-owned-upload-01/B/smoke-20261006T224240Z`. There is still no
native performance result or complete compatibility pass.

The failure is a benchmark library-loading gap. A renderD128 EGL inventory on
the pinned radeonsi stack included depth 24/stencil 0, contrary to the initial
missing-config hypothesis. A private Sophia XAuthority socket selected visual
34 using the pinned GLX library. The decisive control used the exact wmbench
executable, with an observer that exits before `XCreateWindow` and records its
loaded libraries:

- Original environment: `no visual`, exit 1, no Mesa GLX vendor loaded. The
  loader searched the absent NixOS `/run/opengl-driver/lib` and other Nix paths,
  without reaching the pinned Mesa directory. It did not load host Mesa.
- Same executable and socket, with the three pinned glvnd/Mesa/GBM directories
  in its library search path: visual 34 selected through DRI3/radeonsi, with
  the pinned Mesa GLX vendor and gallium library mapped. Exit 0.

Both selection invocations finished below 0.12 seconds. No Session, KMS,
window, GL context or draw was involved. The executable's RPATH alone did not
supply the vendor library to this dynamic lookup. No Sophia GLX catalog or
rendering change is warranted by these results. The failed setup and observer
attempts remain labeled invalid in `t289-native-glx-visual-01/RESULT.txt`.

Successor fixture `t289-native-owned-upload-02` applies that environment only
to the benchmark child, identically for B and C. Before releasing the warmup
barrier, it requires one renderer process in the benchmark's child tree,
checks its identity across the maps read, and records the exact pinned GLX,
Mesa vendor and gallium paths and hashes. Missing, foreign, extra or deleted
libraries refuse measurement. Original wrapper, TTY recovery, cleanup,
geometry, binaries and bounds remain unchanged. Eleven driver/graphics
controls, five existing native controls and both profile/argument checks pass.
A native retry from the active local TTY is still needed; the owned-upload
candidate remains unpublished and unmeasured.

## Native warmup reached DMA-BUF; placement refused (2026-10-06)

The operator ran `t289-native-owned-upload-02`. Baseline B reached the Radeon
RX 7900 GRE renderer and completed 120 DMA-BUF warmup Presents. The previous
GLX selection failure did not recur. Hagia placed the window at **2440×1320 at
1980,60 on DP-1**, instead of the frozen DP-2 geometry. The client geometry guard
refused before `measure.go`; candidate C never ran. This remains a failed
qualification, with no measured CPU result. Evidence: the fixture's
`RESULT.txt`, `RESULT.SHA256SUMS` and `B/smoke-20261006T230554Z`.

The Session completed and cleaned up normally: wrapper exit 0, no surviving
application group, no pending native cleanup, and TTY restoration without
emergency recovery. The client's `client-error.json` names the geometry error;
the outer verdict instead reported the missing terminal `client.json`. The
loaded-library attestation was after the geometry guard, so it was not reached
in this attempt. Do not infer that receipt from the earlier selection probe.

The source explains the placement: `update_public_work_areas_at` preserves the
WM's active output when it remains live. Making DP-2 the profile primary did
not replace that active DP-1. Hagia follows `snapshot.activeOutput`; monocle
with gaps 59 and the 1px border yields the observed DP-1 size. Record this
focus-at-startup/active-output mismatch separately from performance. A repair
must distinguish initial focus selection from later topology updates that
should preserve focus; this fixture does not change WM production behavior.

The transport also settles the optimization scope: the run records 120
snapshot captures, promotions and evictions, with **zero CPU updates and zero
CPU payload bytes**. The owned-upload candidate is not exercised by this native
workload. Keep its copy-saving measurement on the software upload recipe; do
not use a native B/C difference to claim that saving.

Successor `t289-native-dmabuf-01` is a single corrected published baseline,
`cb2cc176b`, for the real DMA-BUF path. It keeps both heads and their positions,
makes DP-1 primary, and uses Hagia policy `gaps 0` plus left/right struts 379 and
top/bottom struts 239. A CPU-only control loads this exact profile through Hagia
source `155daab6c`, applies its policy candidate, and runs the real monocle
projection: outer **1802×962 at 2299,239**, predicting content **1800×960 at
2300,240** after Sophia's 1px inset. The actual native geometry must still match.
This agrees with wmbench's centered 1920×1080 stage and 60px margins on DP-1.

The successor checks that exact content rectangle, DP-1's output/head identity,
and the primary RandR mapping. It checks post-configuration RandR rates of
120 Hz on DP-1 and 60 Hz on DP-2; bootstrap `native_head` rates are not proof of
the applied configuration. The workload targets 60 fps on the 120 Hz head.
This is a new native recipe, not comparable directly with the QEMU workload.
The graphics map receipt is taken before the geometry guard, and the outer
verdict now preserves a client failure's original message.

Twelve CPU fixture controls, the Hagia projection control and actual
profile/argument checks pass. Peer review accepts the placement/launcher
changes; wrapper, bounds and group/TTY recovery are byte-identical to the prior
fixture. No native launch was performed while preparing the successor. The
remote tool has no active local TTY; the prepared launch remains an operator
console command. A completed native baseline and profile are still needed to
choose the next DMA-BUF optimization. The owned-upload candidate remains held.

## First native DMA-BUF baseline passed (2026-10-06)

`t289-native-dmabuf-01/smoke-20261006T232842Z` passes the frozen native
qualification on baseline `cb2cc176b`. The actual window is **1800×960 at
2300,240 on DP-1**. RandR reports DP-1 primary at 120 Hz and DP-2 active at
60 Hz before and after the workload. The client reports RX 7900 GRE/radeonsi,
and its pinned GLX, GLdispatch, Mesa vendor and gallium mappings are attested.

The unprofiled report gives:

| Interval | Desktop CPU | Elapsed | Approximate share of one core |
| --- | ---: | ---: | ---: |
| Idle | 0.02 s | 10.0 s | 0.2% |
| 300 render frames | 0.28 s | 5.0 s | 5.6% |

The client reports 60.20 fps. CPU accounting sums user and kernel time for the
Sophia owner, its protected Sophia helper and Hagia, with unchanged process
identities. It excludes benchmark client CPU and GPU execution. The render
figure is approximately 0.93 CPU ms per client frame, not presentation latency.
Accounting is at 100 Hz; the idle figure is only two aggregate ticks. The
frozen fixture retains full Session records in a private raw log; keep this
logging mode fixed during attribution. This is one native observation, with rounded elapsed time, not a repeatability claim,
an optimization acceptance or a native comparison with XLibre.

Whole-Session work records include setup, 120 warmup frames, the 300 measured
frames and teardown. They show 420 snapshot captures, promotions and evictions;
zero CPU uploads or payload bytes; six composition targets and 442 target
reuses; six import-cache imports and 416 hits. All 448 worker requests complete,
with no worker failures, soft/hard stalls, generation/recovery replacements,
frame-slot deferrals or direct scanout. Present records contain 419 copy
completions, one skip and 420 idle/fence events. Keep that terminal split; do not
normalize it into 420 presented frames. These counters are not timed-interval
deltas and do not prove scanout pixel equivalence.

Both Session and the benchmark exit cleanly, with no orphan group members or
pending native cleanup. TTY modes and termios are restored without emergency
recovery. No process from the run remains; END was sent to the peer. Evidence
is bound in `RESULT.json`, `RESULT.txt` and `RESULT.SHA256SUMS`. The benchmark's
power field remains a sensor observation; its scope has not been validated for
whole-machine or battery claims.

The next fixture, `t289-native-dmabuf-profile-01`, holds binaries, geometry,
head rates, client and graphics libraries fixed. It runs an unprofiled control
then a profile, each with 1,800 measured frames (about 30 seconds), to collect
enough CPU samples. Both arms take process/thread counter bookends; only the
profile arm samples. Host `perf_event_paranoid=2` remains unchanged: the event
is `cpu-clock:u`, so samples cover user space, while the counter deltas retain
kernel CPU time separately. The sampling period is 2 ms of CPU time, with DWARF
stacks. The pinned Sophia binary retains its symbol table and unwind sections.

The profiler is attached disabled to the exact desktop PIDs after warmup,
acknowledges readiness, enables before the render gate, and disables after
`MEASURE-END`. It stays in the Session application's process group, with nested
cleanup if profiling fails. Each Session keeps its 240-second bound and
270-second wrapper watchdog; benchmark deadline is shortened to 150 seconds.
The driver stops at the first failure and never replaces a run. U and P must
differ only in the profiling flag; this is no source-change comparison.

Eighteen native/graphics/attribution controls and seven driver controls pass;
both actual profile and Session argument checks pass. A short CPU-only
functional control attached user-only perf to its creating process, collected
samples and stopped through the same acknowledgement channel. Peer read-only
review accepts the attribution ordering and cleanup. At fixture freeze no
native profile had run; `READY.txt` owns that launch. The completed result
follows. The owned-upload candidate stays separate because the native workload
performs no CPU uploads.

## Native DMA-BUF attribution (2026-10-06)

Both arms of `t289-native-dmabuf-profile-01` pass on `cb2cc176b`:
`U/smoke-20261006T234618Z` and `P/smoke-20261006T234711Z`. Each renders 1,800
measured frames at **60.03 fps**, with the same geometry, two heads, client,
libraries and Sophia binary. This is baseline attribution; no candidate code
differs between the arms.

| CPU time over approximately 30 seconds | Unprofiled U | Profiled P |
| --- | ---: | ---: |
| Desktop total | 1.49 s | 1.71 s |
| User space | 1.02 s | 1.13 s |
| Kernel | 0.47 s | 0.58 s |
| Owner thread | 0.86 s | 0.96 s |
| Two renderer workers | 0.37 s | 0.39 s |
| Mesa submission thread | 0.09 s | 0.09 s |

U consumes approximately **5% of one core**, or 0.83 CPU ms per client frame.
The owner accounts for 58% of desktop CPU; renderer workers account for 25%.
The 100 Hz process counters include exited threads, while thread bookends can
only attribute surviving threads: their sums are 1.39 s in U and 1.51 s in P.
Do not assign the missing difference to a particular worker. The helper and
Hagia accrue no whole CPU tick in either measured interval.

P records 2,380 physical pointer events over its whole Session; U records zero.
The explicit first-motion and output-transition records precede the measurement
gate, but later motion within an output is not individually logged. Its ending
before measurement is therefore unproved. **The 0.22-second difference does
not isolate profiler overhead.** Both runs remain valid compatibility and
capture observations; neither is an optimization acceptance.

Both whole-Session reports have 1,920 snapshot captures, promotions and
evictions, six targets, six import-cache imports and 1,916 import hits. CPU
updates and payload bytes remain zero. U completes 1,949 worker compositions;
P completes 1,948. No worker failure, stall, replacement, slot deferral or
direct scanout occurs. The terminal Present split is 1,919 copies plus one
skip, with 1,920 idle/fence events. These totals include setup, warmup and
teardown. Both native cleanup and app-group cleanup finish empty; TTY recovery
passes and END was sent to the peer.

### What the samples establish

The user-space recording contains **521 samples, zero lost**, all within the
Sophia process in the explicit desktop scope. The owner has 326 samples (63%);
the two renderer workers have 142 (27%). Every sampled instruction pointer
resolves, but **298 caller stacks do not unwind**. Preserve their leaf symbols
without inferring callers. Inclusive stack counts overlap.

Owner samples include allocator and collection work, 17 `rustix::ioctl`
leaves, three `drm_ffi::mode::get_property` leaves and a malloc caller chain
through `PropertyValueSet::as_hashmap`. The ioctl leaves do not identify their
requests. Kernel CPU, 32% of U's total, is outside this recording. All-thread
`memmove` has 19 samples (3.6%); this does not justify custom SIMD. Renderer
samples measure CPU submission to Mesa, not GPU execution time.

Keep two limits with later comparisons: this fixture retains full Session
records, so formatting cost need not match ordinary-login diagnostics; and
perf has no recorded clock identity, so its timestamps must not be aligned
directly with Python's monotonic bookends. The enable/disable acknowledgements
and benchmark gates bound the recording. Future timestamp correlation needs
an explicit supported clock choice. No native XLibre or power conclusion
follows from this pair.

### Next candidate: retain primary-plane property handles

The source at the measured revision confirms avoidable repeated metadata work:

1. `persistent_native_scanout/singleton_tick.rs:62` supplies the real group's
   `session.card()` to submission.
2. `drm/native_scanout/prepare.rs:266` discovers connector, CRTC and plane
   property handles after every successful buffer export, including page flips.
3. `native_atomic/properties/lookup.rs:39,48,57` uses
   `get_properties().as_hashmap()`. The DRM crate queries every property's
   metadata and allocates names and hash entries. Conversion then makes another
   vector before discovery selects the fixed handle bundle.

The next slice should retain only a **successfully discovered handle bundle
for the current device and head selection**. Cursor handles already use a
per-head cache initialized empty at owner construction. Audit selection changes
without reconstruction, including mirror and preview paths, before choosing
the key and invalidation points. Identical numeric IDs on different cards must
never share a bundle. Keep storage bounded by the heads; do not cache changing
property values, alter atomic requests or change framebuffer/fence ownership.
Failed discovery must remain retryable.

Required controls are cached/uncached atomic-request equivalence, discovery
only once for unchanged selections, rediscovery on owner/device/selection
change, failure followed by success, and optional-property behavior. Cover VRR,
out-fence, IN_FORMATS and cursor-bearing requests. Then run the isolated gate,
native correctness smoke and balanced unprofiled trials with matched geometry,
work and input activity. The profile motivates this candidate but does not
establish its saving. Temporary per-frame collections remain a later target;
xshmfence loader churn and SIMD stay lower priority.

`RESULT.json`, `RESULT.txt` and `RESULT.SHA256SUMS` bind the raw runs, offline
analysis and exact source hashes. The source audit matches the measured
revision byte for byte. `SAMPLE-SUMMARY.json` distinguishes sampled leaves
from available callers. No new device run, production edit or install was
performed during this analysis. t289 remains open.

### Property-handle candidate ready for native measurement (2026-10-06)

Signed candidate `82ec37c0b` retains one successful handle bundle per physical
head, keyed by native owner, device group, connector, CRTC and plane. Ordinary
singleton and mirror preparation use it; startup and topology preparation keep
raw discovery. The common topology installation method clears it for both the
candidate and rollback, including unchanged object IDs. Device operations,
atomic values, cursor updates, PRIME imports and out-fence handling are forwarded
unchanged. One record at owner release reports cache hits, discoveries, failures
and invalidations; these are adapter counts, not measured kernel ioctl counts.

The clean isolated gate exits zero: 7,181 reported test passes, zero failures,
100 ignored, with clippy and layout passing. Nine focused cache/installation
controls pass; six mutants fail their named assertions. The installation control
calls the shared production head method, not a complete hardware transaction.
Evidence is `t289-kms-property-cache-01`: `SOURCE.json`, `MUTANTS.json`,
`09-gate.json`, `GATE-SUMMARY.json` and `10-nix-build.json`. Earlier compile
failures, the empty-filter test run and the corrected layout refusal are kept.

The Nix binary is `/nix/store/qdah1mk0dsy3dphmrainsq65jcjs5945-sophia-0.1.0/bin/sophia`,
SHA-256 `7ac7a1d0ff1faccbd64264d2e43e45dfe0e884afb3f5dc247aa5c22d5537ff73`.
Its baseline remains the measured `cb2cc176b` binary; baseline production source,
Cargo files, flake and toolchain equal candidate base `fdb5976d9`.

Frozen fixture `t289-native-kms-property-cache-01` keeps the reviewed native
launcher, attribution, graphics checks, profile and wrapper byte-identical to
`native-dmabuf-profile-01/U`. Ten CPU controls and both arms' argument/profile
checks pass. Native source/fixture review and the quiet-window launch remain
pending. Run one candidate smoke, then four unprofiled pairs in order
**BC CB CB BC**, with 1,800 measured frames each. The smoke is excluded from the
comparison. Keep the 240-second Session and 270-second wrapper bounds, and reserve
360 seconds before each launch within the 25-minute series budget. Stop on the
first failure or refused workload; keep it with no replacement.

Acceptance requires every pair to lower desktop CPU, a median paired reduction
of at least 5%, and candidate compositions at least baseline in every pair.
Reject input activity and changed transport, capture, completion or cleanup
work; retain the actual terminal counts. These are whole-Session checks beside
gate-bounded CPU accounting. No native run, saving, publication or install is
claimed for this candidate yet. The owned-upload candidate stays separate.

### First cache smoke passed; comparison parser refused (2026-10-06)

Claude accepted the source and fixture before the native window. Fixture 01's
candidate smoke then passed native compatibility: 1,800 measured frames, clean
TTY recovery and no surviving application processes. Desktop CPU was 1.49
seconds; this is a smoke observation, not a matched saving. The backend emitted
`owner=1 heads=2 hits=1941 discoveries=2 failures=0 invalidations=0` at release.

The comparison driver nevertheless stopped with `candidate cache evidence
missing`: its parser expected the record at column zero, while backend tracing
added a timestamp, colour codes and module prefix. Keep
`qualification-20261007T002912.390995Z/RESULT.json` **FAILED**, with no comparison
trials. The native cache is observed; no performance acceptance follows.

Successor `t289-native-kms-property-cache-02` changes the parser and its controls
only. For the cache record it removes ANSI SGR and the exact known INFO logger
prefix, then applies the existing schema, owner and counter checks. The real
log, plain and colour-free forms agree; wrong logger, quoted record, WARN,
duplicates, bad schema and invalid counters refuse. Twelve controls pass; the
old parser fails the new real-log regression. Both argument/profile checks pass.
`SMOKE01-OFFLINE.json` records the successor interpretation separately, and
`SMOKE01.SHA256SUMS` binds the source evidence. Source, binary, launcher, settings,
workload, bounds and acceptance rules are unchanged. Review and a new quiet ACK
precede the fresh comparison; the first result is never replaced.
