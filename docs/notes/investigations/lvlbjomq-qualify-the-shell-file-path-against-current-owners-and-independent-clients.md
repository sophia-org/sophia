---
id: lvlbjomq
date: 2026-10-14
kind: investigation
status: investigating
tags: [investigation, shell, 9p, qualification]
---
# Qualify the shell file path against current owners and independent clients

## Question

What remains to qualify t252 after source retirement and the later shell-owner
repairs? Reconcile the existing evidence before repeating a desktop campaign.

## Current resumption

The approved extra-pass fixes are implemented. Signed production `d817e6d8a`
services launch relays from owner readiness and deadlines while retaining
per-pass custody and retirement checks. Signed SDK `3edcd28` batches each
fresh writer's walk and open, but waits for both successful replies before
payload. It is pushed upstream and vendored in production `70685cac7`.
The SDK workspace, strict Clippy, signed vendor gate and 24 runtime interoperability tests
pass, including independent C pipeline checks. Relay checks and the isolated
native-feature Session suite also pass. The full workspace run passes with
`sophia-session/native-session` and an empty configuration root. The
[implementation audit](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#extra-pass-attribution-and-approved-fixes-october-16)
records checks and the pre-existing default-feature and formatting limitations.

Relay-13 completed all four arms on comparator `f5d0f0932` and integration
`fd2cb6c`, deliberately retaining SDK `d8558f4`. Offline replay matches journal
metrics, 1,000 frame joins per arm and raw progress records; console recovery
is clean. Files core flushes fell from about 138 to six per panel frame and
489 to nine per bulk frame. Core polling turns fell from 71/245 to about five.
Idle relay skips account for the change; relay expiry and stale handoff counts
are zero. Session passes remain roughly 33/120, as expected before SDK batching.
Both IPC arms now have zero steady core flushes. These are progress counts,
not CPU or allocation acceptance. The [relay-only results](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#relay-only-native-results-october-16)
record bounds and limits.

Batching-14 completed all four arms on comparator `bf19952dd`, integration
`fd2cb6c` and SDK `3edcd28`. Independent replay matches all 1,000 frame joins,
journal metrics and raw counters in each arm; console recovery is clean.
Session passes fell to 27.426–27.469 per panel frame and 109.977–110.203 per
bulk frame, about six and ten fewer respectively. Reply counts remain about
53/152. This supports the intended reduction in setup waits without removing
protocol operations; it does not establish a CPU or allocation win.

Cost-15 completed all eight arms on the same comparator and integration.
Offline replay matches costs, journal metrics and 1,000 joined frames per arm;
console recovery is clean. Against cost-11, files CPU/MiB fell about 12.6% for
panel and 6.3% for bulk; allocations/frame fell about 10.1% and 6.0%.
The current paired IPC gaps remain about 31%/22% CPU and 83%/80% allocations,
so the +10% gate still fails. This pilot measures both fixes together and
cannot attribute their individual cost savings. See the
[cost results](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#cost-15-results-and-fresh-profiling-october-16).

All relay-13, batching-14 and cost-15 captures are occupied; do not rerun them.
The quiet hold is released. Claude reports only light local edits, zk/Git
inspection and small local copies during cost-15; exact timestamps were not
supplied. The report is retained with the evidence.

Perf-16 stopped after the first panel arm. All 1,000 uploads and cost replay
complete, and console recovery is clean, but the presentation join refuses
`completion lacks exact heads or kernel timestamp`. Native records show
out-fence retirement and observation-time fallback throughout, not accepted
kernel page-flip timestamps. The launcher stopped before the other three
arms. Preserve this failed join and its raw profile; do not call the campaign
complete. The partial profile selects host PIDs 7616/7701 and contains 1,230
user-space samples within the workload interval. It has no paired IPC profile.
The cause of choosing out-fence retirement throughout is not established.

Perf-17 captured both panel arms successfully: journal, cost and all 1,000
presentation joins replay exactly, with clean recovery. The files bulk arm
then failed the same timestamp check; IPC bulk never ran. Its first 436
completion records use kernel timestamps, then frame 438 switches to
observation fallback for all remaining 567 records. Source inspection explains
the persistence: synthesizing an out-fence callback permanently sets
`OutFenceAuthoritative`, which discards later page-flip callbacks. The initial
trigger is not established; do not attribute it to perf without evidence.

The valid panel pair has 1,184 files and 723 IPC user-space samples, filtered
to host Session/peer PIDs and the measured upload interval. Exclusive sampled
differences include copy leaves (+57 ms), custody checks (+47 ms), vDSO clock
reads (+41 ms), and recognizable allocator leaves (+21 ms). These are sampled
periods across 1,000 frames, not exact runtimes or additive inclusive stacks.
Repeated component service remains a strong lead. The
[profile audit](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#perf-17-panel-attribution-and-bulk-refusal-october-16)
records limits and evidence.

The bounded repair is implemented in signed production `93c542e17` and
comparator `075fdfa21`. Before the first signaled-fence fallback, singleton and
mirror retirement recheck the affected card and prefer a newly queued event.
Ordinary event retirement and heads already using fence authority add no read.
Read/routing failures propagate; fallback, serial safety and exact timestamp
requirements remain. Production backend library tests pass (353, one ignored),
DRM-event tests pass (316, one ignored), strict library Clippy passes, and all
eight comparator head-completion tests pass. The fixtures prove the controlled
arrival ordering; hardware benefit is not yet established.

Perf-18 completed all four arms on repaired comparator `075fdfa21` and
integration `fd2cb6c`. Offline replay matches costs, journal metrics and all
1,000 exact presentation joins per arm. All 1,003 native completion records
per arm use kernel timestamps; console recovery is clean. This demonstrates
a passing pilot, not that every future race is eliminated or that the recheck
was exercised. The [profile results](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#perf-18-complete-paired-profiles-october-16)
record selections, hashes and limits.

Bulk exclusive differences concentrate in syscall wrappers, clock reads and
custody checks; recognizable allocator leaves are not the dominant difference.
Nearest visible callers place +252.8 ms of bulk syscall-wrapper sample periods
under SDK Pipeline::poll (+38.1 ms panel). This is user-space sampling, not
kernel duration or syscall counts. The isolated SDK syscall investigation now records three normal and three
short-write/retry uploads in `t331-sdk-io-19`. Normal uploads consistently make
14 useful reads and 34 WouldBlock reads, but most repeats follow new sends;
this does not justify skipping socket reads. The same-thread scripted peer
cannot establish native readiness frequency. Temporary markers are restored;
SDK source is clean. See the audit for exact counts and limits.

Next bounded candidate: move the submission-retry clock read behind RetryWait,
same-pass and event-progress checks. Source currently reads time on every drive
round even when no retry needs it. Preserve deadline behavior and existing
scripted retry tests; no measured savings or SDK behavior change yet. Preserve
EOF, backpressure, fairness, upload barriers and every custody check. Copy
callers remain mostly unresolved.

The quiet hold is released; mini Claude reports no crunch work. All perf-18
captures are occupied: do not rerun `t252-check`. No new attended run is needed
for the SDK investigation. The installed desktop is unchanged and t331 remains
open pending the unchanged cost and native acceptance gates.

Progress-12 established 33/120 Session passes per files panel/bulk frame,
versus roughly 8/45 for IPC. Source attribution identifies repeated idle
launch-relay work and sequential SDK setup as avoidable costs, while retaining
the authority checks and upload barriers. The latest cost-15 pilot still fails
the +10% gate. t331 remains open until the full paired gate passes; diagnostic
counts cannot establish CPU or allocation savings. t332 stays an investigation.
The installed desktop is unchanged. Normative scope and acceptance remain in
the [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md).

## Relay implementation and preceding qualification checkpoints

The owner approved readiness-driven launch relays and batched SDK writer setup
on October 16. The relay implementation now consumes the actual owner wait,
including short frame-service waits, or one aggregate nonblocking sample.
Idle relays skip transport turns and flushes; custody, expiry, revocation and
retirement still run each pass. The SDK slice and fresh native pilots follow;
neither change has a native performance verdict yet. The approved scope is in
the [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#approved-relay-and-writer-setup-slice--october-16).

Deterministic relay validation passes the core and Factotum suites, 30 Session
authentication tests (five child-entry helpers ignored), and 1,048 Session
library tests (36 ignored) with an empty configuration root. The first broad
run inherited live namespace recipes and failed 16 configuration fixtures;
the isolated rerun passes all of them. All-target Clippy passes for the three
affected crates. New checks cover 10,000 idle observations, late input,
activation, revocation, foreign observations, HUP, asynchronous Factotum
completion, silent expiry and the 64-slot retirement boundary.

Progress-12 completed all four arms on comparator `47ac3f2f7` and integration
`59bee7f`. Offline replay matches journal metrics, all 1,000 frame joins per arm
and every raw progress record; console/keyboard recovery is clean. Files takes
33.08–33.16 Session passes per panel frame versus IPC's 8.03–8.20; bulk takes
120.07–120.12 versus 45.02–45.51. Handoff is used about 32/119 times per frame
with zero stale fallbacks in those intervals. This identifies service
amplification, not yet redundant work or a safe authority-check removal.

The Plan 9 audit records counter bounds and limits. Next, attribute the roughly
53/152 encoded core replies per files panel/bulk frame to request kinds and
client state transitions, distinguishing required round trips from unchanged
service. Peer wait counters are API calls, not proven sleeps. Core totals cover
all process servers. No CPU/allocation acceptance verdict follows. All progress-12
capture directories are occupied: do not rerun `t252-check`. The quiet hold is
released; Claude reports only initial status reads and confirmation.

Cost-11 completed all eight arms on production-source candidate `deae479d6`,
comparator `8bbef6e00` and integration `4eb52fb`. Independent offline replay
matches the stored costs, journal metrics and 1,000 joined frames in every arm;
console and keyboard recovery are clean. Files allocations fell about 0.4% on
both workloads, but CPU changed +1.8% on panel and -1.0% on bulk versus cost-10.
This is a small allocation improvement, not an established CPU win. The +10%
gate still fails and t331 remains open. The scratch candidate remains
unaccepted for the full native gate; do not install it on this evidence.

Evidence is under `t252-authenticated-20261015/t331-native-cost-11`, including
`offline-replay/summary.json`. All capture directories are now occupied:
do not reuse cost-11 captures. Its quiet hold is released; Claude
reported no crunch activity after the initial 13:06:38Z confirmation and Muse
status read. Reanalysis of perf-08 finds only about 22/10 ms of panel/bulk
sampled differences in recognizable allocator leaves. Clock reads, polling and
repeated owner service are stronger leads, though these profiles predate the
latest changes. The Plan 9 audit records exclusive leaf groups and caller
limits. Next, count current service turns versus useful progress per frame,
including readiness handoff versus fallback, before another transport API change.

Both optimized builds remain in Cargo slots. The initial CPU config validation
refused generated file permissions (`UnsafeMode`); private modes corrected it,
and all final checks pass. The failed validation remains in the evidence folder.
The previous cost-10 captures are preserved. This pilot measures exact sizing
and scratch reuse together against cost-10; it cannot separate their effects.

The owner requested renewed comparison with maintained Plan 9 implementations.
The [pinned rio/Go p9/diod audit](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md)
finds a concrete completed-journal-read allocation path and the remaining SDK
upload copy. Exact sizing of the completed-read temporary now removes the two
reallocations per four-record read in the isolated fixture. Unbuffered completed
reads still allocate once; warmed reply encoding and empty tail reads allocate
nothing. Huge-count requests allocate only the available requested range. The
full 9P suite and strict Clippy pass in production and comparator. Comparator
`840a6824f` carries the same fix and fixture. Follow-up copy accounting confirms
two journal payload copies and one SDK staging copy. A second same-size SDK
upload allocates nothing; all six isolated uploads use one socket send each.
These establish byte traffic, not CPU attribution. The owner-approved journal
scratch experiment is implemented through the authenticated shell export;
comparator `8bbef6e00` carries the same implementation and regression tests.
Warmed journal reads now allocate nothing across all five isolated cases;
both payload copies remain. Scratch retains at most one negotiated read payload
per opted-in connection and is released on reset/close. New core tests cover
queued output independence, pending/error/oversized outcomes and revocation;
106 runtime library tests pass, with one ignored, in both checkouts. Strict
core all-target and runtime library Clippy pass in both. Cost-11 results above
supersede the planned pilot; the installed desktop is unchanged.

The approved readiness handoff is implemented in signed production `c2138f95e`
and comparator `377b0413d`, both pushed. Cost-10 does not establish a useful win:
panel CPU/MiB is about 0.9% below cost-07, bulk about 2% above it; allocations
are essentially unchanged. The candidate remains unaccepted, and t331 is open.
Session preserves the actual owner wait for the next connected shell input turn,
with slot/grant and server/connection/generation checks. Stale observations poll
again. Seven core handoff tests, the full 9P suite, 24 owner-wake tests, 14 component
connection tests and 40 focused shell tests pass; strict Clippy passes for the
three changed libraries with native Session enabled. Pending-read tests now
exercise cancellation, revocation and backpressure through observed turns.
All eight `t331-native-cost-10` arms completed. Independent replay matches the
captured costs, journal metrics and 1,000 joined frames in every arm; console and
keyboard recovery are clean. The quiet hold is released and Claude confirmed no
activity after the initial coordination. Capture directories are occupied: do
not rerun `t252-check`. Actual handoff use versus fallback frequency remains
unmeasured; do not infer poll savings from correctness tests.
The approved retention criterion still requires repeatable benefit; this pilot
does not satisfy it. No desktop installation or restart occurred.
Repository-wide formatting reports pre-existing differences in two native scanout
files; changed files are formatted. The full evidence packet is at
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-native-cost-10/`.
See the [approved scope](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#approved-readiness-handoff-october-16).

Resume t331 before t252. The objective is to beat IPC on CPU/MiB and allocations
per frame, retaining +10% as the acceptance ceiling. The owner accepted
[Session-plus-client accounting](../../sophia-shell-files.md#cost-accounting-scope-accepted-october-15-2026)
and the [allocation and copying slice](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#approved-allocation-and-copying-slice-october-16).
Upload ordering, resource limits and the installed desktop remain unchanged.

The preceding CPU pilot, `cost-07`, failed the cost gate: files cost
about 47% more than IPC for the panel and 28% more for panel-plus-4-MiB uploads.
Earlier fixes cut files allocations by about 12–14% against `cost-02`. All twelve
`cost-04`/`profile-05` cases are accounted for. The four finer `profile-06` arms
also join 1,000 frames each, match independent replay and restore console and
keyboard state. They point to repeated lifecycle and authority service, not
exclusive allocation stacks. See [finer profile results](#t331-finer-profile-results).

Production `7c8fd76c4` borrows head observations and stores frame-service sets
inline. The one-output reducer fixture falls from 7,000 allocations per 1,000
turns to zero; revised fixtures also allocate nothing at two and sixteen outputs.
Borrowed head selection measures zero versus 1,000 owned-list allocations in
1,000 visits. These are isolated results, not native per-frame savings.

SDK `bcf840a` advances send/upload cursors without shifting ordinary tails;
`25b63ff` moves outbox payloads without releasing ticket or budget charges.
Signed `d8558f4` also synchronizes the already-accepted cost-accounting document.
Production `a9a92746a` imports that SDK; comparator `a2ecde0ef` carries the same
native fixes and SDK onto both wires. SDK workspace tests, focused strict
Clippy, the signed vendor gate, 112 mirror tests, frame-service checks and
independent C/Go export checks pass. See [implementation evidence](#t331-bounded-storage-and-copying-results).

All eight CPU-only/allocation arms completed in
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-native-cost-07/`,
on comparator `a2ecde0ef` and integration `4eb52fb`. Independent replay joins
1,000 frames per arm and verifies clean console/keyboard recovery. Files
allocations fell about 38% for the panel and 43% for bulk against `cost-04`;
CPU/MiB fell only about 1.3% and 1.9%. The shared changes also help IPC.
See [cost-07 results](#t331-storage-pilot-results). The quiet hold is released.
All four `t331-native-perf-08` profiles completed on the exact cost-07 CPU-only
binaries. Replay joins 1,000 frames each with clean recovery. No samples were
reported lost. The strongest differential lead is extra owner/transport service,
including native service and custody checks. Pixel generation and validation
are large shared bulk costs, not an explanation unique to 9P. See
[CPU profile results](#t331-cpu-profile-results) for counts and unwind limits.
Caller inspection separates idle polling, per-operation checks and delivery
guards; none is proved redundant. The counting fixture confirms 100 unrelated
replies cause 100 pending-read retries, but also proves that request-driven
publication can require a retry without a wake. A generic retry skip is unsafe.
The subsequent shell audit rejects implementing a change token for now: keeping
authority checks preserves the custody poll, while the pending journal read is
already an allocation-free tail comparison. See [change-token audit](#t331-change-token-audit).
Return to CPU attribution of owner service and system calls; do not infer a
large saving from the fixture's read-call count.
The owner has approved temporary kernel profiling (paranoid 2 to 1, then back
to 2). `t252-check` now selects four fresh perf-09 runs, prompts for sudo on
tty4, and restores the setting on normal exit, error or catchable interruption.
All four perf-09 runs now completed with 1,000 joined frames and clean recovery;
independent cost/join replay matches. Perf attributes confirm kernel sampling,
and the host setting is verified restored to 2. The quiet hold is released.
Kernel/user attribution is recorded in [perf-09 results](#t331-kernel-profile-results):
roughly two-thirds of excess sampled CPU is user-space work and one-third is
kernel work. Polling is the largest identified kernel difference; custody is
only part of it. Prioritize avoiding redundant transport readiness polls, while
preserving custody checks, before considering upload concurrency. This remains
a source-inspection lead, not an implemented saving. Perf-09 reuses the validated
perf-08 plans and binaries. Its output directories are now occupied; do not
rerun those plans. The owner reported a silent exit 1 after captures existed;
the helper's overwrite guards explain such a refusal, though the original
terminal exit was not captured. Installed guards now print an explanation.
The perf-08 directories remain occupied. These profiles do not establish B9 or
authorize concurrent uploads.

[t332](gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md#t332--investigate-bounded-upload-concurrency)
is a candidate concurrency investigation, not an implementation authorization
or a new t252 prerequisite. The earlier corrupted `cost-01` remains invalid;
completed evidence directories must not be reused. t252 still needs native
lifetime/input acceptance on both desktop selections, full B9 and attended
release rollback.

## Earlier qualification checkpoints

t252 remains open for native lifetime/input acceptance on both desktop
selections, the declared B9 performance campaign and attended release rollback.
The generic C/Go export, descriptor, content and Session checks pass; E1's old
independent descriptor gap is resolved. The t330 notification repair is included
in Sophia `58a481ebc`; the broader t032 backlog remains outside this task.

External integration `d51c966` pins that Sophia revision and passes workspace checks. Actual
Provlita, Lom and Bemenu authenticated launches pass twice each. The installed
Narthex predates authentication; signed Narthex `11eb0332` passes the
fallback authentication/recovery check. External integration `77c8e96` now
packages that Narthex with both the two-component baseline and three-component
candidate. Their configuration checks, checksums and isolated install/rollback
pass; the packaged dock also authenticates from its private installed path.
Integration `c3e49db` records those checks and supports store artifacts in the
supplied-export gate. Integration `62b9829` now proves actual dock GPU uploads
and retained resource custody across replacement on the explicit development
GPU. Integration `86de1ed` also proves real bar/menu progress while a stopped
dock reaches catalog backpressure, then through replacement with retained dock
pixels. Those completion receipts are supplied, not physical presentation.
Integration `434d302` separately proves two-component baseline content and
menu replacement using the baseline package and two-role envelopes.
The isolated IPC comparator now builds and passes paired resource acceptance,
pixel identity, retirement and stale-generation checks at 8 bytes, 180 KiB and
4 MiB. Its content owners are unchanged from `58a481ebc`; the restored adapter
and test are signed `6df9b559f` on the comparison branch. Its successor
`c466ff065` also passes equal candidate completion, renderer-failure and
held-render-lease checks, plus an exact supplied action/acknowledgement exchange.
This is partial equivalence evidence, not B9 timing or physical input.
Successor `5a880fb78` proves two neighbors continue while a third reader reaches
backpressure, and through its revocation and replacement. Full reservation
reclamation waits for the retained pixels. Queue depths differ between wires;
the dated evidence below retains both counts. Next build the measured harness
and freeze its complete method, including CPU/allocation accounting and actual
presentation, before collecting B9 acceptance samples.
Signed comparator `2aebdcdfd` adds an ignored diagnostic upload probe for wall time, process CPU
and optional allocation counts. Its short instrumentation checks pass in both
counter modes; this has not frozen the full method or produced a B9 verdict.
The native binding and kernel-completion records already exist in that Sophia
candidate. External integration `91d5370` adds `xtask shell-timing join`, with
five positive/negative tests and strict Clippy. It joins every offered candidate
to its first exact native completion and rejects incomplete head coverage or
fallback clocks. Comparator `b2bfb9678` now supplies a paced child peer with
complete offer/terminal journals; integration `fbe20dc` validates those journals
before joining. Both supplied-owner wire runs and refusal checks pass. The peer
also passes two actual authenticated launches and revocations. Comparator
`16d1cb870` adds a guarded native Session selector for the IPC baseline; its
protected launch/replacement and unarmed-refusal checks pass. Current file
launches retain authentication. Native scanout has not run for this peer.
External integration `0c8c1a0` adds the bounded attended runner, with artifact
hashes, signed source checks, console guards, complete capture and native joins.
Its device-hidden checks pass; no physical run plan has been executed.
Comparator `e66d612d1` adds bounded Session/peer process counters and reuses two
resource IDs with fresh generations. External integration `60dd30f` verifies
conservative CPU/allocation bounds and separates instrumented runs from latency
runs. The focused checks pass. A supplied-owner debug diagnostic shows a marked
9P allocation excess; this is not native B9 evidence. See
[frame counters](#frame-counters-october-15) for scope and exact results.
Next prepare the campaign artifacts and exact hardware/output selection, and
freeze the paired method, including accounting scope and aggregation rules.
The first optimized native diagnostic is now prepared under
`native-calibration-01`: two 60-second runs, 64 offers per wire, at 1080p/60 Hz
on DP-1. Both exact argument vectors pass device-hidden validation. Integration
`643dec3` binds the required Factotum executables. The attended command is
`t252-check` from tty4 after leaving the live desktop. Calibration 01 failed
before peer admission because the PAM helper's Nix-store ancestor was
group-writable. Console/keyboard recovery verified; IPC did not start.
Integration `4ec2904` checks helper custody before preparation and launch.
Calibration 02 now selects the byte-identical root-protected installed helper;
both new plans and device-hidden argument checks pass. The same short command
selected calibration 02. That run authenticated and reached native 1080p/60,
but Session revoked the peer while polling an unnegotiated optional indicator
feature. Regression `content_peer_without_optional_indicators_survives_indicator_service`
reproduces the error without hardware. Production `f8adbcdb4` skips that service
when the capability is absent; comparator `bef52ef0f` carries the same fix.
Six real-client action/indicator tests and strict Session Clippy pass. No peer
frame was presented in calibration 02; console and keyboard recovery verified.
Calibration 03 is now prepared with the rebuilt comparator; both exact argument
vectors pass device-hidden validation. `t252-check` selects these fresh plans.
Calibration 03 then completed all 64 file-wire offers and Presented receipts,
with clean wrapper recovery, but recorded zero native completion witnesses.
The strict join refused the run and IPC stayed stopped. Production `08be65293`
and comparator `136cbcb41` correct the logger's singleton custody lookup;
the verifier's exact kernel-completion requirement remains unchanged.
Calibration 04 passed both native diagnostic arms: 64 offers per wire, all
joined to exact kernel completion, no failed journal events and verified
console/keyboard restoration. Sources are comparator `136cbcb41` and external
integration `4ec2904`. Independent replay of both collectors and joins passes.
This is a 60 Hz, 64-frame diagnostic without warmup exclusion, not B9 acceptance.
Do not rerun `t252-check`: its run directories now contain retained evidence.
Comparator `df3a08f8d` now records upload attempts, acceptance times and exact
RGBA byte counts; integration `fd1e0a1` validates and exports them alongside
demand-to-permit times. Both supplied-owner wires pass 64-frame replay, and
calibration 04's old journals remain readable. This instrumentation has not yet
run natively. See [upload journals](#upload-journals-october-15).
Comparator `ba871b742` and integration `571ba0a` add the separate 4 MiB
transfer probe. Both wires pass 64 accepted/released probes followed by bar
frames in the device-hidden supplied-owner test. This does not claim those
4 MiB resources were displayed. See [bulk probes](#bulk-upload-probes-october-15).
Integration `4768fbf` now exports conservative CPU/MiB and allocations/frame
bounds from complete upload journals. The four instrumented supplied-owner
arms pass evidence validation; Rust allocations remain substantially higher on
9P in this diagnostic. The [accounting proposal](#accounting-method-proposal-october-15)
is pending owner decision and does not relax the +10% limit.
At niltempus's request, [t331](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#t331--reduce-shell-allocation-overhead)
now owns attribution and reduction of the measured allocation excess before
t252. This does not promote broader t289 work or approve the accounting scope.
Production `34edcd707` and comparator `fd9423597` remove two per-turn readiness
allocations for a single adopted 9P socket. The paired diagnostic's bulk file
allocation bound falls from 21,498–21,538 to 8,953–8,964 per frame; small-bar
costs remain high. See [the first t331 reduction](#t331-single-connection-readiness).
SDK `894dbca` removes temporary reply copies and reuses bounded drain storage;
production `64524a300` and comparator `651e4a969` pin it. Small-bar file
allocations fall further to 538–553/frame, bulk to 8,542–8,552. See
[SDK reply storage](#t331-sdk-reply-storage). Comparator `05cbdd476` now measures
the actual 1920×24 bar: files 839–850 allocations/frame versus IPC 623–634.
SDK `af21f0a` uses stack request headers and copies writes directly into the
bounded output buffer. Production `368e07c3f` and comparator `95dc246d2` pin it.
Workspace tests, strict Clippy, signed snapshot validation and 13 production
file-path tests pass. The panel diagnostic now measures 709–721 file allocations
per frame versus IPC's 622–633, still above the +10% target. SDK `f638e2f`
then reserves walk-request storage once; production `30f8f784f` and comparator
`f055afef7` pin it. Panel allocations are now 678–689 versus IPC's 613–625;
the conservative bounds do not prove +10% parity, and panel CPU remains above
the budget. See [walk encoding](#t331-walk-encoding). Settle the accounting
proposal before preparing acceptance measurements; debug optimization results
alone cannot close t331 or t252.
External integration `b199ac0` now summarizes complete native latency evidence
with nearest-rank p50/p95/p99/max, retaining all samples and over-bound uploads.
Its tests and calibration-04 replay pass; it is not a paired campaign verdict.
See [latency summaries](#native-latency-summaries). The accounting-scope question
has been put to the owner and remains unanswered; do not infer approval from
automatic goal continuation.
An optimized supplied-owner diagnostic now establishes a more relevant baseline:
panel file allocations are 420–456/frame versus IPC's 348–380, and bulk file
CPU exceeds IPC +10%. Debug bulk CPU had shown the opposite ordering. Do not
use debug timings to choose a performance verdict. See
[optimized diagnostic](#t331-optimized-diagnostic). Native B9 remains unrun.
See [panel baseline](#t331-panel-baseline-and-pending-request-encoding).
Settle accounting scope and freeze the full B9 method,
then collect the declared campaign and native lifetime/input evidence for both
desktop selections. See [calibration 04](#native-calibration-04-both-wires-joined).
See [comparator preparation](#comparator-preparation-october-15).
Next prove native lifetime/input for both selections, freeze and run B9,
then complete attended acceptance. The live desktop is
unchanged. See [the packaged pair](#packaged-pair-october-15) for exact identities.

## Packaged pair, October 15

External integration `77c8e96` builds the three-component candidate
`niltempus-ad35e88b86b247648c71` and two-component baseline
`niltempus-339765fe1cc1dd8166f5`. Both select Sophia `58a481ebc` and Narthex
`11eb0332`; the dock selects Provlita `da4a15ad`. The integration's
`docs/provlita-files.md#packaged-pair-october-15` records their exact store paths.

Packaging found two errors before rollout: the Rust source filter omitted the
embedded font, and the dock configuration path requested an extra mount inside
the read-only Nix store. The filter now includes the font, and the profile names
the configuration included in the installed release. No protection rule changed.

Both builds pass profile validation. Complete checksums pass after installation
into a private Bubblewrap `/opt`. Baseline installation, dock installation,
rollback and return to the dock all pass. The installer's existing corruption,
locking and pruning tests pass separately. Packaged Provlita authenticates at two
fresh epochs from its private installed path and configuration, then refuses
the absent GPU grant as required. Its supplied-export test also passes twice;
packaged Narthex passes authenticated revocation/recovery. The store-aware
external fixture and evidence are signed in integration `c3e49db`.

Logs are in `~/.local/state/sophia/development-evidence/t252-authenticated-20261015/`:
`desktop-pair-final-build.log`, `release-pair-rollback.log`,
`desktop-tool-check.log`, `provlita-packaged-export-final.log`,
`provlita-isolated-installed-auth.log` and `narthex-packaged-auth.log`.
Failed packaging and launch attempts remain alongside them. No live installation
or GPU test ran. Isolated release switching is not attended rollback; actual
content, native lifetime/input, three-client isolation, B9 and physical
acceptance remain open.

## Dock GPU content, October 15

External integration `62b9829` adds the ignored `provlita_gpu` gate using
Sophia's public protected GPU launch owner at the existing `58a481ebc` pin.
The packaged Provlita from the candidate above passes on the explicitly selected
development node `/dev/dri/renderD129`, PCI `0000:16:00.0`. Its renderer confirms
Vulkan/RADV on device 226:129, with that node alone visible. No KMS node or
display socket is exposed, and the live desktop excludes this GPU.

At epochs 71 and 72 the actual dock allocates, rasterizes, uploads and submits
catalog-bound pixels with an authorized terminal target. The replacement
uploads while the revoked dock's pixels remain leased and readable. The old
completion owner rejects the replacement's live grant. Each unpresented
submission receives explicit failed-render completion; its bytes stay until
the consumer releases the lease. All epoch and byte accounting then settles.
The first run omitted that completion and correctly failed the reclamation
assertion. The test now proves both completion and lease-release obligations.

Wrong PCI identity and an unarmed invocation fail before client GPU admission.
Device-hidden workspace tests, strict Clippy, provisioning and pin checks pass.
The pin fixture was updated for the newly used public Session crate; no Sophia
production source changed. Exact commands, binary/configuration hashes and
limits are in external `docs/provlita-files.md#render-node-content-and-revocation`.
Logs in the existing October 15 directory include `dock-gpu-final.log`,
`dock-gpu-wrong-device.log`, `dock-gpu-unarmed-negative.log`,
`dock-gpu-workspace-final.log` and `dock-gpu-clippy-final.log`.

Geometry and catalog decisions are supplied. Admission uses real supervisor
evidence, while authenticated launch remains separately proven above. This test
issues no Presented event and proves neither native frame retirement nor
physical input. Three-client coexistence, B9 and attended acceptance remain open.

## Three-client isolation, October 15

External integration `86de1ed` runs the actual packaged Lom, Bemenu and Provlita
against three production exports sharing the same 64 MiB content registry.
It uses Session's nominal three-role envelopes and public reconnect-limit
selector. All three upload content. The dock is then stopped with SIGSTOP,
verified through `/proc`, and its catalog publications reach bounded
backpressure after 192 accepted updates. Lom and Bemenu continue producing
candidates. The dock is revoked and replaced while its old pixels remain held;
both neighbors keep their original grants and continue, and the new dock
uploads. Releasing the old pixels reclaims that epoch; final accounting is
quiescent.

The first fixture used the registry's default capacity of two and correctly
refused a third admission. The fix explicitly selects Session's capacity of
three without increasing its byte budget. The final check passes on development
GPU PCI `0000:16:00.0`, with only render node 226:129 exposed. Unarmed and
wrong-device invocations fail before launch; ordinary tests leave this gate
ignored. Formatting and strict Clippy pass. Logs are `trio-backpressure.log`,
`trio-wrong-device.log`, `trio-unarmed.log`, `trio-ordinary.log` and
`trio-clippy.log` in the existing October 15 evidence directory. Exact artifacts
and invocation are in external `docs/provlita-files.md#three-client-progress-under-dock-backpressure`.

This proves actual-client content progress, bounded slow-reader isolation and
retained-byte reclamation across replacement. Admission, geometry, launcher
typing and completion receipts are supplied. Client `presented` log lines in
this gate do not prove physical presentation. Native lifetime/input, the
separate baseline, B9 and attended acceptance remain required.

## Two-component baseline, October 15

External integration `434d302` adds a separate baseline test using Lom and Bemenu
from release `niltempus-339765fe1cc1dd8166f5`. It selects Session's two-role
capacity and byte envelopes. Both clients upload; a supplied text edit produces
another launcher candidate. Lom retains its grant and continues while the
launcher is revoked and replaced with its old pixels still held. The new menu
uploads, old pixels remain readable until release, and final accounting is
quiescent. The three-component test passes again after sharing the inventory
selection helper. Strict Clippy and formatting pass; nested scratch-path
expressions were simplified after rustfmt produced an unstable layout.

`baseline-run.log`, `baseline-clippy.log` and `trio-after-baseline.log` in the
existing October 15 evidence directory record the checks. The baseline log binds
the exact package paths and hashes. Completion and input are supplied, so this
does not close physical baseline acceptance or provide timing samples.

## Selected clients and immediate blockers (October 15)

Sophia source `4ea088abd` was reviewed after t254 acceptance. The installed
release remains `niltempus-b7f017d6e4cdb2056faa`, built from Sophia `ed0caa4f2`
and desktop integration `a6eb56d`. Its lockfile selects:

| Client | Selected source | Remaining t252 evidence |
| --- | --- | --- |
| Lom | `dc65f561f8c9522f6a09d3e5f386d32226228bfd` | Exact native lifecycle/input and B9 coverage, including coexistence with the dock. |
| Bemenu | `6e2efd96e2256a138ed9928387e83d3c30585937` | Exact presented focus, activation and retirement on the selected three-component release. |
| Narthex | Installed `56d513fa` fails authentication; candidate `11eb0332859ed0e0b9244dd3ddafcea70c2b1b6e` passes it. | Package the compatible fallback and verify selected-release behavior. |
| Provlita | Not in the installed lockfile; prepared source `da4a15adb9a6555441653a9dd9ec0e4d69f3d6af` | Authenticated launch passes. GPU content, native presentation/activation/retirement, packaging and attended acceptance remain. |

Provlita's signed clean source builds offline with its locked dependencies.
Its version and example configuration checks pass. The release binary is
`~/.cache/cargo-slots/provlita/1/release/provlita`, SHA-256
`1bfa663d7583014d049ad5695e8ea869df33d964dbc90a60672e7feb9568be6b`.
These checks do not contact Session or prove GPU execution. Its current code
uses 9P and authenticated protected startup; older integration prose saying it
retains the legacy wire is stale.

The external `niltempus` dock profile omitted an explicit transport for
Provlita while selecting 9P for its two neighbours. The profile and its parser
test now require 9P for all three. Both dock tests pass with devices, display
sockets and network hidden, using the existing provisioned Cargo cache. The
first offline attempt used the default cache and could not find the pinned
Sophia dependency; no tests ran in that attempt. The integration test pin is
`316d969507cdeb6ef061ea2d9645758113c53db6`, distinct from the installed release;
this parser check must not be presented as current native qualification.

### Provlita protected export join (October 15)

Signed external integration `78e00bf` adds the device-free executable gate and
its reproduction guide, `docs/provlita-files.md`. It uses the production
supervisor, Bubblewrap protection and shell file export from pinned Sophia
`316d969507cdeb6ef061ea2d9645758113c53db6`, with Provlita `da4a15ad` and the
binary recorded above. Its configuration SHA-256 is
`c46db4786c9021a9a65d3537b53c8761c65226ab089aacab47d6e46170e192ef`.
The host supplies admission using the actual protected peer PID; it does not
provision a Session factotum relay.

Both fresh connection/content-grant generations, 61 and 62, negotiate revision
8 over 9P2000.L. Each executable then refuses the missing GPU grant, as required,
and the owner reclaims every content reservation. Changing the host profile
from persistent catalog to native launcher fails negotiation; restoring it
passes. The first fixture attempt reused content-grant generation 1 and was
correctly rejected as stale on the second launch. The final fixture advances
both generations. Strict Clippy, formatting, pin audit and provisioning checks
pass. No Sophia product code changed for this gate.

Logs are under
`~/.local/state/sophia/development-evidence/t252-provlita-20261015/`:
`join.log` preserves the fixture failure; `join-final.log` and `restored.log`
record passes; `wrong-profile-negative.log` records the negotiation refusal.
The test wrapper hides display devices and sockets, creates matching PID/proc
namespaces and denies networking. No binary was copied into evidence.

This closes the supplied-admission executable negotiation check only. It does
not prove authenticated Session launch, GPU rendering, content upload,
Presented/input/retirement, coexistence or B9. The tested Sophia pin predates
t330 and differs from the installed release. Update the external pair and prove
authenticated startup before claiming those later stages.

### Authenticated clients and current pair (October 15)

Signed external integration `d51c966` pins Sophia `58a481ebc756b50fe97a890b968a6c9b70863496`,
refreshes the C SDK manifest and sixteen contract digests, and passes the pin
audit, provisioning, device-hidden workspace tests and strict workspace Clippy.
The first workspace run caught a stale expected count in the binding-removal
negative control; that test now expects fifteen remaining bindings when one of
sixteen is removed. The refusal assertion remains intact. Provlita's protected
supplied-admission gate passes again on the updated pin.

Sophia's existing generic external-client fixtures exercised the production
Factotum owner and protected launch coordinator on that same server revision:

| Client and exact selection | Result at two fresh epochs | Limit |
| --- | --- | --- |
| Provlita `da4a15ad`, binary recorded above | Authenticated dock negotiation, missing-GPU refusal, process and launch-custody retirement | No GPU/content execution |
| Installed Lom, binary SHA-256 `6d0af594cf3170ca4d2a35afdef09e0ff9d768608c561157f5b2dc703bb21e45` | Authenticated bar negotiation, missing-GPU refusal, retirement | No GPU/content execution |
| Installed Bemenu, binary SHA-256 `84a6bfc1dc0c81d2c5a3b2ebe3dc98d91e1050b77a138d7ea3cbcd95dedf1505` | Authenticated launcher negotiation, liveness until explicit revocation, retirement and replacement | No opened launcher or activation |
| Narthex `11eb0332859ed0e0b9244dd3ddafcea70c2b1b6e`, binary SHA-256 `699505779b729084de04f25fc8752b417cd40671048258cd9b35a5d89d43e536` | Descriptor revision-8 negotiation, liveness, revocation and fresh-epoch recovery | No descriptor presentation or physical fallback |

Provisioning Provlita with the bar role fails with authentication refusal 13
before shell negotiation. The packaged Narthex `56d513fa` repeatedly fails
authentication because it predates that implementation. Its signed successor
was built from a clean tree with `nim c -d:release`. An initial outer sandbox
hid its temporary build directory; explicitly binding that directory read-only
allowed the real check to run. No product source repair was needed.

Evidence is under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/`:
`provlita.log`, `lom.log`, `bemenu.log`, `narthex-current.log`,
`wrong-role-negative.log`, `external-tests-final.log`, `external-clippy.log`
and `inputs.sha256`. Earlier failures remain alongside the passes. All these
checks hide display devices and sockets; none changes the installed session.

### Full t252 exit: evidence still needed

The goal is the original t252 exit, not authentication alone. The authoritative
requirements remain in the linked plan and shell-file budgets.

| Required evidence | Current coverage and next proof |
| --- | --- |
| Independent clients, role grants and metadata boundaries | Generic C/Go and actual-client negotiation evidence above; retain exact client pins in the selected release. |
| Real content round trip for bar, launcher and dock | Earlier generic owners and Lom/Bemenu evidence exist; actual Provlita GPU raster/upload and candidate submission now pass. Native presentation remains separate. |
| Exact presented actions, focus and reservations | Existing owner tests are insufficient for the selected three-client native release. Exercise each output and reject stale actions. |
| Replacement, revocation, slow-reader isolation and reclamation | Authentication, actual dock custody and actual bar/menu progress through stalled-dock backpressure and replacement pass. Final native retirement still needs physical evidence. |
| Two-component baseline and three-component target | Both exact packages build and pass isolated installation and separate actual-client content/replacement tests; their native behavior still needs acceptance. |
| Narthex compatibility and rollback | Packaged descriptor client authenticates; whole-release isolated rollback passes. Native descriptor fallback and attended rollback remain. |
| B9 measured bounds | Freeze a semantically matched IPC comparator and workload method before measurements. All shell-file budgets still apply. |
| Attended acceptance and default selection | Requires the exact installed candidate after prerequisite gates; no new rollout or acceptance is claimed here. |

### t330 — Bounded route-lease delivery

Approved by niltempus on October 15 as a separate prerequisite to t252. Only
the notification repair below is promoted; the rest of t032 remains candidate
work. Exit: bounded custody and retry, explicit disconnect settlement, unchanged
identity/epoch validation, checked lifecycle model, generic full-queue and
recovery tests with a negative control, and the repository gate. No live
installation or broader input redesign is part of this repair.

The pre-repair frontend-to-Session notification sites called blocking
`SignalSender::send` and discard its error: `release_route_lease` and
`send_route_lease_update` in the X routing registry's `delivery.rs`.
The wrapper uses `SyncSender`; a full queue could therefore block indefinitely.
No live hang was induced or attributed to this defect.

The approved repair is limited to these notifications and their custody:
bounded nonblocking delivery, retained obligations under backpressure, explicit
disconnect settlement, and unchanged lease identity/epoch checks. Do not replace
blocking sends with silent drops. Model the lifecycle before implementation;
prove full-queue, disconnect, delayed drain and stale-epoch cases with generic
device-free tests and a control that fails against the old behavior. Required
accounting is accepted = delivered + explicitly settled + retained pending.
Do not promote the rest of t032, alter shell budgets or change the live profile.
The task ledger records the approved promotion as t330.

### t330 implementation and validation (October 15)

Signed Sophia `5e717c73d` contains the repair, tests and model, based on
`ad624948e`. The source hashes and complete gate below identify this candidate;
the later task-closure commit changes only the evidence note and ledger.

The registry now reserves notification space before a lease-bearing input or
release changes state. Both producers share a bounded outbox whose queued
notifications and outstanding reservations together use at most the configured
control capacity. Nonblocking publication preserves FIFO under a full channel.
Both ordinary and private service turns retry retained notifications. Session
rings the frontend's existing wake after draining, including stale updates;
an empty drain does not create an idle wake loop.

Exhausting the additional bound refuses the next lease effect with an explicit
service error. This is a deliberate fail-closed availability limit, not a
promise that arbitrary overload cannot end the frontend. Receiver disconnect
is detected on a send attempt; an effect can precede that detection. Such a
notification is accounted as abandoned delivery, never as an undone effect.
Terminal teardown also reports abandoned notifications. Accounting is
accepted = channel-published + retained pending + abandoned; publication does
not prove Session consumed the notification. Unused reservations return credit.
Lease identities and Session's epoch checks are unchanged.

The [bounded lifecycle model](../../../validation/specula/route-lease-delivery/README.md)
passes 1,034 distinct states and an eleven-step design fixture. Deliberate
silent-drop and stale-identity variants fail their intended invariants. This is
design evidence, not a runtime refinement proof. Six new X-authority tests
cover FIFO, reservations, disconnect and both real pointer-press/release paths;
two native-feature Session tests cover capacity wakes and replacement identity.
All pass with devices and display sockets hidden. A mutation restoring blocking
sends fails the release test's two-second hang guard; dropping the receiver
unblocks and joins its worker. The source was restored afterward.

The first Session test invocation omitted `native-session` and ran zero matching
tests; it is not a pass for this repair. The explicit native-feature invocation
runs both tests successfully. Evidence logs are under
`~/.local/state/sophia/development-evidence/t330-route-lease-20261015/`.
The initial full-gate wrapper created a PID namespace without a matching
private `/proc`; protected popout fixtures failed opening their namespace.
Adding `--proc /proc` fixes all three. The next run exposed an intermittent
existing seat-broker assertion: shutdown observed an extra wait/dispatch pair.
Its isolated rerun passes; the assertion and seat code are unchanged. With eight
test workers, the next run passed workspace tests and strict Clippy, then caught
a test-only snapshot helper under `src`. Moving that helper into test support
passed the layout check. `check-complete.log` records the final full rerun with
private devices, runtime directory, display sockets, PID and network namespaces.
It passes (exit 0): workspace and SDK tests, strict Clippy, layout and script
checks. The eight new tests also pass within that run. Hardware proofs are
skipped because devices are hidden, not counted as passes. `source.sha256`
records the thirteen changed Rust files tested, including the relocated helper.
No installed binary, native input timing or physical acceptance changed.

## Evidence

Commands ran through `cargo-slot sophia`, offline and locked, using the existing
slot 1. These are correctness checks, not quiet-host timing samples. No native
scanout, input-device access or installed-session change was requested.

| Scope | Current result | What it does not prove |
| --- | --- | --- |
| `sophia-runtime`: `shell_files_c`, `shell_files_c_roles`, `shell_files_oracle`, `shell_native_sdk` | 7 test cases pass, including independent C vectors, production export exchange, launcher/dock sessions and the Go verdict controls. | Product UI, native pixels or supervisor authentication in the supplied-admission oracle. |
| `sophia-session`: `shell_component_connections_files`, default features | 9 pass. | Native-feature joins omitted by this build. |
| Same connection target with `native-session` | 13 pass, including three independently negotiated roles, borrowed native content and exact indicator publication. | Physical presentation; these controls use supplied completion facts. |
| `shell_component_processes`, default/native features | 1/7 ordinary tests pass respectively. The child entry and protected tests remain ignored in the ordinary invocation. | Ignored tests are not counted as passes. |
| Protected two-process custody | Explicit device-hidden invocation passes: independent stop/replacement, retained bytes and cleanup. | Native pixels or three-product coexistence. |
| Protected joined-launcher identity | Fails at the resource-wait deadline before the fixture repair; passes afterward with two generations, stale grant refusal and cleanup. | Installed launcher UI or physical focus. |
| `sophia-conformance`: `shell_content_files`, `shell_descriptor_modes` | 4 content and 5 descriptor tests pass, including malformed/mutated peers, persistent tabs/reference, reservation commit/withdrawal and launcher replay controls. | Product release identity or installed rollback. |

The protected tests ran the already-built native-feature test executable under
`bwrap`: read-only host filesystem, private `/dev`, `/proc`, `/tmp` and
`/run/user`, private PID/network namespaces, no DISPLAY or WAYLAND_DISPLAY, and
a private runtime directory. Only the named parent test was selected; its child
entry ran through production protected launch. No test binary was copied into
retained evidence. Cargo outputs and the red/green test results are retained in
the session transcript. The initial canonical gate log is
`~/.local/state/sophia/development-evidence/t252-owner-review-20261014/check.log`.
That invocation reached automatic render-node probes: buffer-age tests passed
on `/dev/dri/renderD128` (two passed, one ignored). It was stopped after the
first-frame wrapper selected the same node and began preparing its tests.
This was not the intended device-hidden scope and is not promoted as hardware
acceptance. The first device-hidden attempt, `check-device-hidden.log`, failed
three Session unit tests because its runtime directory was `/tmp`, which does
not meet private-directory admission. The corrected run uses a mode-0700
runtime directory, private `/dev`, `/tmp`, `/run/user` and PID/network
namespaces. Its log is `check-device-hidden-private-runtime.log` in the same
directory. Builds and tests use eight workers.

## Finding and resolution

`ShellComponentSession::poll` advances connected wires through
`turn_connected`; `ShellComponentTransport::service_owner_turn` resets the
per-pass service guard. The fixture previously borrowed resource service and
called `poll_io` repeatedly after reaching Connected, without starting another
owner pass. A newly arrived upload could therefore remain unserviced until the
test timed out. Polling Session in that wait models the actual owner loop.
The test also rejects unexpected child exits and negotiation failures while
waiting. The fix does not enlarge a timeout or relax a grant check.

The independent descriptor host evidence is already recorded under
[t271](1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md#independent-descriptor-hosts-over-files-t271-2026-09-28).
The current rerun covers its generic E1 cases. Narthex product compatibility and
whole-release rollback still need exact release identities; codec independence
does not supply those identities.

## B9 method preparation

The initial read-only reconciliation, before the packaging work above, found the external desktop's
`flake.lock` pins Sophia `a29663a06`, Lom `dc65f561`, Bemenu `6e2efd96` and
Narthex `56d513fa`; it contains no Provlita input. Local Provlita HEAD is
`da4a15ad`. These are source selections, not verified installed artifacts.
The older Bemenu SDK-adoption note records an actual protected executable join
at Bemenu `a354251a` and Sophia `c23a38453`, including close/reopen and resource
retirement. Thus the old blanket "Bemenu r7 remains" wording is also stale;
the remaining question is coverage on the exact selected candidate, especially
the three-component target. No external checkout or pin was changed here.

Keep every numeric budget in [the shell file contract](../../sophia-shell-files.md#budgets).
Use an external comparison harness with frozen sources and independently built
clients. A historical IPC binary is not automatically comparable to current
owners: first document semantic equivalence or restore only the retired test
transport in an isolated comparison tree, as t249 did. Never restore it to
production or call unmatched owner costs transport overhead.

Declare these workload families before collecting acceptance samples:

- Bar-sized and 4 MiB resource upload through acceptance, with exact payloads.
- Candidate submission through actual Presented at 60 and 120 Hz, plus
  frame-demand/permit timing. Receipt time is not presentation time.
- Three independently admitted generic roles, including a stalled reader and
  replacement; report per-role progress, retained bytes and final reclamation.
- CPU per uploaded MiB and allocations per frame, with complete event totals.

Use alternating paired transport order under idle and declared load. Report
all offered, accepted, completed, refused, coalesced and timed-out operations;
never select matching survivors. Pin sample counts, warmup, hardware/output
identity, clock boundaries, instrumentation and failure rules before running.
Do not silently import t249's outcome-accounting amendment into shell traffic:
identify which operations may coalesce under the shell contract. An incompatible
comparator or proposed budget/outcome-rule change needs review before the
campaign, not reinterpretation after a failure. This is method preparation,
not a frozen runnable campaign or a claim that B9 passed.

### Proposed sampling rules, pending native calibration

These rules make the next campaign review concrete. They are not a frozen
acceptance method or permission to treat the prepared 64-frame diagnostic as
qualification. Keep the contract's numerical budgets unchanged.

Use the same DP-1 monitor at 1920×1080, scale 1 and VRR off for both wires.
Run separate 60 Hz and 120 Hz conditions, first idle and then with 16 bounded
CPU workers on this 32-logical-CPU host. Leave the workload and workers eligible
for all CPUs at normal priority; record the actual affinity, scheduler,
governor, kernel and GPU driver. Record the load executable, command, start/end
times and worker completion. A missing or failed load process invalidates its
condition. Coordinate a quiet window for unrelated heavy work, and retain any
reported interference alongside the measurements.

For each condition, propose three independently restarted pairs, with files
first in pairs 1 and 3 and IPC first in pair 2. Each latency arm offers 1,100
frames: the first 100 are a declared warmup, followed by 1,000 measured frames.
Keep and report every warmup outcome; a warmup failure invalidates the arm.
Publish each pair and the combined condition, using nearest-rank p50, p95 and
p99 plus the maximum. Every pair must satisfy the budget; pooling cannot hide
a failing pair. Preserve failed arms and stop for diagnosis rather than
replacing them with successful retries under the same campaign identity.

Use separate instrumented cost arms, with 1,000 frames and no excluded warmup,
so the whole-workload counter interval includes all work between those frames.
Report per-frame bounds as well as whole-workload counts divided by the exact
offered frame count. A conservative +10% pass requires the file upper bound
to be at most 1.10 times the IPC lower bound. A file lower bound above 1.10
times the IPC upper bound proves failure within the declared scope; overlapping
bounds are inconclusive. Neither result proves costs outside that scope.

Before freezing this method, resolve these measurement gaps:

- Upload timestamp and byte instrumentation is now implemented, including a
  separate 4 MiB transfer probe. Its device-hidden checks pass; it has not yet
  been exercised in the native campaign. Keep transfer probes separate from
  bar-presentation measurements and retain all outcomes.
- The allocation counter covers the Session and peer Rust global allocators.
  It excludes foreign allocators and authentication/WM/helper processes.
  Either add evidence covering the required scope or obtain an explicit
  accounting-method decision before judging the contract. A +10% result in
  a convenient subset is insufficient on its own.
- Preserve and review the IPC protected-PID versus 9P Factotum admission
  difference. Do not label their entire cost difference framing overhead.
- Verify native timestamps, exact output mode and wrapper recovery in the
  prepared calibration before preparing qualification artifacts. Record all
  three-role backpressure/replacement evidence separately; a single bar peer
  does not satisfy native lifetime/input acceptance for both desktop packages.

### Comparator preparation, October 15

The isolated worktree `~/dev/sophia-t252-comparator`, branch
`qualification/t252-shell-comparator`, starts at the exact candidate
`58a481ebc756b50fe97a890b968a6c9b70863496`.
The historical adapter was removed in `d13464500`; its wire enum selected
socket or files at the typed owner boundary. The shell codecs were removed in
`42fc47b42`. These are restoration sources, not accepted reference binaries.
The adapter and codecs have now been restored in that tree only. Current file
authentication, owner-turn scheduling and environment selection are retained;
authenticated launches cannot select the restored IPC path. Every source under
`shell_content` remains byte-identical to the base. Signed checkpoint `6df9b559f`
is not yet an accepted comparator. The production branch remains free of
the retired transport.

The generic `shell_comparator` integration test passes both transports through
the current resource owner with identical typed records and supplied admission.
At 8 bytes, 180 KiB and 4 MiB it checks equal limits and response records, exact
retained pixels, retirement, rejection of a reused generation and complete
reservation cleanup. The payload is valid premultiplied RGBA; chunks contain
whole rows within the negotiated limit. The file peer respects enqueue
backpressure. Neither the supplied admission nor the thread-based peer proves
independent authentication, native presentation or timing equivalence.

Evidence is under `~/.local/state/sophia/development-evidence/t252-authenticated-20261015/`:
`comparator-equivalence7.log` passes the paired test and
`comparator-runtime-lib.log` passes 106 unit tests (one ignored).
The unfiltered regression runs deliberately remain red: production guards reject
the restored shell message numbers and socket framing. Both guards are unchanged;
every framing finding is confined to `shell_transport/socket.rs` and its modules.
`comparator-protocol-compatible.log` passes with only the shell-number retirement
guard excluded. These exclusions are specific to this comparison tree, not changes
to production acceptance. `comparator-runtime-compatible.log` passes with only
the framing guard excluded; strict Clippy for both crates and all targets passes
in `comparator-clippy.log`. Formatting and diff checks pass. Test and lint runs
hide devices, display sockets and network access. Candidate, input, replacement
and scheduling equivalence
still need proof before freezing workloads or collecting B9 timing.

Signed successor `c466ff065` extends the paired fixture to allocation, frame
demand/permit and candidate submission. It compares complete typed limits,
output facts, allocation results, permits, resource responses and candidate
outcomes. Both wires produce Prepared then Presented for a supplied successful
completion, and Prepared then RendererFailed for the negative control. Exact
resource bytes and their reservations survive repeated disconnect while a render
lease is held; dropping it permits full reclamation. The successful case sends
one supplied target action and checks every acknowledgement identity field at
the owner. Unknown records while awaiting outcomes fail the test.

The file client submits one whole candidate; IPC carries Begin, Chunk and End.
These are their declared wire forms for the same typed candidate. The fixture
uses the existing generic file test's geometry and owner completion inputs. It
does not compare physical hit testing, authorization of an input effect, native
scanout, fair multi-role scheduling or independently built client processes.
`comparator-candidates-final.log` passes both comparator tests, and
`comparator-candidates-clippy-final.log` passes strict Clippy for the changed
test; formatting and diff checks pass. These tests ran with devices, display
sockets and network hidden. The comparison branch remains separate from the
production candidate. Next cover replacement and bounded multi-role progress,
then freeze the B9 method with actual presentation instrumentation. The package-
bound native runner also remains to be prepared for the paired desktop gate.

Signed successor `5a880fb78` adds a three-owner fixture on the same 64 MiB
registry and declared bar/menu/dock envelopes. All three upload. The third
client then performs no reads or SDK polling while output-facts publications
continue until the owner reports bounded backpressure. In this run, refusal
occurred at publication 256 over files and 342 over IPC. These counts include
the refused publication and are not asserted to be equal. They reflect different
wire buffering and must not be hidden by selecting matching survivors in B9.

The two other clients upload while the stopped reader is still connected and
visited on every owner cycle, after its revocation, and after its replacement.
Their grants remain unchanged. The held four-byte resource blocks replacement
within the unchanged shared budget until it is released. The replacement then
uploads; neither the old owner nor the new one can lease the other's grant.
Every successful resource response is compared across wires, every retained
pixel is checked, per-owner response bounds hold, and final accounting is
quiescent. `comparator-components1.log` passes all three comparator tests;
`comparator-components-clippy.log` passes strict Clippy. Formatting and diff
checks pass. This supplies admission and pauses a thread's I/O deliberately;
it does not establish native process scheduling, kernel latency or presentation.

### Measurement instrumentation, October 15

Signed comparator `2aebdcdfd` has an explicitly ignored `measurement::resource_probe`.
It records an offer before each upload and its result after acceptance, using
the same validated exchange as the equivalence test. It alternates transport
order for 180 KiB and 4 MiB payloads, records warmups as well as samples, and
never replaces a failed operation. A panic leaves an unmatched offer and no
completed-run record. Counts must be supplied explicitly and are bounded.
The probe requires its exact test filter, `--ignored` and `--test-threads=1`:
process-wide counters would otherwise include unrelated tests.

The wall interval starts immediately before ResourceBegin and ends when the
client decodes Accepted. Preparation, negotiation, pixel verification, retirement
and teardown are outside it. Process CPU and allocator snapshots bracket that
wall interval and cover both the client and the owner, including counter-call
overhead. Each upload currently uses a fresh connection, and the owner
is driven by a bounded service loop; this is not the native Session scheduler.
The fixed payload contains valid premultiplied pixels and uses whole-row chunks
within the same negotiated bound on both wires.

Allocation counting is a separate build feature,
`comparator-allocation-counting`, using pinned
[`stats_alloc` 0.1.10](https://docs.rs/stats_alloc/0.1.10/stats_alloc/).
It counts Rust system-allocator calls in both threads, not foreign-library
allocations. The default build keeps the ordinary allocator for latency and
CPU runs; absent counts are recorded as `unmeasured`, never zero. No new unsafe
code or allocator change enters the production branch. The lockfile adds only
this one package.

The first diagnostic found repeated full encodings in the harness's file-client
queue retry. The harness now waits through the SDK's readiness API, as its IPC
peer waits on the socket. This changes the comparison client, not the production
SDK. The earlier log remains `comparator-instrumentation-smoke.log`.
`comparator-instrumentation-ready.log` and `comparator-uninstrumented-smoke.log`
each contain twelve matched offers/results: one warmup and two samples per wire
and size. They are debug-build instrumentation checks, not accepted timing
distributions. `comparator-probe-serial-refusal.log` confirms an ambiguous
invocation fails before offering work. The instrumented equivalence regressions
and strict Clippy pass in `comparator-instrumented-regressions.log` and
`comparator-instrumentation-clippy.log`; formatting and diff checks pass.
The optimized uninstrumented comparator builds and passes its three semantic
tests, with the diagnostic probe ignored, in `comparator-release-build.log` and
`comparator-release-regressions.log`. No optimized timing campaign was run.

Still needed before B9 acceptance: native submission-to-presentation and
demand-to-permit instrumentation at both refresh rates; allocations per
presented frame; a complete frozen method with hardware, sample counts, load,
timing boundaries and every terminal outcome; then the paired campaign and its
numeric verdict. Upload allocations alone do not prove the per-frame budget.

The existing external tty4 dock runner is only a host-transcript smoke gate:
its verifier explicitly leaves latency and restart acceptance unproved. It also
copies prepared binaries into evidence, which conflicts with the current build
storage rule. Prepare a package-bound native run that records immutable paths
and hashes before using it for the remaining attended checks; do not run that
old copy-based path unchanged.

### Native completion join, October 15

Sophia `58a481ebc` already emits `sophia_shell_native_binding` from the actual
queued frame and `sophia_shell_native_completion` from displayed custody. The
latter uses the maximum completion time across the exact converged mirror heads
and identifies kernel versus observation-fallback clocks. The source is
`production_visual_runtime/projection/retirement.rs` and
`production_session/native_scanout/persistent_native_scanout/presentation_timing.rs`
in `sophia-backend-live`. No Engine instrumentation change was needed.

External integration `91d5370` adds the read-only Rust command
`xtask shell-timing join HOST_LOG CLIENT_JSON MODE_MILLIHZ EXPECTED_OFFERS`.
Its README specifies the complete client-offer record. It requires the exact
declared 60000 or 120000 mHz mode, unchanged native owner/topology, every head,
kernel timestamps, and a matching offer count. It refuses duplicate offers,
non-presented outcomes, conflicting identities, incomplete or missing completion,
and timestamps before submission. Repeated redraws may bind one candidate to
several frames; the earliest exact completion supplies the latency. Two different
roles may share a native frame, but one role cannot claim two generations in it.

The five tests include the real command from a directory outside the checkout,
mirror coverage, later redraws and refusals for missing samples, stale owners,
changed topology and fallback timestamps. `shell-timing-join-final.log` and
`shell-timing-join-clippy-final.log` pass under the device-hidden sandbox.
The result is explicitly `joined_only`: this is tested evidence plumbing, not
a physical run, an attestation of client-recorder completeness, a bound artifact
pair or a B9 verdict. Next record every native client submission and terminal
outcome, bind the inputs to the selected artifacts, and complete the campaign.

### Paced timing peer and journal, October 15

Signed comparator `b2bfb9678` adds the generic `shell_timing_peer` example.
It uploads changing pixels, waits for a permit, records each offer before
submission, waits for its terminal outcome, and retires the previous resource
only after its successor is presented. Allocation, permit and outcome identities
must match. The bounded JSON configuration names `wire`, `output`, `frames`,
`height` and `hold_msec`; launching requires `--serve` and
`SOPHIA_SHELL_CONFIG`. The frame count cannot exceed the resource identity budget.
The final hold keeps the last frame available until a harness stops the peer.

Files use the current SDK and, when supplied, its launch-authentication relay.
IPC refuses an authentication descriptor or file endpoint, so it cannot turn a
protected file launch into an unauthenticated comparison. The authentication
branch now passes the existing generic protected-launch test twice, with
revocation and reaping at each fresh epoch. It waits for output facts when
revoked; this proves authentication, not content or native presentation.
Evidence: `timing-peer-auth-native-feature.log`. The earlier
`timing-peer-auth.log` omitted the native-session feature and ran zero matching
tests; it supplies no authentication evidence.

Signed external integration `fbe20dc` adds `shell-timing collect`. It requires
the complete start/permit/offer/terminal/end sequence, exact counts and identities,
forward clocks and successful outcomes. Missing or duplicate rows, truncation,
rejection, unknown fields and trailing failure records prevent sample export.
The original journal retains demand-to-permit timestamps. Offer timestamps
precede the flushed journal write, so that write's cost is included on both
wires. This is a declared measurement boundary, not a completed B9 method.

Device-hidden checks pass: two peer tests cover both wire round trips and
launch refusals; eight external tests cover journal validation and the existing
native join. Strict Clippy passes for the peer example/tests and all xtask targets.
The collector also accepts the actual three-frame child journals from each wire.
Evidence under `t252-authenticated-20261015`: `timing-peer-journals.log`,
`timing-peer-journals/{files,ipc}.{ndjson,json}`, `timing-peer-clippy-final.log`,
`timing-journal-tests.log` and `timing-journal-clippy.log`. These runs use supplied
admission and presentation receipts. They prove recording and parsing, not
native presentation, input or performance acceptance. Protected authentication
is covered separately by the test above.

### Guarded native comparison selection, October 15

Signed comparator `16d1cb870` wires the retired adapter into the existing
component Session behind `transport "comparison-ipc"`. Native startup also
requires `SOPHIA_COMPARATOR_ADMISSION=protected-legacy-baseline`. Without that
guard, Session refuses the selection before launching a component. With it,
the existing supervisor supplies the protected process and exact peer identity;
normal connection epochs, grants, content owners and cleanup still apply.
The new selector and all restored IPC remain confined to the comparison branch.

Current 9P selections still take the unchanged Factotum path. The IPC baseline
cannot authenticate a file launch, and the runtime still refuses to downgrade
an authentication-required connection. This is deliberately asymmetric:
current 9P includes its ongoing authentication checks; retired IPC uses its
protected-process admission. Admission is outside the timed upload/frame window,
but ongoing checks are inside. The complete campaign method must name and review
that difference; do not describe the result as the cost of framing alone.

Device-hidden evidence: `comparator-protected-launch-final.log` proves two
protected IPC negotiations, immediate revocation, reaping and cleanup before
replacement; `comparator-unarmed-refusal.log` proves the missing guard refuses
without spawning. The peer reports a failed read when the test deliberately
closes its connection before publishing output facts. Those records are expected
in this launch-only test and cannot enter a timing dataset. The first launch
test omitted Session's retained revocation cleanup before replacement and failed;
the final test performs that cleanup and proves quiescence.

The full configuration suite and 20 Session file-connection/process tests pass
(three explicit process tests remain ignored in that ordinary run). Strict
Clippy passes for configuration and all native Session targets. The native CLI
also checks successfully. Logs: `comparator-config-regressions.log`,
`comparator-session-wire-regressions.log`, `comparator-native-selection-clippy.log`
and `comparator-native-cli-check.log`. No native session, physical input,
presentation or timing campaign ran in this checkpoint.

### Bound native runner, October 15

Signed external integration `0c8c1a0` adds `shell-timing prepare-native` and
`run-native`. Preparation binds the selected Sophia, peer, WM, preflight and
runner executables plus core, desktop and peer configuration to explicit hashes.
It requires clean repositories at signed source revisions. The desktop profile
must name exactly the selected bar peer, config and wire, deny peer GPU access,
and select empty startup; core/profile includes are refused. The peer's frame
count, output and full-runtime hold must match the request. The README documents
the request format and commands.

Execution requires the plan hash, unchanged artifacts and sources, the prepared
runner executable, `SOPHIA_SHELL_TIMING_ARM=1`, and the named console in the
foreground. It shares the existing output proof's wrapper supervision rather
than creating another shutdown path. The wrapper owns input guarding, watchdog,
Session termination and console restoration. Capture is bounded and private;
binary artifacts remain at their recorded paths. Normal recovery must verify
before the runner preserves every client JSON record, extracts native witnesses
and attempts the complete offer-to-kernel-completion join. Missing, damaged or
failed client records cannot become successful samples.

`result.json` records wrapper capture/recovery only; `native-samples.json` is
written only after a successful join and reports `joined_only`. Neither is B9
acceptance. Source revisions and executable hashes are separate assertions, not
proof of the build process. Record build commands and artifact provenance before
the campaign. The expected mode is checked against bindings after capture; the
runner does not set monitor modes or establish physical connector identity.
The hardware/output selection and full profile configuration still need review.

Six new device-hidden tests cover both wire plans, changed artifacts/workloads,
profile indirection and extra clients, altered plan hashes, unarmed/non-console
launch refusal, and preservation of failed or damaged client records. The
existing output-runner checks and the xtask regressions also pass; strict
all-target Clippy passes. Evidence under `t252-authenticated-20261015`:
`timing-native-xtask-regressions-final.log`,
`timing-native-verifier-isolation-fixed.log`,
`timing-native-regressions-remainder.log` and
`timing-native-runner-clippy-final.log`.

The first broad run found the stale provisioning marker after adding the already
cached KDL dependency. `tools/provision.sh` refreshed it, audited the pinned
source and reported no downloaded crates or registry/git changes; see
`timing-native-provision.log`. Its accepted lock hash is
`8372db786f2e6313652a7ac7e18bc86eca5cf5d170624f99423ce8fdac81256b`.
The next run exposed an isolation setup error in an older verifier: `/run/user`
was hidden while `XDG_RUNTIME_DIR` still pointed into it. That verifier passes
with the variable unset, and the remaining test binaries pass separately.
No physical run plan or B9 campaign ran at this checkpoint.

### Frame counters, October 15

Signed comparator `e66d612d1` instruments the Session and peer processes behind
`comparator-allocation-counting`. Sophia also requires `native-session`. The
instrumented executable refuses session runs without an explicit lifetime of
at most 60 seconds, before diagnostic capture or desktop startup. The peer now
alternates two resource IDs with fresh generations after retirement; 64 offers
on each wire pass with counters enabled and disabled.

The peer records counters before pixel preparation/upload and after Presented
plus retirement. Session samples every two milliseconds, subject to scheduling.
Clock bounds surround each snapshot. External integration `60dd30f` brackets
each frame with enclosing and enclosed Session snapshots, then adds the peer's
counter delta. It also brackets the whole workload once, including work between
frames. It rejects incomplete lifecycles, counter resets, process changes,
missing coverage and instrumentation inconsistent with the declared run mode.
The native runner requires both cost verification and the native completion
join; the standalone cost checker cannot attest presentation.

Allocation operations count Rust global allocations and reallocations in those
two processes. Foreign allocators and other processes are excluded; process CPU
includes all their threads. Instrumentation overhead is included. These scope
limits, warmup handling and aggregation still need to be fixed in the campaign
method. Neither the feature nor its verifier changes the B9 budgets.

The debug supplied-owner recording contains all 64 offers on each wire. Its
whole-workload bounds are:

| Wire | Allocation operations | Process CPU |
| --- | --- | --- |
| 9P | 50,325–52,339 | 78,248,934–82,264,303 ns |
| IPC | 13,943–15,667 | 23,042,918–26,435,216 ns |

This is a substantial diagnostic gap. The workload is a small 64×24 surface,
with supplied admission and completion, debug builds and no native rendering.
It establishes neither B9 success nor failure. Preserve it when planning the
native cost comparison; do not describe the instrumentation checks as a
performance pass.

Evidence under `t252-authenticated-20261015` includes
`frame-counter-recording.log`, `frame-counter-journals/`,
`frame-counter-unit.log`, `frame-peer-uninstrumented-tests.log`,
`frame-counter-cli-guard-final.log`, `frame-counter-cli-clippy-final.log`,
`frame-cost-tests-final.log` and `frame-cost-clippy-final.log`.
The CLI guard's two tests pass, as do the counter tests, both peer modes,
18 timing verifier tests and focused strict Clippy. The earlier CLI test attempt
without `native-session` failed on the existing renderer feature dependency;
the supported native feature combination passes. No physical session changed.

### Prepared native diagnostic, October 15

Comparator `e66d612d1` now has optimized, uninstrumented Sophia and peer builds.
Preparing the actual arguments exposed a missing runner dependency: current
shell and public WM startup require the Session authenticator. External
integration `643dec3` binds the Factotum agent and PAM helper as executable
artifacts and supplies both arguments on both wires. Its six native-runner tests
and strict all-target Clippy pass. Both complete argument vectors also pass
Sophia's `--validate-session-args` in device-hidden isolation.

The prepared calibration selects the larger Dell S3222DGM on DP-1 at
PCI `0000:03:00.0`, 1920×1080 at exactly 60 Hz, scale 1, VRR off. The second
monitor is disabled only in these temporary profiles; the development GPU is
excluded. Read-only EDID inspection confirms this monitor also advertises exact
1080p/120 Hz for the later paired campaign. The profiles do not alter the
installed configuration. Each wire offers 64 successive 1920×24 frames and
holds the final frame until the 60-second Session limit. There is no startup
application or deliberate load. This is a recording/recovery diagnostic, not
B9 acceptance; no warmup samples are discarded or thresholds judged.

The plans use the packaged dock candidate's unchanged Hagia, Factotum and PAM
helper, the new optimized generic comparator/peer and integration runner.
Sources, artifact paths, SHA-256 hashes and exact arguments are recorded under
`t252-authenticated-20261015/native-calibration-01/`. Build JSON and logs sit
beside that directory; no binaries were copied into evidence. The first config
validation refused non-private file modes; after making inputs private, the
device-hidden validation needed the native feature's explicit smoke opt-in.
Both final logs report accepted arguments without opening devices.

Prepared plan hashes:

- Files: `b51b75d08df79f8053d38099a4dfa571f9ff1f564461669f76eb26d11c161654`.
- IPC: `119acdb9506dccfc4c646ea49afc05a9409c6730d40f24822a8ef54d589348a9`.

The short launcher `~/.local/bin/t252-check` runs them in that order from tty4,
stopping on any failure. The runner refuses changed artifacts, changed sources,
an occupied run directory or the wrong foreground console. The operator must
leave the live desktop before this attended check. Both runs remain unexecuted.
The complete qualification method still needs its load, sample, warmup and
accounting rules frozen; this calibration cannot fill that gate.

### Native calibration 01 failure and prepared retry

The October 15 attended file run reached DP-1 at 1920×1080, exactly 60 Hz,
then failed with `authenticated role launch deadline`. Factotum repeatedly
reported `a pam helper directory is not root-owned and protected`. Its helper
was an immutable Nix-store file, but `/nix/store` was group-writable. The
root-owned-path check intentionally rejects that ancestry. No peer samples or
performance result followed, and the launcher stopped before IPC.

The wrapper's retained recovery records report restored termios, identical
before/after keyboard mode 3 and KD mode 0, and `keyd_restored=true`.
The failed run remains under `native-calibration-01/files-run`; inspect
`untrusted-session-output.log` for the actual Session failure, rather than the
short wrapper-owned `session.log` alone.

Integration `4ec2904` now checks the canonical helper and every ancestor for
root ownership and absence of group/other write permissions before preparation,
and again before execution. Seven device-hidden runner tests and strict Clippy
pass. The original store helper also fails the rebuilt preparation command on
the real host before a plan is written. No permission check was weakened.

Calibration 02 uses the existing helper at
`/opt/sophia-niltempus-desktop/releases/niltempus-b7f017d6e4cdb2056faa/target/release/sophia-factotum-pam`.
Its SHA-256 is identical to the packaged candidate helper:
`deb6f5113690eb24bb4dbce2db408e7a253d1ed4e594b3c1ef6e6fef3ef09695`.
All other workload choices remain as in calibration 01. The new runner and
signed source revision are bound in fresh plans under `native-calibration-02`;
both actual argument vectors pass device-hidden validation. `t252-check` now
selects these plans. Neither has run yet.

The owner separately reported that after a day-long lock the smaller monitor
continued the Matrix animation while the larger appeared frozen. The password
successfully unlocked the desktop. This is an unconfirmed lock-animation issue,
not an unlock failure or evidence about the calibration's authentication error.

### Native calibration 02: optional indicator polling

The second attended file run completed authentication and negotiated the peer
at revision 6. It reached DP-1 at 1920×1080/60, then repeatedly reported
`service_failed ... reason=MissingCapability`. The peer's later `ack write
refused` was a consequence of revocation. The Session continued until its
60-second deadline without presenting any peer frames. Its final native
presentation validation failed; IPC did not start. Recovery again records
restored termios, keyboard mode and KD mode. Keep the evidence at
`native-calibration-02/files-run`.

The Session's indicator service called `poll_indicator_activation` without
checking whether the content peer negotiated that optional capability. The
transport correctly refused it. Adding more capabilities to the test peer
would hide the owner bug: a content-only peer is valid. Production commit
`f8adbcdb4` instead skips indicator activation service for such peers. Existing
negotiated indicator handling and transport rejection rules remain unchanged.
The comparator carries the identical change as `bef52ef0f`; both wires use it.
The original `58a481ebc` owner baseline is therefore superseded by this explicit
Session correction for subsequent comparisons, not silently treated as identical.

The real private 9P client regression fails with `MissingCapability` before
the fix. It passes afterward, together with the five existing client
action/indicator tests. Strict Session Clippy also passes. Evidence:
`optional-indicators-before.log`, `optional-indicators-after.log` and
`optional-indicators-clippy.log`. These prove the optional-service correction,
not native presentation. Rebuild and bind a fresh comparator plan before retry;
the installed desktop and its packaged candidates have not been replaced.
The rebuilt comparator is now bound in `native-calibration-03` with both
argument-validation checks passing. Its files plan hash is
`cc0bc8e7ec529664277816300104325c42d5536103ab7a2ffada6a4d2de3543c`;
IPC is `923dcb4fb8285d319cc6ca2b0e71112a8ade6f9ef9a4d3e08e30fa850ee03c2e`.
The short launcher selects calibration 03; neither run has started.

### Native calibration 03: singleton timing witness

Calibration 03 passed the file peer's complete journal: 64 permits, 64 offers,
64 successful terminal receipts and an end record, with no peer failure. The
host recorded 64 candidate/native-frame bindings at exactly 60 Hz. Session and
wrapper recovery completed successfully. There were, however, zero
`sophia_shell_native_completion` records. The join correctly refused
`offered candidate has no exact native completion`, so no native timing report
was produced and the launcher did not start IPC. `captured_only` in the wrapper
result is not a successful timing verdict. The retained `client-offers.json`
and `join-diagnostic.log` reproduce these findings from the raw capture.

Singleton scanout keeps its displayed buffer in the output runtime's custody;
mirrored heads keep theirs in each head's custody. The timing logger always
looked at the latter, which is empty for singleton output. Production
`08be65293` selects the actual custody owner and emits the singleton witness
after accepted callbacks, before any successor submission. Comparator
`136cbcb41` carries the same change. Exact frame identity, head coverage and
kernel timestamp checks are unchanged; no timestamp is reconstructed from the
client receipt or log arrival time.

The new custody regression uses real boxed displayed owners and checks that
singleton timing reads runtime custody, never falls back to mirror storage,
and mirror timing uses only mirror custody. Five focused timing tests, the
device-hidden native-event integration tests and strict backend Clippy pass.
The first integration invocation selected zero tests with an incorrect filter;
the subsequent unfiltered target is the evidence. Logs are
`singleton-timing-owner-tests-final.log`,
`singleton-timing-callback-order-tests-final.log` and
`singleton-timing-owner-clippy.log`. The initial backend build omitted
`gbm-probe` and failed on the existing renderer feature dependency; the
supported `test-support` combination passes.
The native-event target reports 316 passed and one ignored. Calibration 04
binds the rebuilt comparator in fresh plans; both device-hidden argument
checks pass. The short launcher now selects `native-calibration-04`. No run
has started and no installed desktop artifact has changed.

### Native calibration 04: both wires joined

Both attended diagnostic arms completed successfully on October 15 using
comparator `136cbcb413ab60a3103fe88814601bd4a958c095` and integration
`4ec2904efecad93904bb21ad75dd4fee3c626f5c`. Each journal contains 64 permits,
64 offers, 64 successful terminal receipts and an end record, with no failed
event. Every offered candidate joins to an exact native completion on DP-1 at
1920×1080/60 Hz. Neither capture contains a service failure or fallback-clock
marker. Both wrapper recovery records verify termios, keyboard and KD-mode
restoration. The installed desktop is unchanged.

Independent replay of `collect` and `join` from each retained capture passes;
the run's stored plan is byte-identical to its prepared plan. Derived
`client-offers.json` and `rechecked-join.json` are retained beside the original
`native-samples.json` under `native-calibration-04/{files,ipc}-run`.

For diagnosis only, nearest-rank submit-to-native-completion latencies over
all 64 frames, with no excluded warmup, are:

| Wire | p50 | p95 | p99 / maximum |
| --- | --- | --- | --- |
| 9P | 16.310 ms | 16.490 ms | 25.581 ms |
| IPC | 16.746 ms | 16.913 ms | 45.547 ms |

This verifies the recording path and recovery, not performance qualification.
There is one short pair at 60 Hz, no declared load or accounting pass, and no
120 Hz run. Do not infer a transport advantage from these diagnostic samples.
The complete B9 method and both desktop packages' native lifetime/input and
attended release/rollback gates remain open. The existing `t252-check` points
to completed runs and correctly refuses to overwrite them.

## Validation and remaining work

The corrected device-hidden `cargo xtask check` passed (exit 0), including
workspace tests, strict Clippy and the available script checks. Hardware-dependent
proofs were skipped, not passed. The source was based on `d160fd03e` with this
fixture repair and the separate t254 codec foundation; no installed artifact
was selected. `git diff --check` also passed.

The t330 route-lease concern is resolved by the candidate and checks above.
Authenticated startup, actual dock GPU upload and three-client progress through
backpressure/replacement now pass on the updated pair. Next prepare the exact
paired-client native gate for both selections and freeze the runnable B9 method.
Do not mark t252 complete or advance its default/rollback acceptance on the
strength of the generic checks alone.

## Upload journals (October 15)

Signed comparator `df3a08f8d` emits schema 2 journals. Every frame has a resource
upload offer before ResourceBegin and a matching record after decoding Accepted.
The offer binds the grant, output, frame, resource identity, dimensions and RGBA
byte count. Timing excludes pixel preparation but includes flushing the offer
record. This conservative boundary is explicit; it is not an uninstrumented
transport-only measurement.

Signed integration `fd1e0a1` validates the full sequence and exports upload and
permit timestamps through `xtask shell-timing metrics`. Native reports now
include those metrics. Missing acceptance, changed identity, invalid byte count,
backward clocks, mixed schemas and incomplete journals fail without exporting
surviving frames. Slow accepted uploads remain in the data for budget assessment.
Schema 1 replay remains supported with an empty upload list, not invented data.

Device-hidden checks passed: five journal tests, five native-join tests, four
counter tests, seven runner tests and two real-child peer tests. Strict Clippy
passed for external xtask and the generic peer. Both real-child wire journals
exported 64 uploads, permits and offers. Both calibration 04 journals replayed
as schema 1 with all 64 offers. Evidence is under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/upload-journal-01/`:
`files.ndjson`, `ipc.ndjson` and their `*-metrics.json` exports.

These are supplied-owner checks, not new native measurements. At 1920×24 the
upload is 180 KiB; the peer does not yet provide the separate 4 MiB workload.
CPU/MiB accounting scope, the frozen campaign method and physical acceptance
remain open. No live installation or console state changed.

## Bulk upload probes (October 15)

Comparator `ba871b742` accepts optional `probe_bytes: 4194304`. Before each bar
upload, it uploads a separate 1024×1024 RGBA resource, waits for Accepted,
retires it and waits for Released. The probe uses resource ID 1; the displayed
bar alternates IDs 2 and 3. Fresh IDs increase and reused IDs advance generation.
The initial harness attempt incorrectly introduced ID 3 first, then ID 1;
Sophia rejected it. Correcting the peer's ordering resolved that failure without
changing production admission rules.

Integration `571ba0a` binds the option through the prepared configuration hash,
checks the captured declaration and requires exact probe dimensions, bytes,
identity and release ordering. Metrics distinguish `transfer_probe` from
`presentation`. The probe's bytes are never described as Presented. The bar's
geometry remains unchanged, and probe-disabled runs preserve the original path.
Use the disabled mode for presentation-latency comparisons; process counters
in enabled mode include both uploads, preparation and retirement.

Device-hidden real-child checks passed all four combinations of wire and probe
mode: 64 bar offers/receipts each, plus 64 exact 4 MiB acceptance/release cycles
per enabled arm. The supplied bar is 64×24, not the native 1920×24 workload.
Four captured journals independently passed the external metric exporter.
Old schema 1 calibration and schema 2 upload journals also replayed. Six journal,
eight runner, five join and four counter tests passed; strict Clippy passed for
external xtask and the peer/test targets. Evidence is under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/upload-probe-02/`.
The earlier `upload-probe-01` directory contains only completed probe-disabled
arms from the failed harness attempt; it is not a passing bulk-probe run.

This closes workload preparation, not native qualification. Accounting scope,
the frozen campaign, native lifetime/input and attended rollback remain open.
No new physical session or installation ran.

## Accounting method proposal (October 15)

Status: accepted by the owner on October 15 through plan selection and the
instruction to implement it. The +10% budget is unchanged; beating IPC on both
cost measures is the optimization objective.
Use steady-state Session-plus-peer CPU across all their threads, including
foreign-library CPU, divided by all successfully uploaded bytes in the complete
declared workload. Count Rust global allocation and reallocation operations in
those same processes per offered frame. Include preparation, upload, demand,
presentation, retirement, instrumentation and intervening work. Do not subtract
an idle baseline or discard slow frames. Keep the existing conservative bounds
and separate instrumented runs from latency runs.

This scope excludes WM, Factotum, PAM and other processes, startup before the
first frame, foreign allocation calls, GPU allocations and kernel allocations.
These exclusions now apply explicitly to the B9 cost gate; they do not
establish whole-desktop cost parity. Existing supplied-owner reports remain
diagnostic because they do not exercise native Session presentation.
The protected-PID IPC admission and Factotum file admission remain documented
differences; their full startup costs are not measured by this proposal.

External integration `4768fbf` implements the scoped normalization without
declaring acceptance. `cost-metrics` requires complete schema 2 journals and
matches every upload, permit and terminal to its counter frame. It separately
reports presentation and probe bytes, uses 1,048,576 bytes per MiB, and rounds
integer bounds outward. Old offer-only cost reports stay unnormalized. CPU and
allocation scope labels distinguish foreign-code CPU from foreign allocations.

Five cost tests, six journal tests, eight runner tests and strict xtask Clippy
pass. The instrumented generic peer's two tests pass, covering both wires with
and without probes, 64 frames per arm. Four independent `cost-metrics` replays
pass. Evidence is in
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/normalized-cost-01/`.
The debug supplied-owner bar is 64×24. Total uploaded bytes are 393,216 per
bar-only arm and 268,828,672 per probe-enabled arm, including 268,435,456 probe
bytes. Whole-workload allocation bounds per frame are 821–851 versus 250–271
for files/IPC without the probe, and 21,498–21,538 versus 8,288–8,300 with it.
These are diagnostic excesses within the stated scope, not native B9 verdicts.
No hardware, installation or acceptance criteria changed.

## t331: single-connection readiness

The first attribution split used peer counter deltas from `normalized-cost-01`.
For the small bar, the peer alone allocated about 442 operations/frame on files
versus 111 on IPC. For the bulk probe these were 1,608 versus 645, leaving most
of the 21,500 total file operations in the supplied owner process.

`sophia-9p::unix::Server::wait` allocated both a descriptor vector and a result
vector on every turn, including empty polls. Production `34edcd707` keeps both
on the stack when there is one adopted socket and no listener. The descriptor
interest calculation remains shared with the general path. Listener and
multiple-connection handling retain their existing behavior; no cache or new
retained storage is introduced. Comparator `fd9423597` carries the same fix.

The complete sophia-9p test suite and strict all-target Clippy pass, including
adopted sockets, buffered partial requests, backpressure, wake/stop, listener
capacity, authentication and revocation. The instrumented comparator peer
checks pass all four wire/probe combinations, with 64 frames per arm, and all
four independent normalized-cost replays pass. Evidence is under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-single-poll-01/`.

| Workload | File allocations/frame before | File after | IPC after |
| --- | --- | --- | --- |
| Supplied 64×24 bar | 821–851 | 668–683 | 252–272 |
| Same bar plus 4 MiB transfer probe | 21,498–21,538 | 8,953–8,964 | 8,366–8,376 |

These are debug supplied-owner diagnostics, not a balanced native campaign or
a B9 pass. Their busy owner loop magnifies per-turn allocation; do not project
the percentage reduction onto the live desktop. The small-bar gap remains,
with most of its excess in the peer. Next inspect the SDK's reply decode and
reply-drain allocation paths. Change SDK source in
`~/dev/sophia-desktop-sdk-rs`, then use the signed snapshot workflow; Sophia's
vendored SDK is immutable. t331 and t252 remain open; accounting scope is still
proposed, and no live installation changed.

## t331: SDK reply storage

SDK `894dbcacce81b6085db9de848a32c644007825dd`, published on
`performance/t331-reply-storage`, starts from the exact previously pinned SDK
`0e5ad3638`. The working tree is `~/dev/sdk-rs-t331-reply-storage`; the older
primary SDK checkout was left untouched. Sophia `64524a300` vendors the signed
revision through the standard snapshot tool; comparator `651e4a969` carries
the same pin.

The 9P pipeline previously copied each complete frame into a new vector, then
copied its body into another. It now borrows the body from the receive buffer
while decoding, restores that buffer and retains any partial next frame.
Decoded payloads still own the data they need. The shell reply drain reuses its
outer scratch vector, bounded by pipeline credits, and clears all replies on
both success and error. Events are still buffered before submission responses
are judged, preserving the existing custody ordering.

The SDK workspace checks pass, including a new complete-reply/partial-next-frame
regression; all 30 pipeline tests and the shell-client tests pass after the
changes. Strict workspace/all-target Clippy passes. Sophia's signed snapshot
and contract check passes (`t331-sdk-check.log`), as do 13 focused production
file transport, reduced-limit and owner tests. Both instrumented peer tests
pass all four wire/probe combinations; all four cost replays pass.

Evidence is under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-sdk-replies-01/`.

| Workload | Files before SDK change | Files after | IPC after |
| --- | --- | --- | --- |
| Supplied 64×24 bar, allocations/frame | 668–683 | 538–553 | 252–271 |
| Same bar plus 4 MiB probe | 8,953–8,964 | 8,542–8,552 | 8,358–8,374 |

Peer-only allocation deltas after the change are about 316/frame for the bar
and 1,200/frame with the probe. The remaining small-bar excess still includes
both client and server work. Next use the intended 1920×24 bar geometry in
the supplied-owner diagnostic, keeping old results and not confusing their
6 KiB fixture with the 180 KiB contract example. None of these debug runs is
a native B9 verdict. t331, the accounting decision and t252 remain open.

## t331: panel baseline and pending request encoding

Comparator `05cbdd476` changes the supplied output to 1920×1080 and its bar to
1920×24. The four instrumented wire/probe arms complete 64 frames each and
pass independent normalized-cost replay. Evidence is under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-panel-180k-01/`.
Each bar uploads exactly 184,320 bytes; each bar-only arm uploads 11,796,480
bytes. Probe-enabled arms upload 280,231,936 bytes, including 268,435,456 probe
bytes. These remain debug supplied-owner measurements, not native evidence.

| Workload | Files allocations/frame | IPC allocations/frame |
| --- | --- | --- |
| 1920×24 bar | 839–850 | 623–634 |
| Same bar plus 4 MiB probe | 8,892–8,907 | 8,797–8,805 |

SDK `af21f0a` replaces heap-built fixed request headers with stack arrays
and writes header/payload slices directly into the existing bounded output
buffer. Secret writes still reserve the full output bound before copying the
secret and scrub vacated storage; the temporary body no longer exists.
Full SDK workspace tests and strict all-target Clippy pass. The test log is
`t331-sdk-request-tests.log` beside the evidence directory.

Signing initially timed out; after the owner's unlock, the signed SDK commit
was pushed. Production `368e07c3f` and comparator `95dc246d2` pin that revision.
The signed snapshot and SDK contract gate passes (`t331-sdk-request-snapshot.log`),
as do 13 production file transport, reduced-limit and owner tests.

All four instrumented diagnostic arms and normalized-cost replays pass under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-sdk-requests-01/`.
Geometry, frame counts and uploaded bytes match the panel baseline above.

| Workload | Files before | Files after | IPC after |
| --- | --- | --- | --- |
| 1920×24 bar, allocations/frame | 839–850 | 709–721 | 622–633 |
| Same bar plus 4 MiB probe | 8,892–8,907 | 8,126–8,138 | 8,580–8,595 |

CPU nanoseconds per uploaded MiB are 30,697,002–31,079,608 for the bar on
files versus 27,936,329–28,201,071 on IPC; with the probe, they are
21,198,846–21,214,794 versus 24,768,934–24,787,613. These are conservative
Session-plus-peer bounds in the debug supplied-owner harness. They neither
approve the proposed accounting scope nor establish native B9 acceptance.
The panel allocation excess remains above +10%. Source review also found heap
growth in `walk_body`; that path remains unchanged and is the next attribution
candidate. t331 and t252 remain open.

## t331: walk encoding

SDK `f638e2f` validates all walk-name lengths, computes the bounded body size
and reserves its buffer once. Both callers already limit walks to 16 names;
wire bytes, request limits and reply handling are unchanged. SDK workspace
tests, formatting and strict all-target Clippy pass. Production `30f8f784f`
pins the signed snapshot; its SDK contract gate passes. Comparator `f055afef7`
carries the same production change.

The four matched 64-frame supplied-owner arms and normalized-cost replays pass
under `~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-sdk-walk-01/`.
SDK checks are recorded alongside it in `t331-sdk-walk-tests.log` and
`t331-sdk-walk-snapshot.log`. Geometry and byte totals match the panel baseline.

| Workload | Files allocations/frame | IPC allocations/frame | Files CPU ns/MiB | IPC CPU ns/MiB |
| --- | --- | --- | --- | --- |
| 1920×24 bar | 678–689 | 613–625 | 31,219,538–31,582,479 | 27,298,406–27,687,930 |
| Same bar plus 4 MiB probe | 8,349–8,358 | 8,702–8,710 | 21,424,906–21,437,335 | 24,938,161–24,948,422 |

Panel allocations fall from the previous 709–721 range, but conservative
bounds do not establish +10% parity. Panel CPU exceeds that budget in this
diagnostic. Bulk bounds remain below IPC; their variation between runs is a
reason to retain paired measurements, not claim a fixed percentage gain.
These debug results do not settle native performance. The accounting proposal
still needs an explicit decision before acceptance collection; it excludes
other processes, startup and foreign allocation calls. No acceptance budget,
installed desktop or native evidence changed.

## Native latency summaries

External integration `b199ac0` adds `shell-timing latency HOST_LOG CLIENT_NDJSON
MODE_MILLIHZ EXPECTED_OFFERS`. It validates the complete journal and native
join before reporting nearest-rank p50, p95, p99 and maximum for presentation,
permit and recorded upload timings. All frames and raw metrics remain in the
report. Accepted uploads reaching 2,000,000 µs are listed explicitly; failed
or missing outcomes invalidate the whole report. Older journals' missing
upload timings are null, not zero.

Fourteen focused journal/native-join tests and strict xtask Clippy pass.
Replay of both calibration-04 arms reproduces their previously recorded native
presentation distributions exactly. Reports are retained under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/latency-replay-01/`.
This is all-frame reporting without warmup exclusion, pair comparison or
campaign acceptance. No new hardware evidence or accounting approval follows.

## t331: optimized diagnostic

Comparator `f055afef7`, SDK `f638e2f`, was profiled in the device-hidden
supplied-owner harness with `perf record --clockid mono -e cpu-clock:u -F 199
--call-graph dwarf,8192`. All four arms and cost replays pass. The retained
profile is under `t331-profile-01/` in the October 15 evidence root. Reports
restrict monotonic sample times to each arm's first frame start through last
frame end. No samples were reported lost. The panel arms have only 60 and 58
samples; iteration and the harness polling loop are prominent. This is too
little evidence for another production optimization, especially in a debug build.

The same source, feature, geometry, 64-frame workload and isolated environment
were then built with `--release` and run without perf. Both harness tests and
all four cost replays pass. Evidence and binary paths/hashes are retained in
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-optimized-01/`.

| Workload | Files allocations/frame | IPC allocations/frame | Files CPU ns/MiB | IPC CPU ns/MiB |
| --- | --- | --- | --- | --- |
| 1920×24 bar | 420–456 | 348–380 | 4,169,325–4,541,232 | 3,509,037–3,832,759 |
| Same bar plus 4 MiB probe | 2,022–2,046 | 1,928–1,945 | 1,567,640–1,579,243 | 1,268,293–1,276,909 |

The conservative bounds do not establish panel cost parity. Bulk allocations
are within +10% in this diagnostic, but bulk CPU is above it. Debug bulk CPU
had shown files below IPC: build mode materially changes the result. Use this
optimized baseline for further attribution, not the debug timing ordering.
The supplied owner still polls repeatedly and supplies presentation receipts;
this is not the native Session event loop or a B9 acceptance run. The proposed
accounting scope remains unanswered. No installed desktop or hardware state
changed. The comparator release peer was rebuilt with allocation counting;
old native plans must not be reused with this changed artifact.

## t331: native cost baseline preparation

After the owner accepted the scope and implementation plan, comparator
`f055afef78ef675ebdd020b373633374745a787a` was rebuilt in release mode with
`native-session,comparator-allocation-counting` for Sophia. Its generic peer
uses the matching allocation-counting build. External integration
`b199ac08c4eff904bb4420fdf639ec318daeb545` supplies the rebuilt release runner
and preflight. Build logs are `t331-native-cost-build.log` and
`t331-native-cost-runner-build.log` in the October 15 evidence root.

Four fresh plans under `t331-native-cost-01/` select files, IPC, files plus a
4 MiB probe, and IPC plus that probe, in that order. Each is 60 seconds,
1,000 offered frames, a 1920×24 bar on DP-1 at 1080p/60 Hz, scale 1 and VRR
off. Other outputs are disabled only within the test profile. All ten artifact
hashes and the signed source revisions are bound by each plan; no binaries
were copied into evidence. This is baseline collection, not the full paired
60/120 Hz acceptance campaign or a profiled run.

All four `prepare-native` calls and exact `--validate-session-args` checks pass
with devices hidden. The initial validation refused group-writable generated
profiles, then correctly refused the missing native opt-in. The corrected
profiles are mode 0600; the validation-only process received the required
opt-in inside the device-hidden sandbox. Both failed logs are retained.
No native process was launched and no display manager or installation changed.

`~/.local/bin/t252-check` now invokes these four plans from the attended tty4,
stopping at any failure. Its shell syntax check passes; launcher SHA-256 is
`7f9a9352aaf97b23346268bf6c5959955ad5c3ce07263ddbf136622d504a9b51`.
Previous calibration directories remain untouched. After collection, verify
every offer, upload, native join and recovery record before using the cost
bounds to select the next optimization. Arrange a fresh attended window; no
quiet-window hold is currently active.

## t331: atomic evidence records

The first files arm of `t331-native-cost-01` reached its end record declaring
1,000 offers and completions. Session reported bounded completion and clean
health; console recovery verified termios, KD mode and keyboard restoration.
However, the collected journal contains only 990 upload offers, 988 acceptance
records, 989 permits, 988 candidate offers and 994 terminals. The strict reader
fails with `missing or misattributed upload acceptance`; the launcher stopped
before IPC or either bulk arm. `captured_only` is not measurement acceptance.

The raw shared capture demonstrates the cause: at line 1310, the frame-12
upload acceptance appears between `stage=` and the remainder of a Session
counter record. `writeln!` streamed formatting fragments directly to stderr;
the other process could write between them. No missing timestamps or outcomes
have been reconstructed. The original capture remains retained and invalid
for cost or latency qualification.

Comparator `4801e2650` formats counter lines into a 1,024-byte stack buffer,
then writes the entire newline-terminated record at once. The peer similarly
serializes each JSON record into bounded stack storage first. Oversized records
fail before emitting a prefix; no new heap allocation is needed for formatting.
These records fit below Linux PIPE_BUF. The four counter tests include a
single-write/overflow regression and 1,000 counters concurrent with 1,000 JSON
records on one pipe. All pass. Strict focused Clippy passes, as do the two
optimized peer tests and all four cost replays in `t331-atomic-record-01/`.

Both release executables were rebuilt. Four new hashed plans under
`t331-native-cost-02/` use comparator `4801e2650` and integration `b199ac0`;
workload, output, mode and accounting match the first attempt. All preparation
and exact device-hidden argument checks pass. `t252-check` selects the fresh
directory and stops on failure. No second native run has started, no current
quiet-window hold is active, and the installed desktop remains unchanged.

## t331: native cost baseline results

All four attended `t331-native-cost-02` arms completed with comparator
`4801e2650` and integration `b199ac0`. Independent `cost-metrics` replay
matches the stored cost report exactly. Independent `latency` replay validates
all 1,000 offers per arm, every upload and permit, and each exact kernel-backed
presentation. No upload reached the two-second transfer bound. All four
console recovery records confirm restored termios, KD mode and keyboard mode;
Session reports bounded completion and clean health. Replays are retained as
`replayed-costs.json` and `replayed-latency.json` inside each arm directory.

| Workload | Files allocations/frame | IPC allocations/frame | Files CPU ns/MiB | IPC CPU ns/MiB |
| --- | --- | --- | --- | --- |
| 1920×24 bar | 4,285–4,290 | 1,708–1,711 | 11,424,436–11,433,719 | 7,417,317–7,429,519 |
| Same bar plus 4 MiB probe | 13,155–13,157 | 5,955–5,957 | 1,822,547–1,822,675 | 1,383,872–1,384,284 |

Each arm uploaded 184,320,000 presentation bytes. Probe arms additionally
uploaded 4,194,304,000 bytes. Both cost measures exceed IPC +10% in both
workloads within the accepted scope. This is one idle pair per workload at
60 Hz, not the full acceptance campaign.

Subtracting exact peer counter deltas from whole-workload bounds attributes
the allocation gap to Session. Peer allocations/frame are 173.990 on files
versus 124.994 on IPC for the panel, and 681.868 versus 658.994 with probes.
Session allocations/frame are 4,111.303–4,115.548 versus 1,583.999–1,585.861
for the panel, and 12,473.353–12,474.261 versus 5,296.960–5,297.967 with probes.
This attributes roughly 98% or more of the excess to Session, not a specific
function. The measured workload intervals are all about 16.7 seconds.

Session's full-run turn counts are 36,958/11,210 for files/IPC on the panel
and 123,869/48,380 with probes. Those counts cover the full 60-second runs,
not only the cost interval; they suggest extra service work but do not prove
a call-stack cause. Source review found nested readiness-vector construction
and whole-owner service around shell requests. Profile these paths before
choosing a fix; client buffer removal alone cannot eliminate this gap.

Panel submit-to-native-presentation p95/p99 are 16.507/16.525 ms on files and
16.835/16.860 ms on IPC. Probe arms are 12.183/12.233 versus 13.124/13.181 ms.
These instrumented all-frame distributions are diagnostic, not ordinary
latency qualification. No production optimization or installed release changed
as a result of this baseline. All four `t252-check` output directories are now
occupied and must not be reused.

## t331: measurement audit and Session changes

The counter reads atomics and formats into a 1,024-byte stack buffer. An isolated
test now verifies zero allocations and reallocations over repeated snapshots,
formatting and steady-state sampling after thread/channel initialization. The
benchmark previously constructed allocating JSON values for each event. Typed
records now preserve the same schema and escaped strings without allocating;
overflow writes no partial record. These checks do not establish zero CPU cost.

Allocation tracking adds atomic operations to allocations and deallocations.
Comparator `edcee33c5` therefore adds `comparator-cpu-counting`, which uses the
ordinary allocator. CPU-only reports contain null allocation results. Separate
`comparator-profiling` builds count owner, readiness, component, wire and logging
scopes in fixed storage. Samples carry monotonic bounds. Their CPU counters cover
the calling thread; allocation deltas cover the process while each scope is open.
Scopes overlap, and observations can straddle scope completion. They are
diagnostic, not exclusive stack attribution or acceptance measurements.

Production `083c33c1f` borrows readiness through the existing owners into inline
wait storage; larger inventories retain every descriptor through a heap fallback.
It borrows presented action bindings, retains one indicator publication after
comparing all current facts, compares the projected wire fields before cloning,
and reuses output bounds and reservation storage. Exact unchanged launch contexts
skip encoding after their first validation. Existing public facades, service
order, input processing, authority checks and resource bounds remain intact.
The comparator applies these shared changes to both files and restored IPC.

Focused checks measure 1,000 old readiness lists at 1,000 allocations versus zero
for borrowed visits. A one-indicator fixture measures 1,000 owned snapshots at
4,000 allocations versus zero for unchanged comparisons. Repeated unchanged
launch-context updates also allocate nothing. These are per-path allocation
checks, not native frames or whole-desktop CPU results.

Validation: eight server readiness tests, both isolated allocation tests, 30
policy projection tests, 1,046 Session tests (36 ignored), four catalog publication
and five queue tests pass. Two real-child comparator tests pass in each of the
allocation, CPU-only and profiling modes, covering both wires and both upload
workloads. All four retained allocation-mode 64-frame journals replay. Counter and phase checks,
CPU-only bounded-launch guards, external cost/native/journal tests and focused
strict Clippy pass. The first broad Session run inherited the user's core
configuration and lacked a private runtime directory: 19 tests failed before
their intended checks. The corrected device-hidden run sets an empty private
`XDG_CONFIG_HOME` and a mode-0700 `XDG_RUNTIME_DIR`; all 1,046 tests pass.

Evidence is under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-session-overhead-01/`.
The installed desktop is unchanged. Native CPU, allocation and profiling runs
remain separate; neither these tests nor the previous instrumented baseline
establish a production CPU win. The full B9 campaign and t252's other physical
acceptance gates remain outstanding.

The native pilot has twelve prepared, hash-bound arms in `t331-native-cost-04/`.
It uses Cargo's existing hashed executables in the comparator slot, keeping all
three measurement builds available without copying binaries into evidence.
Comparator `90988ff6d` changes only counter tests after `edcee33c5`; rebuilding
all three modes retains the recorded binary hashes. Preparation initially
refused a stale preflight hash (`t331-native-cost-03`), then an unarmed argument
check. Both failures are retained. After refreshing the rebuilt helper's hash
and supplying the wrapper's native opt-in to the device-hidden validator, all
twelve plans publish argument acceptance. No physical run occurred during these
checks. The helper offers one command for the whole pilot and mode-specific
four-arm subsets; it cannot resume into an already occupied run directory.

## t331: optimized pilot results

The October 15 pilot (`t331-native-cost-04`) pins comparator `90988ff6d` and
integration `14519aa`. All eight CPU/allocation arms joined 1,000 frames at
1080p/60 Hz. The first profiling arm also completed and restored the console,
but its 191,445-line capture exceeded the splitter's 100,000-line limit.
The helper stopped before the other three profiling arms. No native crash or
truncated capture was found. Original captures and runner results remain intact.

Independent replay in `cost-04/offline-replay/` extracts every JSON client record
and native record, preserving their order, and runs the existing collect,
metrics, cost-metrics and join commands. All nine captures pass. The eight
original reports match their replayed costs exactly; the recovered profiling
arm also joins all 1,000 frames. Recovery logs prove restored console and keyboard
modes without emergency recovery in every captured arm.

| Workload | CPU-only files / IPC (ms/MiB) | Allocations/frame files / IPC |
| --- | --- | --- |
| Panel | 11.572–11.586 / 7.378–7.385 | 3,756–3,762 / 1,543–1,545 |
| Panel plus 4 MiB | 1.8074–1.8077 / 1.3771–1.3774 | 11,381–11,385 / 5,232–5,234 |

Allocation counts use separate Rust-global runs. Relative to `cost-02`, files
allocations fall about 12% for the panel and 14% for bulk uploads; IPC also benefits
from the shared changes. Files still exceed IPC by about 143% and 118% on
allocations, and 57% and 31% on CPU. These pairs fail the +10% ceiling; they do
not establish a CPU improvement over the old allocation-instrumented baseline.
The recovered profiling arm measures 13.938–13.950 ms/MiB and 3,744–3,748
allocations/frame. Its CPU overhead is why profiling remains separate from cost
acceptance. Phase scopes overlap and cannot be summed as exclusive costs.

Integration `4eb52fb` raises the capture record ceiling to 500,000 while keeping
the 64 MiB total and 64 KiB per-record bounds. A regression test covers profiling
volume, excessive record counts and oversized individual records. All ten native
runner tests, formatting and diff checks pass. The signed fix is pushed.

Before the pilot, the owner authorized stopping five obsolete t064 processes;
two DOSBox children required KILL after TERM. Specula processes were untouched.
The Mac mini agent held crunch work and transfers; the hold is now released.
`quiet-window-preparation.txt` records the cleanup. This is pilot evidence, not
the full three-pair 60/120 Hz idle/load campaign.

Three missing profiling arms are prepared in `t331-native-profile-05`: IPC panel,
files bulk and IPC bulk. Plans bind the same comparator and the rebuilt runner
at integration `4eb52fb`; all pass device-hidden argument validation. The short
`t252-check` helper selects only these fresh directories. Complete those profiles
before attributing the remaining costs or selecting another production change.

## t331: completed profiling comparison

All three `t331-native-profile-05` arms completed with 1,000 native joins and
clean console recovery. Independent collect, journal, cost and native-join
replays match their original reports. Combined with the recovered files-panel
profile in `cost-04`, this completes the pilot's twelve cases. The quiet hold
is released. The Mac mini agent may run the owner's authorized GPU training on
crunch; a later measurement window must coordinate a pause first.

`profile-05/phase-comparison.json` records diagnostic phase deltas near each
workload interval. Owner passes number 33,053 on files versus 8,049 on IPC for
the panel, and 120,050 versus 45,049 for bulk uploads: about 4.1 and 2.7 times
as many. Owner-scope thread CPU is about 1,618 versus 785 ms for the panel and
5,485 versus 3,666 ms for bulk. These are profiling observations, not CPU-only
acceptance values. Scopes nest; allocation deltas include other process threads
while a scope is open. Never sum them or treat them as exclusive stack costs.

Readiness scopes report only 2–17 allocation operations per workload. Measured
Session logging scopes report about 2,000 operations and 8 ms of thread CPU in
each arm. These observations direct investigation toward repeated owner-pass
services; they do not prove that every additional pass is unnecessary or that
all logging is covered. The wire phase has no earlier nonzero sample and its
diagnostic delta therefore uses a zero baseline, explicitly flagged in the
derived report. It must not be compared as an exact workload-only cost.

The next bounded slice is to inspect repeated Session work, prove individual
allocation sources with deterministic checks, and carry shared fixes to both
comparator paths. No scheduler change, larger I/O budget or wire-contract change
is admitted by these observations. The CPU-only and allocation gate remains
failed at the values recorded above.

### Primary-output lookup

Source inspection found a complete `OutputAuthoritySnapshot` clone in the
cadence lookup on every owner pass, solely to read its primary output ID.
Production `d18775828` reads that ID directly from the authoritative published
snapshot. Startup cadence and present-clock admission use the same accessor.
There is no retained cache: a committed primary-output change is immediately
visible. The owned snapshot facade remains available to topology operations.
Comparator `8293249f6` carries the identical change for both wires.

The isolated two-output allocation fixture measures 8,000 allocations for 1,000
old inventory clones and none for direct reads. It also replaces the published
topology and verifies the new primary. Nine output-authority tests pass in both
production and the profiling comparator; all 21 device-hidden present-clock
tests pass in production. Focused strict Clippy, formatting and diff checks pass.
The first compile caught an unqualified `OutputId` in the private facade; the
qualified protocol type fixes it. No native measurement or installed-desktop
change follows this patch yet, so it has no measured per-frame saving or CPU win.

## t331: finer owner-service profiling

The first profile separated component and wire service but left most owner
allocations in a broad scope. Source inspection found additional services on
every pass, without enough evidence to rank their allocation costs. Comparator
`e45744623` therefore adds five diagnostic scopes: preparation before component
service; post-component controls and topology; lifecycle and retirement; WM work
and the second clock visit; and authority/input service, including its wait.
Early exits close their guards normally. Production service order and budgets
are unchanged; these hooks exist only in the comparator profiling build.

The sampler writes all ten phase rows every eighth 2 ms counter tick and at
startup and shutdown. Cost-counter cadence stays unchanged. Phase observations
are coarser, roughly 16 ms, but the expanded record set stays bounded without
raising capture limits. Tests verify zero allocation for phase guards and
record writing, zero steady-state sampler allocation, and both phase endpoints
even when the sampler stops before its first periodic phase publication.
Counter tests and focused strict Clippy pass. Both release artifacts are rebuilt.

The pinned SDK's `files/uploads.rs` explains a structural source of extra turns:
upload-slot walk and open depend on replies, and only one current slot write is
awaiting a reply before the next write can advance. The driver is bounded and
handles short writes and EAGAIN explicitly. This explains why extra turns can
be legitimate; it does not assign an exact fraction of the measured pass gap.
The current slice preserves that ordering, resource custody and all limits.

Four matched diagnostic arms in `t331-owner-profile-06` bind comparator
`e45744623`, including the primary-output fix, and integration `4eb52fb`. Each
offers 1,000 frames at 1080p/60 Hz: files and IPC, panel and panel-plus-4-MiB.
The helper now selects only these fresh directories. Device-hidden argument
validation passes for all four. No production speedup is established by this
preparation, and the cost gate remains failed at the last CPU-only results.

Claude's crunch setup/download stopped cleanly at 01:49 UTC on October 16;
training had not started. Its quiet-window confirmation is retained alongside
the plans. The diagnostic build completed before the proposed native window.
The next step is the attended four-minute run, then independent replay and
phase attribution before another production patch. Phase allocations still
cover all process threads while each scope is open; scopes overlap and must
not be summed as exclusive costs.

## t331: finer profile results

On October 16, all four `t331-owner-profile-06` diagnostic arms completed on
comparator `e45744623` and integration `4eb52fb`. Each has 1,000 exact native
joins, matching independent collect/metrics/cost/join replay, and clean console,
termios and keyboard recovery. The evidence root is
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-owner-profile-06/`.
`campaign.json` binds the plans; `offline-replay/` holds the independent reports;
`phase-comparison.json` records source-log hashes, interval bounds and deltas.
The `captured_only` and `joined_only` statuses are diagnostic results, not B9
acceptance verdicts.

| Workload and wire | Allocations/frame | Profiling CPU ns/MiB |
| --- | ---: | ---: |
| Panel, files | 3,579–3,582 | 14,411,370–14,421,089 |
| Panel, IPC | 1,503–1,505 | 8,031,711–8,040,750 |
| Panel plus 4 MiB, files | 10,775–10,778 | 2,328,918–2,329,151 |
| Panel plus 4 MiB, IPC | 5,007–5,009 | 1,607,398–1,607,862 |

The primary-output fix is present, but instrumentation also changed. These
numbers cannot establish its individual native saving. Profiling CPU cannot be
compared with the earlier ordinary-allocator CPU arms as an improvement or
regression. The last acceptance-comparable CPU results still miss the gate.

The new scopes narrow the next investigation:

| Workload and wire | Owner passes | Allocations during lifecycle | Allocations during authority service |
| --- | ---: | ---: | ---: |
| Panel, files | 33,047 | 1,374,476 | 1,557,769 |
| Panel, IPC | 8,044 | 505,024 | 629,292 |
| Panel plus 4 MiB, files | 120,040 | 4,150,933 | 4,524,322 |
| Panel plus 4 MiB, IPC | 45,712 | 1,714,041 | 1,929,337 |

Preparation and readiness each observe fewer than 130 allocations per arm;
WM observes fewer than 14,000. Post-component service observes roughly three
allocations per pass. Files still takes about 4.1 times as many owner passes
for the panel and 2.6 times as many for bulk uploads.

These are completed-scope deltas from approximately 16 ms snapshots bracketing
the workload. A completed scope can cross a boundary. CPU measures the calling
thread; allocations count the whole process while a scope is open. Nested
scopes overlap, so do not sum them or attribute their counts to exclusive stacks.

Source inspection finds `runtime.service_native` in both lifecycle native
service and authority timeout service. It performs completion, retained-image
and frame-reducer work even when the owner has no new authority batch. Inspect
its unchanged-state allocations next and reproduce a concrete cause in an
isolated test. Preserve service order, deadlines, retirement and authorization;
the profile does not justify skipping native service or changing scheduling.
No further production patch follows these measurements yet.

Claude reports no crunch activity during this window after stopping its setup
at 01:49 UTC, except a Herdr confirmation. The hold is now released; setup and
GPU training may resume. Another native measurement requires fresh plans,
directories and quiet-window coordination. t331 and t252 remain open.

## t331: established 9P implementation comparison

On October 16, the owner requested an implementation comparison alongside the
local profile. The following upstream source reads identify techniques, not
portable benchmark results. Links name moving branches; recheck the code before
implementing a recommendation.

- Linux's [9P client](https://github.com/torvalds/linux/blob/master/net/9p/client.c)
  allocates request records from a slab cache, sizes message buffers to the
  operation, and uses a transport-specific zero-copy path for larger reads and
  writes when available. Its ordinary write loop still waits for each reply
  and advances by the returned count. This is evidence for selective copy
  avoidance, not a claim that Linux pipelines every write or that virtio's
  zero-copy mechanism transfers directly to Unix sockets.
- QEMU's [9P server](https://github.com/qemu/qemu/blob/master/hw/9pfs/9p.c)
  reuses PDU records through free and active lists. Its payload handling uses
  I/O vectors, although creating a subrange vector still allocates metadata.
  Sophia can examine bounded request-storage reuse without adopting QEMU's
  coroutine execution model or changing Session scheduling.
- The gVisor-derived [Go p9 client](https://github.com/hugelgupf/p9/blob/main/p9/client.go)
  pools response records and dispatches replies through a pending-tag map.
  Its [transport](https://github.com/hugelgupf/p9/blob/main/p9/transport.go)
  pools encoding buffers and sends header, encoded fields and payload as
  separate vectors. That avoids joining the payload into one application
  buffer; it does not imply kernel zero-copy on every transport.

Sophia already has bounded tagged pipelining in the vendored SDK's
`sophia-9p-client/src/pipeline.rs`, and server-side direct upload reception in
`crates/sophia-9p/src/connection/in_place.rs`. The latter rechecks authorization
before returning each destination slice. Preserve that property; a reusable
buffer or retained request must never retain stale authority.

The concrete opportunities, in order of scope:

1. Continue removing proven per-pass temporary collections in Session/native
   service. This is independent of transport and benefits both comparator wires.
2. Measure client byte movement. `files/uploads.rs` drains acknowledged prefixes
   from `remaining`; `pipeline.rs` copies write payloads into `out_buf` and drains
   sent prefixes. Partial progress moves unsent tails. Test an offset cursor and
   bounded storage reuse first, then consider vectored writes with explicit
   payload custody. Preserve byte charges, partial writes, EAGAIN, disconnect,
   flush reserves and sensitive-output scrubbing. SDK changes belong upstream
   and reach Sophia through the normal vendor gate.
3. Separately evaluate concurrency across independent admitted uploads. The SDK
   pipeline supports it, but `Current::SlotWrite` holds one outstanding write
   tag. Walk must establish a fid before open; open must succeed before use;
   partial writes and resource status govern safe progress. Multiple tags alone
   do not prove that writes to one slot can be reordered. Any proposed change
   needs ordering, revocation, cancellation and fairness tests before admission.

Do not increase `msize`, queue limits or per-turn budgets, cache upload fids, or
introduce transport threads under this comparison. Measure copied bytes, syscall
counts and owner passes as well as accepted CPU/MiB and allocation metrics. The
current profiles cannot assign an exact fraction of the gap to any upstream
technique. Research does not close t331 or establish an architectural defect.

## t331: bounded storage and copying results

Production `7c8fd76c4` removes owned head-index lists from read-only native
observations and replaces the frame-service reducer's temporary tree sets with
sorted inline storage under its existing 16-output ceiling. Enabled-head
filtering, physical order, mirrors, validation errors, effect ordering and
attempt budgets are unchanged. Mutable paths retain their owned index facade.
There is no topology cache and no skipped service call.

An allocation fixture run against the old comparator intentionally fails its
zero-allocation assertion: 1,000 one-output service turns allocate 7,000 times.
The revised reducer allocates nothing at one, two or sixteen outputs, including
reordered observations and retirement/submission attempts. A separate selection
fixture measures 1,000 owned-list allocations against zero borrowed allocations,
and checks disabled heads, changed output membership and 33 heads without a new
head-count limit. These tests do not construct native devices.

SDK `bcf840a` retains upload bytes and advances the acknowledged offset, and uses
a cursor for ordinary pipeline sends. Partial sends no longer move the unsent
tail. Appends compact only when needed; a secret write first normalizes the
buffer and keeps the existing reserved-capacity scrubbing path. SDK `25b63ff`
moves the outbox's existing boxed payload into the active file operation. The
outbox retains its original byte charges, control classification and ticket
until settlement. Refused preparation occurs before payload transfer. IPC's
borrowing path and the pipeline's required owned payload copy remain intact.

Deterministic tests verify unchanged storage through partial sends, exact unsent
bytes across header/payload boundaries, compaction under the output bound, a
secret following partially sent ordinary bytes, pointer identity on payload
transfer, and held admission charges with reserved control capacity. A scripted
upload interleaves short acknowledgements and EAGAIN, checking every resulting
byte and offset. The SDK workspace suite and strict Clippy pass.

The vendor gate first caught a pre-existing stale SDK contract copy: it lacked
the accepted cost-accounting section. Signed SDK `d8558f4` synchronizes that text
and its checksum; the gate then passes. Production `a9a92746a` imports it.
Comparator `ff2a52621` carries the native change onto both wires, and `a2ecde0ef`
imports the same SDK and cost scope. No restored IPC adapter enters production.

Ten frame-service behavior tests, both native service-observation tests and 112
mirror-related tests pass. Focused native Session compilation, strict Clippy,
layout and diff checks pass. Seven device-hidden runtime test binaries pass,
covering file-wire service, content/session lifetime, transport, output custody,
reconnect budgets and independent C/Go exports. The backend allocation fixture
initially lacked the required renderer feature; rerunning with `gbm-probe` and
`libdrm-events` passes. No native device was needed for these checks.

Evidence is in
`~/.local/state/sophia/development-evidence/t331-storage-20261016/`: the old and
new reducer results, SDK and integration logs, pinned upstream sources and
release artifact identities. Build artifacts remain in Cargo slots, not copied
into evidence. Both release builds use the exact comparator revision.

Eight one-minute, 1,000-frame pilot arms are prepared under
`t252-authenticated-20261015/t331-native-cost-07`: files/IPC and panel/bulk in
separate CPU-only and Rust-allocation modes. Integration is `4eb52fb`. The first
preparation used an older preflight hash and refused it; the replacement binds
the same preflight as successful `profile-06`. Parser validation initially lacked
the native opt-in environment variable; all final device-hidden validations
include it and return before runtime construction. Both failed preparation
records remain beside the plans. No native arm has started. Arrange a new quiet
window before the attended run; this pilot is not the full B9 campaign.

The separate [concurrency investigation](gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md)
is recorded as t332, a candidate after t331 measurements. Native CPU improvement
and per-frame allocation savings remain unproved for these changes.

## t331: storage pilot results

The attended `t331-native-cost-07` pilot completed all eight arms on comparator
`a2ecde0ef`, SDK `d8558f4` and integration `4eb52fb`. Independent collect,
metrics, cost and native-join replay passes for each arm, with 1,000 frames and
the expected uploaded bytes. All captures report `captured_only`; replay
establishes sample validity, not a B9 pass. Recovery records restore termios
and keyboard mode without emergency recovery.

| Workload | Files CPU ms/MiB | IPC CPU ms/MiB | Files allocations/frame | IPC allocations/frame |
| --- | ---: | ---: | ---: | ---: |
| Panel | 11.419793–11.427201 | 7.762774–7.771587 | 2,327–2,330 | 1,142–1,144 |
| Panel + 4 MiB | 1.773713–1.773943 | 1.384933–1.385123 | 6,492–6,495 | 3,384–3,385 |

CPU comes from ordinary-allocator runs; allocations come from separate
Rust-global runs. Files CPU remains about 47% above IPC for the panel and 28%
for bulk. Allocation counts remain roughly twice IPC. The +10% ceiling fails.
Against `cost-04`, files allocations fall about 38% and 43%, while CPU falls
only about 1.3% and 1.9%. These are single-pilot observations, not confidence
intervals or isolated attribution to individual patches. Both wires received
the shared fixes; IPC allocation reductions are expected too.

Raw captures, plans and `offline-replay/summary.json` remain under the cost-07
evidence directory. The summary binds source-log hashes, normalized bounds,
join counts and recovery records. Claude confirmed the quiet hold from 10:28Z
until release, reporting only a brief read-only SSH log check before 10:28:17Z;
its next crunch command was after release at 10:48Z. No system-wide silence is
claimed. The hold is released and all run directories are occupied.

The next investigation should attribute remaining CPU to repeated owner work
and copying before choosing another patch. Large allocation reductions alone
have not removed the CPU gap. t331 remains open; neither full B9 nor concurrent
upload implementation is authorized by this pilot result.

## t331: CPU profiling preparation

The owner authorized CPU profiling after cost-07's large allocation reduction
produced little CPU improvement. Four one-minute arms cover files/IPC and
panel/bulk on the same binaries, source revisions and workload as cost-07.
The fresh `t331-native-perf-08/campaign.json` binds prepared plans and exact
commands. A short `t252-check` launcher wraps the existing attended runner in
`perf record --clockid mono -e cpu-clock:u -F 997 --call-graph dwarf,8192`.
All foreground-console, artifact, bounded-runtime and recovery checks remain
in the runner. No native profile has started at preparation.

The existing binaries retain symbols and unwind tables; no rebuild is needed.
A non-native perf smoke capture succeeds without lost samples. All four
device-hidden argument validations pass. The first validation refused a
group-writable copied profile; its mode was corrected to 0644 and the failed
validation retained. No device was opened by that check.

Replay the journals and native joins before interpreting profiles. Select the
Session and peer process IDs from their counter records, restrict monotonic
sample times to each workload interval, and report sample counts, losses,
unknown symbols and unwind limitations. Exclude launcher and companion-process
samples from attribution. Compare self and inclusive CPU shares on both wires;
do not treat inclusive stacks as additive. Investigate repeated owner work,
copying and protocol processing before proposing another implementation slice.

This is user-space on-CPU sampling. It cannot directly attribute kernel work,
scheduler delay or time blocked off CPU. Capturing all inherited child processes
adds diagnostic overhead, so the resulting CPU costs are not acceptance data.
Use a separately scoped follow-up if these stacks cannot explain the gap.

## t331: CPU profile results

All four `t331-native-perf-08` arms completed with 1,000 exact native joins,
matching journal/cost replay and clean terminal and keyboard recovery. Analysis
uses each workload's first-to-last counter envelope and only Session and peer
processes, including their threads. The peer logs PID 1 inside its namespace;
host peer PIDs were identified from stacks containing the exact peer binary.
They are 14318 (files panel), 15309 (IPC panel), 16301 (files bulk) and 17287
(IPC bulk). Source revisions and binaries match cost-07.

| Workload | Files samples | IPC samples | Files unknown leaf | IPC unknown leaf |
| --- | ---: | ---: | ---: | ---: |
| Panel | 1,256 | 748 | 36 | 44 |
| Panel + 4 MiB | 5,473 | 4,160 | 44 | 41 |

All reports show zero lost samples. Sampling at 997 Hz gives roughly one
millisecond per sample, not exact function timings. About 17–19% of selected
samples have only one stack frame; most memcpy samples lack caller attribution.
Unknown leaf samples include driver code. Optimized/inlined code and the 8-KiB
stack limit further constrain attribution. The profile covers user-space CPU,
not kernel execution or off-CPU waits. Its CPU counter results are diagnostic,
not replacements for cost-07 acceptance-comparable values.

The main differential lead is repeated Session work. Session's main thread has
869 versus 496 samples for panel, and 3,049 versus 2,133 for bulk. The peer has
268 versus 134 and 2,305 versus 1,919. Native `service_native` appears in 160
versus 102 panel stacks and 344 versus 141 bulk stacks. `LaunchCustody::is_live`
appears in 44 versus one panel stack and 86 versus zero bulk stacks. Source
inspection shows this check polls both kernel identities with zero timeout;
its correctness includes immediate exit observation and active-flag withdrawal.
These counts justify inspecting repeated checks, not caching away revocation.
9P server turns, file-wire service and client pipeline work also appear more
often. Inclusive counts overlap and must not be added as independent costs.

The largest bulk leaf is a vector construction in `shell_timing_peer::main`:
1,619 files samples versus 1,674 IPC samples have that caller. The source builds
the same pixel pattern per frame. Generation precedes each upload-offer timestamp
but later frames' generation remains inside the whole-workload envelope. It is
shared benchmark work, not a demonstrated 9P defect. Likewise, the iterator
under `ContentResourceStore::chunk` has 666 versus 603 leaf samples; source
inspection identifies premultiplied-alpha validation of every pixel. Preserve
that validation. Optimizing either common cost must reach both comparator wires
and cannot be presented as a transport-specific improvement.

Prioritize a bounded investigation of repeated owner turns and custody polls.
Trace the callers and prove any redundant work with deterministic fixtures
before changing it. Keep copy attribution unresolved where stacks terminate
inside libc; use a focused follow-up if that blocks a specific proposal. No
production code or upload scheduling changed during this profiling step.

Evidence is `t331-native-perf-08/analysis/`: replay reports, source stack text,
filtered self reports, `summary.json` and `profile-summary.json`. The latter
retains perf hashes, host peer PIDs, per-thread counts, self/inclusive leaders
and unwind diagnostics. The quiet hold began at 10:57Z and was released after
all four captures ended. All plan output directories are now occupied.

## t331: repeated-service investigation

Inspection after perf-08 found three distinct boundaries, not a reusable
authentication result. `ShellFileWire::service` polls authentication before and
after a turn or publication flush. `AuthenticatedFiles::check` validates each
operation. Guarded output uses `with_delivery`, including partial writes, so
revocation after queuing cannot leak protected bytes. `WorkerAuthority::live`
also checks the active flag and Factotum ingress on both sides of the kernel
custody poll. Removing or caching these checks is not justified by the profile.

Grouping the sampled custody callers makes the distinction concrete:

| Custody caller | Panel samples | Bulk samples |
| --- | ---: | ---: |
| Authentication poll around shell service | 20 | 44 |
| Operation checks while receiving/resuming requests | 19 | 21 |
| Waiting-read retries after socket writes | 5 | 20 |
| Policy-worker authentication poll | 0 | 1 |

These sum to the 44 and 86 files samples reported above. They are samples of
user-space execution in the custody check, not syscall counts or its kernel
cost. `analysis/custody-callers.json` preserves the stack groups.

One concrete retry amplification deserves a deterministic experiment:
`Server::write` calls `resume` and then `retry_waiting` after every successful
socket write. `retry_waiting` revisits every pending read that has reply room;
`read_now` checks authority before asking the export for data. Thus sending an
unrelated acknowledgement can recheck a pending event read even when the event
source has not changed. This explains a caller, not yet a removable check:
resumed requests can change the export, and freed reply capacity can unblock a
read. Publication wakes and revocation must also retry pending reads.

The next experiment should use a counting export and real socket backpressure
to distinguish reads waiting for data from reads waiting for reply capacity.
Count retries across unrelated acknowledgements, request-driven data changes,
publication wakes, partial writes, flush, revocation and disconnect. Only then
propose suppressing a retry for which neither data nor capacity changed. Keep
the authority check on every actual read and delivery attempt; do not add a
launch-liveness cache or weaken idle detection. Any retry state must remain
bounded by the existing tag limit and preserve arrival order.

The two native-service call sites are also not interchangeable: lifecycle
service settles earlier work, while authority service follows new state and
repaint decisions. A once-per-loop skip would need a state-change proof and
could strand a new presentation. Leave both calls intact while investigating
their unchanged-state work. No production patch follows from this inspection.

## t331: pending-read counting fixture

`crates/sophia-9p/tests/pending_read_retries.rs` exercises the driver over real
Unix sockets and the core's separate reply-capacity boundary. A static export
counts actual read calls; its optional sink-write publication models a request
changing data visible to a waiting reader. Five tests establish:

- 100 unrelated getattr replies cause 100 extra attempts at a pending event
  read. A later publication wake delivers the event exactly once.
- Revocation is checked before reading: a queued event remains private and the
  pending request gets ESTALE. Disconnect releases the connection.
- A partial output drain that does not make enough reply room must not consume
  the event. Once sufficient room is freed, the read completes without another
  publication.
- A sink write can publish an event without a wake. Both its acknowledgement
  and the pending read complete in the same server turn. Unconditionally
  removing post-write retries would break this valid behavior.
- Real socket backpressure preserves the published reply until the socket
  drains. Tflush cancels a subsequent waiter; later publication cannot revive it.

The retry count is printed as diagnostic evidence, not asserted as a contract
that would forbid an improvement. The fixture asserts the observable progress,
privacy and cancellation behavior. All five tests, the full `sophia-9p` suite
and focused strict Clippy pass with eight build jobs/test threads. These are
device-free correctness checks, not CPU measurements. Production code is
unchanged; no new native run is required to establish these results.

The result rules out a generic skip based only on reply type or lack of a wake.
A possible next design is an opt-in export change token, with the existing
unconditional behavior as the default. An opted-in export must cover all state
that can change a pending read, including request-driven changes, publication,
revocation and time-dependent readiness. Reply-capacity progress remains a
separate retry trigger. Authority checks remain mandatory before actual reads
and guarded delivery; idle custody polling remains independent. Audit the shell
export's mutation and expiry paths before adopting such a contract. This is a
design lead, not an implemented optimization or a measured saving.

## t331: change-token audit

The owner authorized continuing from the counting fixture. Auditing the actual
shell export does not justify implementing the proposed token yet.

`Journal::read` checks the retention floor, tail and count, then returns Pending
immediately when the requested offset equals the tail. It allocates no buffer
and scans no retained records on that path. A content token could avoid this
small amount of work, but would add its own state, dispatch and invalidation.
No measured saving supports that tradeoff.

More importantly, journal state is not authority state. Namespace or endpoint
revocation, Factotum ingress closure and wrapper/peer exit can happen without
changing journal offsets. `RoleFileAuthentication::check` checks admission and
the admitted verdict; `WorkerAuthority::live` checks ingress and kernel custody.
Checking that authority before comparing a token leaves the observed poll cost
intact. Comparing the token first and skipping the check changes revocation
observation. A content-only token therefore cannot provide both the proposed
custody-poll reduction and the preserved check boundary.

This narrows the fixture's conclusion: 100 unrelated replies cause 100 read
attempts, but it does not establish 100 safely removable custody checks or a
material CPU saving. Keep the behavioral tests and current production behavior.
The earlier token paragraph records a design lead, superseded by this audit's
no-change decision. No new protocol, cache, retry state or native run was added.

The next useful attribution should distinguish user-space owner service from
kernel poll/read/write cost. Perf-08 cannot do the latter because it sampled
`cpu-clock:u`; syscall call counts alone would still not establish time spent.
Use task-scoped kernel-inclusive sampling if host permissions allow it, or a
separate syscall-duration diagnostic with its tracing overhead stated. Retain
the unprofiled cost-07 measurements as the performance comparison. Do not spend
another implementation slice on pending-read tokens without stronger evidence.

The kernel-permission probe confirms `perf_event_paranoid=2`: plain
`cpu-clock` silently records `cpu-clock:u` (`exclude_kernel=1`), and explicit
`cpu-clock:k` is denied. No setting changed. Four proposed task-scoped
`cpu-clock:uk` commands reuse the perf-08 plans with fresh `t331-native-perf-09`
outputs. The owner subsequently approved temporarily changing the setting from
2 to 1, then restoring 2. The installed helper prompts for sudo on tty4 and
restores 2 with an exit trap, including catchable interruptions. SIGKILL cannot
run a trap, so verify the setting after the run. The processes being profiled
still run as the normal user. A private kallsyms snapshot supports offline
kernel symbol attribution. No setting has changed or native run started yet.
A fresh quiet window is requested. This permits kernel sampling for owned
processes; it is not system-wide sampling.

## t331: kernel profile results

Perf-09's four captures include both kernel and user execution (`cpu-clock:uk`,
997 Hz); all have zero reported lost samples. Journal and cost replay and the
1,000 native joins per arm match the runner's reports. The host profiling
setting is restored to 2 and the quiet hold is released. Claude reports only
11:25Z process/Herdr coordination and Muse's finishing handoff relay, with no
later crunch work through release; the review itself finished before the hold.

Analysis selects Session and the peer's host PID during each whole-workload
counter envelope. Host peer identities come from stacks containing the exact
peer binary; namespace PID 1 is not used as the host selector. The retained
private kallsyms snapshot has nonzero addresses, but this perf version still
prints kernel frames as unknown. Offline analysis resolves those addresses
against the preceding text symbol in that snapshot, rejecting distances of
64 KiB or more. All selected kernel leaves resolve under that rule. Aliases,
optimized functions and absent function-size information limit exact attribution.
No live host setting was changed for offline analysis.

| Workload | Files user ms | IPC user ms | Files kernel ms | IPC kernel ms |
| --- | ---: | ---: | ---: | ---: |
| Panel | 1,214.6 | 726.2 | 775.3 | 574.7 |
| Panel + 4 MiB | 5,345.0 | 4,186.6 | 2,166.5 | 1,548.6 |

These are summed sample periods across each 1,000-frame workload, not exact
function runtimes or acceptance measurements. Selected sample counts are
1,984/1,297 for files/IPC panel and 7,489/5,718 for bulk. Approximately 71% of
the panel difference and 65% of the bulk difference lies in user space. Kernel
work matters, but does not explain the whole CPU gap. Cost-07 remains the last
unprofiled comparison.

The strongest identified kernel difference is polling. Kernel samples with
`__x64_sys_ppoll` on the stack total 173.5 versus 20.1 ms for panel and 660.0
versus 73.2 ms for bulk. This measures on-CPU polling work, not sleeping time
or syscall counts. The files bulk poll samples split by nearest application
caller into client `Pipeline::wait_for_io` (235.7 ms), custody `is_live`
(181.5 ms), server `turn` (126.4 ms), and owner `sophia_wake::wait` (115.3 ms).
For files panel those values are 29.1, 54.2, 46.1 and 42.1 ms respectively.
This supports inspecting readiness handling independently of authority checks.

Socket receive/send kernel stacks are not uniformly worse for 9P: bulk
recvfrom samples are 450.4 versus 620.9 ms and sendto 320.0 versus 415.2 ms.
Do not replace this evidence with a blanket claim that 9P copies more in the
kernel. Kernel stacks without an identified syscall frame total 587.8/305.9 ms
for bulk and 278.8/306.9 ms for panel; these remain a separate category.
Counter-writer work also appears in all profiles and is not attributed to the
transport. Inclusive stack categories must not be summed as independent costs.

Source inspection confirms `Pipeline::poll` directly performs bounded
nonblocking I/O; despite its name, it does not call poll(2). Its `wait_for_io`
does. The server's `turn`, however, always calls its own readiness wait, even
when the embedding owner has already waited over the borrowed descriptors.
That is the next concrete inspection target: determine whether validated
readiness can be passed through without a second zero-time poll, preserving
socket identity, interest changes, wake/stop handling and the per-turn budget.
Do not assume every server poll is redundant: there are standalone and
owner-internal callers too. Preserve the existing path for those callers.

Evidence is in `t331-native-perf-09/analysis/`: `kernel-profile-summary.json`,
each arm's raw and resolved stacks, `kernel-categories.json`, and
`ppoll-callers.json`. Raw perf hashes and per-thread contributions are retained.
This profiling step changes no production code and does not close t331.

## T331 readiness handoff pilot results

Cost-10 uses comparator `377b0413d` and integration `4eb52fb`, with the same
1,000-frame panel and panel-plus-4-MiB workloads as cost-07. CPU and allocation
instrumentation remain separate. Results include Session and the peer.

| Workload | 9P CPU ms/MiB | IPC CPU ms/MiB | 9P allocations/frame | IPC allocations/frame |
| --- | ---: | ---: | ---: | ---: |
| Panel | 11.318320–11.330453 | 7.792920–7.804655 | 2330–2333 | 1140–1142 |
| Panel plus 4 MiB | 1.809023–1.809336 | 1.387622–1.387732 | 6500–6503 | 3385–3387 |

At interval midpoints, 9P panel CPU is 0.87% below cost-07 and bulk CPU is
1.99% above it. The paired IPC results changed by +0.41% and +0.19% respectively.
9P remains about 45.2% above IPC for the panel and 30.4% above it for bulk, failing
the +10% ceiling. Allocation counts are effectively unchanged. These are single
pilot comparisons; counter-bracketing intervals are not statistical confidence
intervals and do not prove a small improvement or regression.

All eight arms report `captured_only`, with no capture error. Independent `collect`,
`metrics`, `join` and `cost-metrics` replay matches the captured journal, sample
and cost reports exactly. Each joins 1,000 frames. Recovery logs confirm restored
text mode, termios and keyboard mode, with no emergency recovery. Evidence and
source hashes are in `t331-native-cost-10/offline-replay/summary.json`, beneath
the evidence directory named in Current resumption.

Claude confirmed the quiet window from 12:05 UTC. Its only crunch activity was
the 12:05:05 Muse status read and confirmation message; Muse stayed idle. The
hold was released after capture and acknowledged. The first release message was
rejected while Claude's pane was blocked; a retry succeeded after it resumed.

The handoff is correct in deterministic tests, but this pilot does not qualify
it as an optimization. Measure its actual use and fallback frequency before
another candidate. No upload-concurrency change is authorized by this result.

## Connections

- [t252 exit](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t252--join-and-accept-the-shell-path).
- [t328 delivery audit](jxcqbd04-audit-remaining-topology-and-delivery-obligations-before-architecture-acceptance.md).
- [Source-retirement decision](../decisions/twkn9fsp-retire-wm-and-shell-ipc-with-release-rollback-while-latency-qualification-remains-open.md)
  preserves performance and physical acceptance separately.
