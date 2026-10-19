---
id: 2o32e7ui
date: 2026-10-16
kind: investigation
status: investigating
tags: [investigation, 9p, performance]
---
# Which Plan 9 implementation techniques can reduce Sophia shell costs

## Question

Which maintained Plan 9 and 9P implementations offer techniques worth testing
against Sophia's remaining costs? niltempus requested this comparison after the
mixed t331 readiness pilot. Maintenance does not establish a speed ranking.

## Current resumption

Candidate 27 now has an implementation candidate, not a measured win. Each
native head retains one successful primary-plane property-ID discovery,
keyed by opened-card identity, target generation and connector/CRTC/plane
IDs. Card identities are non-reused process-local tokens; even a cloned card
gets a fresh token. This covers device replacement in an otherwise unchanged
public group slot without relying on raw fd numbers. Topology preparation
invalidates all entries before its fallible work, including failed attempts
and rollback; seat resume constructs a new native owner.

Only successful name-to-ID discovery is retained. A private device wrapper
delegates live value reads, resource creation, cleanup and commits unchanged;
I/O errors through that wrapper invalidate the entry. Singleton and mirrored
ordinary-frame paths use it, including mirror submission. Uncached callers
and topology preparation keep fresh discovery. Caching changes where a
disappearing target's error may first be observed: resource/commit I/O still
fails normally, but a warm frame no longer performs the metadata-read ioctl.
No cached metadata grants authority or proves the device is usable.

Eight focused controls cover bounded reuse, each identity/generation change,
replacement, missing/failed discovery retry, current values, out-fence
delegation, atomic errors and framebuffer/commit cleanup. Disabling reuse
fails the control with 3000 discovery calls versus 3. Backend `test-support`
tests pass (975 passed, 9 ignored across 45 targets), strict clippy and scoped
format checks pass, and the native-session CLI compiles. Minimal
`libdrm-events` clippy is blocked by the unchanged renderer's unresolved
`sophia_renderer_native_egl` import; its independent no-default-feature
check reproduces that failure. No hardware acceptance or allocation/CPU
saving is claimed. Evidence: `candidate-27-property-cache/` beside the audit
below. Keep this a candidate pending matched native measurement on both
transports; t331 remains open with the same red panel gate.

### Candidate 27 source and retained-profile audit

Candidate 27 identified repeated DRM property discovery from the retained
c19 recording without another capture. Across each 1000-frame interval,
files made 81,000 `GETPROPERTY` and 6,000 `OBJ_GETPROPERTIES` calls; IPC made
81,209 and 6,018. Decoded command counts reconcile with the frozen reports'
96,157/96,308 total ioctl entries. Primary-plane property discovery appears
in 122/1581 files Session samples and 112/1200 IPC samples. These are
instrumented counts and sampled attribution, not durations or savings bounds.

The source repeatedly converts each object's properties to full metadata
and allocated names, then keeps only names and IDs for a prepared scanout.
This is shared rendering work, not an explanation of the files-minus-IPC
gap. Reducing it on both sides could lower absolute CPU while increasing
the percentage gap; never optimize just one arm of that comparison.

The bounded candidate is to retain successful property-name-to-ID discovery
with the owning device and scanout target. It needs explicit invalidation
for replacement, topology/selection changes, seat revoke/resume and reused
IDs, and must not retain failed discovery or cache live property values.
No cache was implemented: the existing cursor-handle precedent alone does
not prove primary-plane invalidation or error-observation equivalence.
Tests must cover same-target reuse, replacement, multiple heads/devices,
missing-property retry and unchanged atomic commit/cleanup failure paths
before native measurement. Authority checks and ISB custody stay untouched.

The c12 post-fix allocation evidence also rules out reviving the old preview
list: preview preparation was already nearly allocation-free in that
workload. Per-scope allocation totals cannot be interpreted as allocations
on every idle pass because they include frame work and other threads.
Evidence and the source/test proposal are in `candidate-27-property-discovery/`
under the evidence root below, especially `FINDINGS.txt`, `results.json`,
decoded ioctl rows and `analyze.py`. No SDK or production behavior changed.

### Candidate 26 local MAP_POPULATE screen

Candidate 26 stopped at local screening: `MAP_POPULATE` showed no total
buffer-lifecycle benefit. Three optimized standalone Rust comparisons used
64 warmups and 512 alternating-order pairs at each size, preserving fresh
memfds, fallocate, size seals, DONTFORK, whole-pixel fill, unmap and final
seals. Panel lifecycle medians rose 4.95–6.01%; 4 MiB medians rose
6.37–6.60%. These are local wall times, not native CPU or a native regression
claim. No SDK, comparator or production implementation changed, and no
native screen is warranted by this result.

The fault controls correct an important prediction: ordinary mappings
recorded 45 panel or 1024 bulk minor faults during fill; POPULATE recorded
the same counts during creation and zero during fill. It moved the recorded
faults rather than eliminating them. Full content readback and refusal of
writes after sealing passed in both arms. Linux documents population as
best effort, not a guarantee that mmap fails when population fails or that
later faults cannot occur. Historical sampled attribution is not a savings
ceiling.

Evidence: `candidate-26-map-populate/` under
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/` contains
the standalone source, constant checks, successful `local-02..04.log`,
`results.json`, `analyze.py`, `FINDINGS.txt`, build commands and checksums.
The diagnostic uses libc calls with the SDK's syscall order; it is not the
SDK implementation. Failed toolchain and readback-control attempts remain
recorded separately and are excluded from results. No memory-pressure or
fork test was performed. Mutable buffer reuse and per-turn liveness caching
remain outside this slice. Keep t331 open, t340 deferred and experimental
SDK read scheduling disabled; the candidate-23 panel gate remains red.

### Candidate 25 withdrawal

Candidate 25's native screen completed, but the fresh Ready-listener poll
fallback showed no CPU benefit. Combined new-minus-old CPU was +3.42–3.68% in
old/new/new/old and +1.23–1.45% in new/old/old/new. Session alone rose about
50–54 microseconds/frame in the first block and 6–9 in the second. The second
block's variation exceeds its mean shift; do not claim a causal regression or
pool the blocks to conceal drift. Neither block demonstrates a saving.

Withdraw the candidate fallback and its added idle-visit fixture block from
both active source trees. Preserve the older borrowed-readiness optimization,
all authority checks, and the shared whole-pixel fill. The local syscall test
still proves that the candidate replaced empty accepts with polls, but its
per-call savings did not establish native benefit. The frozen candidate
`4f2a86722`, its production source `b00aa2300`, binaries and all evidence remain
retained; the failed screen is not erased or reinterpreted.

All eight runs joined 1000 unique presentations and accepted uploads each,
with 184,320,000 logical bytes per run, no bulk probe, all outcomes presented,
and zero major faults. The 520-second launch and PAM/tty4 cleanup succeeded.
Only Session differed; the whole-pixel peer was identical across runs. Median
presentation spacing stayed 16.683–16.684 ms. This is CPU screening, not an
allocation measurement, dedicated latency gate or B9 acceptance.

Evidence is in `candidate-25-listener-probe-screen/` under
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/`, especially
`FINDINGS.txt`, `results.json`, `analyze.py` and `POSTRUN-SHA256SUMS`. Preserve the
original receipts and completed `lp25-1..8` stages. The candidate23 panel gate
below remains red. Keep t340 deferred and SDK read scheduling disabled. Next
work needs a larger source-grounded candidate; do not repeat this screen or
weaken checks, ACKs or custody to manufacture a passing result.

### Current panel gate remains red

Candidate 23 completed all six retry captures and cleanup on October 19. Every
idle-60-Hz panel CPU pair fails the IPC +10% threshold. Files/ISB excess is
27.30–27.67%, 30.44–30.78%, and 28.83–29.11% in pairs 1–3. These are conservative
counter-attribution bounds, not statistical confidence intervals. Equal uploaded
bytes make CPU/MiB and CPU/frame ratios identical. Do not pool away any failure.

The sequence was files/IPC, IPC/files, files/IPC, using identical whole-pixel
peer and Session executables. Each run joins 1000 unique presentations and
accepted uploads, exactly 184,320,000 logical bytes, with all outcomes presented
and zero major faults. Files combined CPU is 1.567–1.619 ms/frame versus IPC
1.228–1.240 ms/frame. Session contributes about 210–230 microseconds/frame of
the excess and the peer about 129–151. Median presentation spacing is unchanged;
this CPU screen supplies neither allocation results nor latency acceptance.

Keep the shared fill, leave experimental SDK read scheduling disabled, and
keep t331 open. Defer the full qualification matrix while this condition is
red. Next optimization work should use the retained candidate-19 profiles and
candidate-16/17 syscall correlations to select a bounded change in Session
service work or client pipeline work. Those are historical attribution leads,
not proof of the cause of every microsecond in this screen. Preserve all fresh
authority checks, custody, ACKs and deadlines; no transport redesign is implied.
A candidate needs deterministic proof and a measured screen before promotion.

Candidate 24 tested one concrete peer lead locally: replace std socket wrappers
with the existing rustix dependency, without changing syscall cadence. Four
alternating release repeats show only 43–64 ns less per empty receive and
37–43 ns less per send-plus-peer-read cycle. Historical call counts suggest
only a few microseconds/frame, not a native saving or an explanation for the
remaining peer gap. Do not promote a wrapper-only change or launch another
native campaign for it now. The SDK diagnostic is signed `0c5c6c8`; empty-read,
data, half-close and failed-write controls pass, as do strict Clippy and format
checks. Library behavior, C contracts and all production pins are unchanged.
Logs and limits are in sibling `candidate-24-socket-wrappers/`. This narrows
the search toward repeated service work and kernel costs; a sampled wrapper
frame alone does not establish removable overhead.

Evidence is in `candidate-23-panel-cpu-pairs-retry/` under
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/`, especially
`FINDINGS.txt`, `results.json`, `analyze.py` and `POSTRUN-SHA256SUMS`. The launch
finished in 389 seconds, PAM cleanup succeeded, no residual processes/sessions
remained, and tty4's login prompt was restored. Do not rerun these stages.

The original attempt remains a preparation failure, not a performance result:
its 0664 desktop profile was refused with `Io(UnsafeMode)` before Session
startup. The retry uses private permissions and passed both profile-only and
Session-argument validation as uid 1000, including a refusal control against
the original file. Preserve that package and failed `panel23-1` stage too.

### t340 — Direct rustix socket wrappers

Deferred at niltempus's request while higher-impact t331 work proceeds. The
candidate replaces only the Rust pipeline's std socket wrappers with rustix
recv/send, preserving every I/O attempt, byte/call budget, error class, partial
I/O result, EOF observation and secret-buffer handling. Send must retain
SIGPIPE suppression. Review supported-platform behavior and C parity before
any implementation; an internal Rust-only wrapper cost does not require a
literal C port.

Candidate-24's local diagnostic (`0c5c6c8`) found 43–64 ns per empty receive and
37–43 ns per send-plus-peer-read cycle. That suggests only a few microseconds
per frame at historical call counts, not native savings. Revisit only after
larger costs or stronger evidence justify it. Exit requires deterministic
pipeline/role regression coverage, documented parity, and a matched native
CPU comparison showing benefit without custody or latency regression before
default adoption. No production library change or pin is currently authorized
by this deferred row alone.

### Whole-pixel native bulk result

Candidate 22 completed all eight bulk CPU captures on October 19. Whole-pixel
fill reduced mean peer CPU by 19.1% on files/ISB and 19.9% on IPC: about
0.66 ms/frame on each wire. Combined CPU fell from 6.379 to 5.712 ms/frame on
files (10.44–10.48%) and from 6.548 to 6.043 ms/frame on IPC (7.70–7.74%).
The percentage ranges reflect counter attribution, not statistical confidence.
Peer old/new ranges are disjoint and the changes exceed within-arm spreads.
Files Session CPU overlaps; IPC Session CPU rose about 150 microseconds/frame,
close to its 141-microsecond new/new spread, so its cause remains unresolved.

Each run joins 1000 unique presentations and 2000 accepted uploads, exactly
4,378,624,000 logical bytes. All outcomes are presented, major faults are zero,
and peer minor faults are unchanged. The 520-second launch finished within its
600-second bound; PAM cleanup and tty4 restoration passed. Original receipts
and the harmless openvt deallocation warning are retained.

Submit-to-present median latency increased about 0.5–0.7 ms. The interval from
the previous completion to submission shortened by approximately the same amount;
median presentation spacing stayed 16.683 ms in every run. Source inspection
confirms that fill and timestamp placement did not move. Faster preparation
followed by a longer wait for the refresh is consistent with these observations,
but latency acceptance remains open. Do not erase the increased residence metric
or declare a net qualification win from this CPU screen.

Retain the shared fill in subsequent matched comparator work. It benefits both
wires and is not a transport-specific improvement. Next are matched panel and
bulk qualification, separate allocation and latency evidence, and repeatability
controls. The experimental SDK scheduling feature stays disabled; production
SDK pins and the published ISB architecture are unchanged. t331 remains open.

Evidence, analysis and hashes are in `candidate-22-whole-pixel-native/` under
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/`, particularly
`FINDINGS.txt`, `results.json`, `analyze.py` and `POSTRUN-SHA256SUMS`. The sequence
was old/new/new/old within files, then within IPC, with one Session executable.
All 75 peer compiler units match in features and profiles. The plan checkout is
`b3f13dad6`; old Session/peer source is `d7eae929f`, new peer source `b3f13dad6`.
Exact artifact identities are in `matched-builds.json`. Initial unused panel
preparations remain superseded. Do not rerun or overwrite completed stages.

### Whole-pixel local result

Candidate 21 implements safe whole-pixel filling in the comparator's shared
canvas routine, signed as `b3f13dad6`. Both IPC and mapped ISB use it. Fixed-size
four-byte chunks let the compiler broadcast one packed BGRA pixel instead of
repeatedly calculating byte positions; partial prefixes preserve the previous
behavior. No unsafe code, intrinsics or SDK library changes are involved.
The paired SDK diagnostic is signed as `220e2fd`.

Three alternating local runs show full fresh-buffer preparation medians about
19% lower for panel images and 20% lower for 4 MiB. These are local wall times,
not native CPU savings or an ISB-versus-IPC win. Both versions already vectorize;
the generated whole-pixel loop is simpler (169 versus 746 bytes on this build).
Two SDK differential controls, five actual canvas tests and six rebuilt peer
controls pass, with scoped Clippy and formatting clean. Final evidence is in
`candidate-21-whole-pixel-fill/` beside the earlier directories below. No native
capture has run for this change. Future comparisons must rebuild both arms
with the shared fill; the production Sophia SDK pin is unchanged.

### Current-baseline attribution and first-touch controls

Candidate 19 completed its current-baseline kernel/syscall profile on
2026-10-19. Files/ISB and comparison IPC use the same symbolized comparator
`d7eae929f` binaries with experimental SDK scheduling disabled. Both captures
pass the pinned report checks, including event attributes, interval/TGID
binding and loss checks, and each joins 1000 unique presentations. The fixed
launcher completed PAM cleanup, reported no residual processes and restored
getty. It correctly classified the remaining openvt deallocation message as
a warning. The new launcher also passed 18 offline checks and a PTY control
that fails with the original self-counting query.

The fresh profile identifies peer image preparation as a useful next target:
210 of 591 peer CPU samples include `sealed::prepare`, with 77 passing through
`fallocate`. The pipeline has 191 inclusive samples and image ticks 92.
Session has 1581 samples, including 151 in custody liveness checks, 127 in
`service_native`, 82 in image service and 38 in pixel validation. These counts
overlap and must not be added. Authority checks remain intact.

Tracing is material: 341 Session and 136 peer samples contain selected perf
tracing frames (IPC: 189 and 29). Do not subtract those to invent uninstrumented
durations or savings ceilings. This diagnostic pair is not an acceptance run.
Raw reports, reconciled stack counts, capture hashes and four flamegraphs are
under `candidate-19-current-hotspots/` in
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/`.
The first stack parser failed before producing attribution; its corrected
version reconciles all four totals with the independent profile reports.

Candidate 20 now measures that local decomposition through unchanged SDK
APIs. The ignored diagnostic is signed as `92948e1` in the Rust SDK; no library
code, C contract or production pin changes. Three release repeats show panel
create/fill/finish/close totals of 130–155 microseconds (median per repeat).
Fresh fill alone is 62–75 microseconds versus 26–34 for the same compiled fill
on an already populated ordinary canvas. A separate untimed per-thread fault
control reports exactly 45 minor faults in panel fill and 1024 for 4 MiB,
with zero in the other stages and reused-fill control. Full 4 MiB totals are
2.663–2.728 ms. These are local wall times, not native CPU savings.

Evidence is in sibling `candidate-20-mapped-stage-cost/`, with final data in
`fault-fill-{1,2,3}.log` and `results.json`. Existing ISB controls, strict Clippy
and formatting pass. The reused canvas is a diagnostic control, never an ISB
replacement. First-touch handling is now a measured target, but a candidate
must reduce total cost rather than merely move faults into creation. Preserve
fresh backing, preallocation, size seals, DONTFORK and verified final seals.
Any native performance claim still needs repeated uninstrumented comparisons.

### Candidate 18 screening result

Candidate 18 completed its four-run CPU screen on 2026-10-19 (`sc18-1`
through `sc18-4`, control/candidate/candidate/control). Each run joined 1000
unique presentations. Combined CPU averaged 1.599–1.601 ms/frame for control
and 1.645–1.647 ms/frame for the candidate: nominally 2.73–2.96% higher.
The controls drifted by about 3.8%, so these two runs per arm establish neither
a saving nor a causal regression. There is no demonstrated benefit to promote;
the feature stays disabled and the production SDK pin stays unchanged. This is
screening evidence, not a B9 verdict or a statistical bound on effect size.

All captures and PAM cleanup completed, and outer cleanup proved tty4 and its
sessions empty before restoring getty. The launcher nevertheless refused the
inner residual-PID receipt. A disposable-PTY reproduction confirms that the
inner `ps -t tty4` query counts its own subprocess. A separately retained fix
filters the exact query PID and passes both idle and live-child controls.
The historical PID's command was not recorded; the original failed receipt
has not been rewritten. No rerun is needed solely for this cleanup-check bug.
Evidence, raw capture hashes, CPU bounds, corrected helper and PTY controls:
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/candidate-18-sdk-read-scheduling/post-run-review/`.
The corrected helper has not been exercised as a root native launcher.

### Candidate 18 preparation and qualification limits

Candidate 18 is a disabled-by-default Rust SDK experiment at `2574d72` on
`performance/t331-reply-storage`. The feature
`experimental-write-read-scheduling` lets a shell drive defer a socket read
only after the preceding round reached WouldBlock without receiving bytes
and the current round successfully wrote bytes. Every drive starts with a
normal read; deferred rounds clear the history, so deferral cannot chain.
Generic pipeline polling and other role clients retain their default behavior.
Sophia's imported SDK pin is unchanged.

Device-hidden checks pass: 268 feature-enabled pipeline and shell tests,
84 default FileWire tests, the earlier 909 default workspace tests,
strict Clippy in both modes, and formatting. Eight deterministic pipeline
controls cover deferral, productive and partial reads, final replies before EOF,
failed writes and shared budgets. Disabling deferral makes its distinguishing
control fail. Logs remain in `candidate-18-sdk-read-scheduling/`.

Three new FileWire controls pass in both modes. Zero I/O credit deliberately
leaves queued input unread, then a later dispatch verifies Submitted custody,
ACK confirmation and EAGAIN backoff calculated from observation time. A corrected
closure expectation matters: the EOF intake drive can return `Ok` while settling
the final Submitted event; receive and readiness queries then report PeerClosed.

This is not qualified for default enablement. Pipeline tests establish the
mechanism and public FileWire tests establish the delayed-input contract. The
dispatch control establishes bounded progress and immediate interest, not the
exact deferred-write/close interleaving or feature-specific service-pending
difference. Those integration limits remain explicit; no background-thread
timing race is treated as deterministic proof. The SDK documents observable
scheduling changes rather than claiming identical call timing. Applicable Sophia
export gates and a fresh feature-on/off CPU comparison remain. No native candidate-18
capture or performance claim exists. The C dispatch has no equivalent repeated
socket-poll cost; no C implementation change is proposed. ISB, 9P records,
authority checks and ACK content remain intact; t331 stays open.

The comparator-only import is signed at `d7eae929f`; production retains its
existing SDK pin. Both feature modes pass the comparator controls and strict
Clippy, and both release peers pass six device-hidden peer controls. Four fresh
files CPU plans are prepared in control/candidate/candidate/control order in
`candidate-18-sdk-read-scheduling/ready.json`. They share one Session binary and
the same SDK revision; only the peer's experimental feature differs. Build
receipts verify the feature on both SDK crates. This screen may proceed with
the integration limit above; it cannot qualify default promotion. Native runs
have not started. The SSH launcher requires sudo for a real PAM tty4 session;
Session and the capture remain unprivileged.

The final plans use separate peer builds: all 75 compilation units match after
removing only the experimental feature. The first combined Session/peer build
unified extra dependency features; its unused preparation is retained as
superseded. The replacement control again passes six peer controls. The remote
launcher passes 16 offline tests and pin validation, with VT-child timeout and
verified process/session cleanup before getty restoration. Entry on crunch is
`sh ~/.local/state/t331/run-sdk-scheduling-remote.sh`; sudo authentication is
required from the SSH session. No privilege or native test has been attempted.

## October 19 send-aware peer diagnostic

The send-aware peer diagnostic on comparator `bc3acf69e` is valid: 1,000
unique joined presentations, matching supervised peer PID 20466, unchanged
input hashes, successful control and native tracer exits, and clean PAM/tty
restoration. All 73,458 receives and 25,984 sends reconcile across interior,
boundary and outside bins. No interior cross-FD receive contexts, unbracketed
shell sends, unexpected receive/send outcomes or zero-receive polls occurred.

Of 27,764 later empty polls in fully included 100 ms bins, 24,867 (89.6%)
followed a successful send in that poll. The rest followed a productive
previous poll. SDK `c6ad4fa` repeats its full write/read poll after local
progress; the dominant empty retries therefore follow real request writes.
The proposed rule to read only on entry or after a productive previous poll
would cover 10,850 observed empty polls and 668 observed productive polls.
This is exposure, not predicted savings: deferring those replies changes
later rounds, current-call disconnect observations and custody settlement.
No production change follows from the counts alone. A candidate needs
bounded-progress, reply/EOF and custody controls before a CPU comparison;
preserve ACK progress, deadlines and the published ISB/9P contract.

Evidence: `candidate-17-peer-send-polls-retry/analysis.json`, original traces
and `CAPTURE-SHA256SUMS`. Poll bins use return time; syscall bins use syscall
timestamps. Drive histograms cover the whole run. Uprobes perturb scheduling;
there is no CPU or latency acceptance claim. The original candidate-17 run
stopped in a faulty histogram assertion before native launch and is preserved.
The retry checks histogram frequencies and passes a replay of that control.
The launcher now accepts the exact console-deallocation warning only after
successful child, tracer and PAM cleanup receipts; the retry exercised it.

Keep listener-readiness candidate `c535a0310`. Accept calls fell from
23,024 / 23,066 image-service visits to 8,034 / 23,176, a 65.3% reduction
per visit. Authority and receive calls remain about 99.7% of visits.
Evidence: `candidate-14-listener-ready/profile-check/listener-count-analysis.json`.
Its earlier fresh files-only ABBA screen showed a
1.92–2.15% mean combined CPU reduction and roughly 21–25 microseconds/frame
less Session CPU, with run drift and a lower unchanged-peer cost that prevent
a firm causal estimate. The new instrumented timing is not additional CPU
acceptance evidence. t331 remains open; the published ISB/9P architecture,
authority checks, ACK progression and deadlines are unchanged.

## October 19 listener-readiness CPU screen

The listener-readiness candidate (`c535a0310`, comparator `bc3acf69e`) passed
its four-run files-only CPU screen. All four runs join 1,000 unique presented
frames without capture errors; plan, artifact and source pins were verified.
Control/candidate/candidate/control combined CPU is respectively
1.725–1.728, 1.675–1.677, 1.639–1.640 and 1.657–1.659 ms/frame.
The candidate mean is 1.92–2.15% lower, with Session CPU about 21–25
microseconds/frame lower. This is a positive screening signal, not a proven
causal reduction: costs drift across the sequence and unchanged peer CPU
also falls about 11 microseconds/frame on average. Counter brackets are not
confidence intervals. t331 remains open; full repeated qualification is absent.

Evidence: `candidate-14-listener-ready/native-screening-analysis.json` plus
hashed raw captures. Preserve the candidate on the performance branch, without
calling the remaining panel gap solved. The next discriminator is the accept
call reduction under profiling, or a repeat of the fresh files-only control;
SDK read-loop changes still require EAGAIN/data/EOF measurements first.
The temporary source-only control worktree can now be removed; its signed
revision and external artifact hashes remain recorded in the evidence.

## October 19 listener-readiness implementation and screening

The listener-only readiness candidate is implemented on the performance
branches; native CPU benefit is still unmeasured. A fresh owner-wait
observation can skip one accept probe only in Ready state with no backoff.
The exact server identity/generation/connection must still match, followed by
the listener descriptor/epoch/channel generation at service time. Missing,
stale or mismatched observations fall back to probing. Flushes, handshakes,
fresh custody checks, receives, final-packet/HUP handling and ACKs are unchanged.
A duplicate arriving after an idle poll stays queued until the next visit;
readiness is never authorization. No SDK or wire protocol changes are involved.

Real-socket tests cover Waiting, late duplicate arrival, one-shot consumption,
stale observations, absent/wrong listener descriptors and same-visit revocation.
Disabling the skip fails the late-duplicate assertion; restoring it passes.
Image controls (5), owner turn (1), transport readiness (3), Session reconnect
(10), strict Clippy and scoped formatting pass. Claude's read-only source review
found no safety blocker. Evidence: `candidate-14-listener-ready/`.

Next run files control/candidate/candidate/control with CPU-only instrumentation.
The control reuses signed `8c7479085` binaries and a detached source-only worktree
at `~/dev/sophia-t331-listener-control`; remove that worktree after analysis.
This four-run screen does not qualify t331. Keep the candidate only on evidence;
do not infer its CPU benefit from the preceding instrumented accept timings.

## October 19 image-service diagnosis

The image-phase pair is complete and valid: both arms join 1,000 unique
presented frames with matching plan, revision and artifact hashes and no
capture errors. Files records image_accept 80.044 ms / 23,024 calls,
image_authority 40.043 ms / 22,997 calls, and image_receive 33.853 ms /
22,997 calls. IPC has no image-channel calls. The enclosing wire_image is
209.633 ms / 23,066 calls, versus 131.96 ms in the preceding less-instrumented
pair. Nested guard overhead and run variation prevent treating that difference
as application work, or the child sums as predicted savings. Detailed evidence:
`candidate-13-image-service/image-service-attribution.json`.

The next candidate should target listener accept probes using a fresh,
one-shot owner-wait observation, before considering broader SDK changes.
Retain the duplicate/unauthorized-join drain on readiness, fall back to the
existing probe without a valid observation, and bind observations to the
registered descriptor and channel state. Do not remove or cache custody
checks. Any later receive gating must preserve final-packet-before-HUP
handling. The available fraction of observed owner visits still limits the
saving; measure a CPU-only candidate/control pair before claiming improvement.
The earlier pending-capture statements below are historical.

Read-only analysis of the retained candidate-03 kernel pair confirms 86,307
peer recvfrom entries on files versus 12,006 on IPC, but all entry addresses
resolve to libc's syscall wrapper: exact Rust caller counts cannot be recovered
from these tracepoints. Of 580 files CPU samples, 20 have receive functions
on their stacks and all 20 pass through the 9P Pipeline; none pass through the
image adapter. These overlapping, instrumented samples support investigation
of pipeline reads, not a count of empty reads or a duration estimate. The
SDK drain loop can poll the pipeline repeatedly per drive, and each pipeline
poll reads until blocked or budget-limited. A later diagnostic must distinguish
data, EAGAIN, EOF and retry rounds before altering this behavior. The small
SDK image-readiness cache remains deferred. Evidence is retained as
`candidate-13-image-service/peer-recvfrom-attribution.json` and
`peer-receive-cpu-attribution.json`. The image-phase native pair is still pending.

The preview-idle allocation saving is confirmed, but its CPU screen shows no
speedup: files remains 29.51–29.77% above IPC. t331 is open. Stop optimizing
empty preview bookkeeping; preserve the published 9P/ISB and authority model.

Comparator `63d6687c9` now splits image-channel service into `image_accept`,
`image_authority` and `image_receive` scopes. The latest parent scope measured
131.96 ms across 23,160 visits per 1,000 frames. The new scopes distinguish
listener/receive probes from mandatory fresh custody checks before considering
readiness handoff. They nest inside `wire_image`; receive excludes source
admission, and Hello/publication work remains in the enclosing remainder.
This is diagnostic-only instrumentation, with no production behavior change.
Counter and image-transport controls, strict scoped Clippy, feature-off check
and scoped formatting pass. Evidence and analysis limits are retained in
`candidate-13-image-service/`. The next native attribution pair measures the
available opportunity; it is not CPU acceptance. Claude is reviewing SDK probe
costs read-only; no SDK change is authorized by these measurements alone.

## October 19 preview bookkeeping screens

The preview-idle CPU screen is complete. Files costs 1.668899–1.671071 ms
per frame against IPC's 1.287682–1.288642 ms, or 29.51–29.77% more. The
session contributes about 228–231 microseconds of excess and the peer about
152 microseconds. Reaching the ten-percent threshold would require another
251–255 microseconds/frame in this pair. Both arms have 1,000 unique presented
frames, no capture errors, and verified plan, artifact and revision pins.
Evidence: `candidate-12-preview-idle/cpu-screen/native-screening-analysis.json`.

The allocation reduction below is confirmed, but there is no demonstrated
CPU benefit. The preceding SIMD screen was 25.85–26.07% above IPC; comparing
these separate pairs does not establish a regression or a causal effect.
Do not count preview-idle as a CPU win or spend further effort on empty
preview bookkeeping. The next investigation should target the larger
wire-service and peer costs, preserving fresh authority checks and the
published ISB architecture. t331 remains open; these are screening results,
not repeated qualification runs. Earlier next-step text below is historical.

The preview-idle candidate is signed and published as production `ebde050a9`
and comparator `8c7479085`. The follow-up attribution pair confirms the
intended allocation reduction: files preview preparation falls from 67,059
operations over 33,522 calls to 12 over 33,536; IPC falls from 41,806 over
20,894 calls to 15 over 21,002. Retirement, nested inside preparation, falls
from roughly one allocation per call to five total on files and six on IPC.
Whole-owner allocations fall by 67,166 on files and 40,833 on IPC, with
slightly different pass counts; these totals are diagnostic, not a controlled
CPU result. Both captures match their plan hashes and join 1,000 unique
presented frames without capture errors. The retained analysis is
`candidate-12-preview-idle/preview-attribution-analysis.json`.

Next is a CPU-only panel screening pair. The allocation result does not
establish a CPU improvement or qualify t331. The following paragraphs retain
the baseline and implementation rationale.

The attribution pair has now completed. Its preview preparation scopes report
67,059 allocation operations over 33,522 calls on files, and 41,806 over
20,894 calls on IPC. Retirement accounts for approximately one allocation
per call inside preparation. Demand construction records only eight and two
allocation operations respectively, so an unchanged-demand clone guard is
not a useful target in this workload. These process-wide counts include
workers; the nested scopes must not be added together.

The next candidate skips donor inventory construction when demand is empty
and skips the retired-import traversal after weak-entry pruning leaves no
requests. Epoch synchronization and obsolete snapshot retirement remain
unconditional. Production backend tests pass (354, one ignored), comparator
preview tests pass (38), and strict library Clippy passes in both checkouts.
Evidence is in `candidate-12-preview-idle/`. The allocation saving still
needs a native attribution pair; no CPU improvement is claimed. The earlier
measurement plan below is retained as context and has now been executed.

The fixed-block ISB validator remains accepted in production `cfa256424` and
comparator `914e86378`. The client-drive experiment is retired. Latest native
screening leaves panel CPU 25.85–26.07% above IPC, while bulk is 5.37–5.41%
below IPC. These single pairs do not qualify t331; details and limits follow.

The next step measures the remaining panel cost rather than applying an
unmeasured bookkeeping fix. Comparator `39524e005` adds feature-gated scopes
for preview demand construction, preparation and retired-import service.
Retirement nests in preparation, and both nest in lifecycle_native; do not add
these totals. Demand also runs during policy installation. Allocation deltas
are process-wide and can include workers. The optional backend-to-runtime
diagnostic dependency exists only in this comparator profiling feature.

A new ignored timing control uses the production `LaunchCustody::is_live`
against a real protected wrapper and peer. Two optimized runs measure median
0.436 and 0.432 microseconds/check. Multiplying by the stale profile's roughly
110 checks/frame suggests about 48 microseconds/frame, not the full remaining
208–210 microseconds/frame gap. This warm isolated estimate is neither a native
attribution nor permission to cache authority or remove checks.

Evidence is in `candidate-11-panel-cost/`. Custody timing, scoped Clippy,
38 preview controls and counter controls pass; the profiling-disabled backend
check passes with `test-support`. A bare no-feature backend check still fails
on the renderer's missing native EGL import, outside this patch. Scoped
rustfmt finds pre-existing formatting in preview_preparation and its included
fixtures; the diagnostic additions do not reformat those files. The next
attended files/IPC attribution pair will use existing wire/authentication/owner
scopes plus the three new preview scopes. It is diagnostic, not acceptance.

## October 18 ISB scan and control screening

Publication batching is retired in production `78d7d062b` and comparator
`8cfeac2a7`. The controlled native pair showed approximately seven actual
flushes per frame with batching either enabled or disabled, unchanged protocol
read/write counts, and no clear CPU benefit in the earlier cost pilot. Inline
output observations remain: their screening result reduced files allocations
by 6.5–6.8% and IPC allocations by 4.8–5.1%. The latest cost screening still
misses the ceiling: CPU +37.31–37.72%, allocations +19.10–19.45% against IPC.
That cost screening included batching; it is not qualification of the retired
candidate or a new measurement of the remaining code.

The removal passes 965 Session tests, runtime owner/custody controls, comparator
controls and scoped strict Clippy. The schema-4 service ledger remains solely
in the comparator, with `wire_flush_deferred` retained as a zero field for
historical replay. Evidence and exact sampling brackets are in
`candidate-06-service-ledger/service-ledger-analysis.json`; removal checks are
under `candidate-07-service-ledger-control/remove-batch-*.log`. Both captured
arms have 1,000 unique joined presentations and no capture error. The temporary
batched source worktree has been removed; published commits and captures remain.

The next isolated candidate replaces the worker's byte-at-a-time premultiplied
BGRA scan with aligned pixel comparisons accumulated in blocks of sixteen.
The comparator candidate is `613fcb288`; the matching production change is in
this checkpoint. Partial-pixel edges retain the original predicate, and each
unit still checks at most 64 KiB. Failure remains sticky, and progress advances
only after the whole unit succeeds. Worker deadlines and cancellation checks,
9P control/events, the descriptor side channel, and authority checks are unchanged.

Differential controls compare every step against the frozen scalar predicate,
including arbitrary budgets, invalid RGB components, cap boundaries and 512
seeded random buffers. Existing resource, worker, shell/lock image transport
and cancellation controls pass, as does strict runtime Clippy. An optimized
warm-mapping experiment over 4 MiB reports a 51.2–52.0% median scan-time
reduction across two invocations and opaque/mixed-alpha patterns. The quieter
repeat measures approximately 1.338 ms scalar versus 0.652 ms block scan.
Generated x86-64 code remains scalar: one alpha load per pixel and accumulated
RGB comparisons. This is not a native CPU or latency claim. Evidence, source
hashes, timing limits and codegen are under
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/candidate-08-isb-scan/`,
particularly `scan-analysis.json`, `controls.log`, `comparator-controls.log`,
`benchmark-repeat.log` and `scan-disassembly.txt`.

The attended CPU-only screening pair completed on comparator `613fcb288`.
Both arms have 1,000 unique joined presentations, matching 184,320,000 logical
image bytes and no capture error; retained plan and artifact hashes match.
Files costs 1.615–1.617 ms/frame against IPC's 1.244–1.245 ms/frame, an excess
of 29.67–29.93%. The remaining difference is 242–244 microseconds in Session
and 128 microseconds in the peer. Reaching the +10% ceiling still requires
approximately 245–248 microseconds/frame of combined savings. Captures are
`~/.local/state/t331/scan-cpu-{C,A}-01`; the checked results are in
`candidate-08-isb-scan/native-screening-analysis.json`.

This screening gap is smaller than the earlier 37.31–37.72%, but that older
pair included publication batching and there is no matched native pre-scan
control. Do not attribute the entire improvement to T1. The isolated scan
experiment supports the scan change itself. This is one panel pair without
environment readback, allocation measurement or full latency qualification.
The source-layout audit also reports two pre-existing inline-test violations
in backend-live; both offending files match the pre-scan source byte-for-byte.

The client-drive experiment `c4b0056d7` is retired by comparator `7449a114a`.
Both native arms completed 1,000 joined presentations without capture errors,
but files CPU was 1.658–1.661 ms/frame against IPC's 1.239–1.241 ms/frame
(+33.68–34.08%); peer CPU was 404 microseconds, versus 402 in the preceding
scan pair. This single pair shows no useful saving, not proof of a regression.
No SDK or production client adopted the experiment. Checked capture hashes,
counts and results are in `candidate-09-client-drive/native-screening-analysis.json`.

The next candidate exposes fixed arrays of sixteen whole pixels to the compiler,
with a scalar remainder. Comparator `914e86378` includes this change and the
client-drive revert. The compiler emits SSE2 loads, shuffles and comparisons
on crunch from safe Rust; there is no unsafe code, explicit SIMD intrinsic,
assembly or architecture-specific dispatch. Scalar partial-pixel edges, the
64 KiB unit cap, failure state, worker deadlines and authority remain unchanged.
Added differential controls target full-block/remainder boundaries under
unaligned budgets. Resource, image transport, cancellation and worker controls
and strict runtime Clippy pass in the affected checkouts.

Two warm-mapping benchmark invocations compare this candidate to a frozen
copy of the **improved scalar-block scan**, not the original byte scan. Panel
buffers (184,320 bytes) take approximately 20.5 versus 28.6 microseconds;
4 MiB buffers take approximately 0.47 versus 0.65 ms. The median reduction is
27.6–28.3%. This saves only about 8 microseconds per panel scan in isolation;
it cannot by itself explain or close the remaining whole-frame CPU gap.
Evidence and codegen are in `candidate-10-isb-vector/scan-analysis.json`,
`benchmark{,-repeat}.log` and `fixed-block-disassembly.txt`.

The attended vector-candidate screening completed all four arms, each with
1,000 unique joined presentations and no capture error. Plan and artifact hashes
match. Panel files+ISB costs 1.648–1.649 ms/frame against IPC's 1.308–1.309 ms,
an excess of 25.85–26.07%. Reaching the +10% ceiling still requires roughly
208–210 microseconds/frame of combined savings. Bulk, including a fresh 4 MiB
probe each frame, costs 6.287–6.288 ms for files versus 6.645–6.647 ms for IPC:
files is 5.37–5.41% lower in this pair. Session saves approximately 520
microseconds in bulk while the peer costs approximately 161 microseconds more.
Captures are `~/.local/state/t331/vector-cpu-{C,A}-{panel,bulk}-01`; checked
counts, hashes and intervals are in
`candidate-10-isb-vector/native-screening-analysis.json`.

These are single CPU-only screening pairs without environment readback,
allocation measurement or full latency qualification. There is no matched
pre-SIMD native control. Panel absolute files CPU is slightly higher than in
the earlier scalar scan pair even though the relative files/IPC gap is smaller;
do not claim a demonstrated native panel CPU reduction from SIMD. The isolated
scan controls support keeping the safe fixed-block optimization. Bulk is within
the CPU ceiling in this pair, but t331 is not qualified or closed.

The remaining measured blocker is panel control/owner overhead, not a reason
to replace 9P or the ISB architecture. Further work should isolate that cost;
readiness handoff and server image-probe gating remain deferred experiments.
Other targets and compiler versions may vectorize differently. Do not cache liveness,
suppress Ready retries, or change ACK/deadline policy. t331 remains open;
full cost and latency qualification has not been rerun.

## October 18 service-ledger results

The files-only diagnostic compared batched `ef07b18a8` with immediate-service
control `a0dfcdf57`. Their only source difference is the three role callback
sites; peer hashes, workload, inline observations and instrumentation match.
Both use runner `3a34520` and progress schema 4. Session counters are bracketed
to the peer's first and last progress records over 1,000 frames, excluding the
remaining idle hold. Bounds below reflect the sampling brackets, not statistical
confidence intervals or CPU durations.

| Per-frame count | Batched | Immediate |
| --- | ---: | ---: |
| Actual flushes | 6.992–7.001 | 6.993–7.001 |
| Shell owner input | 14.242–14.292 | 14.279–14.317 |
| Flushes writing bytes | 2.997–3.000 | 3.997–4.000 |
| Flushes writing no bytes | 3.995–4.001 | 2.996–3.001 |
| Protocol reads | 10.987–11.015 | 10.989–11.015 |
| Protocol writes | 18.977–19.006 | 18.979–19.006 |
| Waiting-read retry attempts | 11.239–11.266 | 13.267–13.293 |

Batching saved about two waiting-read attempts per frame, approximately one
Ready and one Pending, but removed no flush services. The 22.973–23.001 deferred
polls per frame were not saved flushes: repeated polls with the same pending
publication counted again. There were no failed or journal-blocked flushes in
either bracket. The earlier estimate of only 1.4 flushes per frame from ppoll
sites was incorrect; actual service counts replace that inference. The source
review and this single diagnostic pair support retiring the added batching API
and its boundary-close classification change. They do not identify which empty
flushes are safe to omit or establish a CPU duration for retry checks.

A subsequent source review explains why byte-empty flushes are plausible but
does not identify their exact native causes. `publication_pending` means a
journal append or image publication occurred since the previous service; it
does not mean a read is parked or socket output exists. A peer's next read may
still be unparsed in its socket, and flush deliberately does not read requests.
However, flush also drains the shared wake pipe, invalidates the readiness
observation, checks authority before and after service, retries waiting reads,
closes ended connections, expires held requests and services the image channel.
Skipping it solely because no bytes were written could leave a wake pending or
delay revocation and deadlines. Any further experiment must first distinguish
queued replies, parked reads and unparsed input, while preserving these effects.
This is a source-based hypothesis, not a causal attribution from the counters.

## October 18 optimization and pilot preparation — superseded

October 18 implementation checkpoint: bounded publication batching is signed
and published as production `2b60076d0` and comparator `6fb397748`.
Inline output observations are separately signed and published as production
`73dd4eba1` and comparator `43bcd5bef`; their patches have the same stable patch
ID on both branches. These implement niltempus's approved optimization plan.
Dock, Launcher and
Bar visits now share publication flushes within each callback; catalog execution
and direct activation remain immediate. Journal capacity still forces progress,
and authentication and delivery checks remain intact. A boundary close becomes
a per-role service failure; a callback error keeps precedence over a flush error.
Supplied-authority controls prove live delivery, no delivery after revocation,
callback-error precedence and disconnect on unwind. These are transport controls,
not evidence of real Factotum launch authentication.

The comparator-only ledger separates input, useful/empty/deferred flushes and
waiting-read retry sources/outcomes. Its controls allocate nothing. Write-path
retries do return Ready in the supplied-owner fixtures, so retry suppression is
not part of this change. Native CPU and allocation pilots must measure batching
and bounded observations separately before the unchanged qualification matrix.
Evidence is in `candidate-03/batch-*.log`; no native performance improvement is
claimed for either new slice. The final Session scope passes 965 tests with
private XDG directories; runtime tests pass across 88 targets, and the backend
suite passes. The engine's 12 focused tests include a zero-allocation and
zero-reallocation control for valid and overflowing observation sets. Scoped
strict Clippy passes for engine, backend, runtime and Session. The backend's
native request-construction allocation claim remains static; the engine
construction/reduction control is measured.

Both separate four-arm panel screenings completed. All eight captures have
1,000 unique joined presentations, no capture error, and the expected plan,
source and instrumentation identities. Batching alone shows no clear panel
benefit: files CPU/MiB remains IPC +37.70–37.96%, allocations/frame +21.27–21.73%.
Adding inline observations lowers files allocations from 1,169–1,171 to
1,091–1,093 per frame and IPC from 962–964 to 915–916. The remaining allocation
gap is +19.10–19.45%; CPU remains +37.31–37.72%. These are accounting bounds,
not statistical confidence intervals. One pair cannot establish causality.

Exact hashes, plans and analysis are under `candidate-04-batch` and
`candidate-05-output`; the combined report is
`candidate-05-output/paired-screening-analysis.json`. Captures use short
`~/.local/state/t331/{batch,out}-*` destinations. Their source-only batching
worktree was removed after capture validation. These are screening results
without environment readback, not qualification. Do not advance to the full
matrix while the panel remains red. Next, use the existing service ledger to
check whether this workload actually coalesces publications; do not infer a
CPU improvement from the allocation reduction. t331 remains open.

The next diagnostic pair is prepared in `candidate-06-service-ledger` and
`candidate-07-service-ledger-control`. Both use the same files workload, peer
binary and inline observations: batched comparator `ef07b18a8` versus control
`a0dfcdf57`, whose only change is three calls back to immediate service. Runner
`3a34520` validates progress schema 4 (30 original counters plus 17 ledger
counters), while preserving schemas 1–3. Six reader tests, two emitter tests,
six rebuilt peer controls, ten control Session tests and scoped Clippy pass.
Run `~/.local/state/t331/run-service-ledger.sh` attended on tty4. The batched
source-only worktree `~/dev/sophia-t331-ledger-batched` stays until capture
validation, then should be removed. Bracket counters to the 1,000-frame workload,
excluding the idle hold. Compare actual useful/empty/failed flushes per frame
and owner inputs; deferred polls are not a saved-flush count. These diagnostic
arms cannot establish CPU cost or qualification. Batching remains unproven.

## October 18 preceding diagnostic checkpoint

The first user-space perf pair has completed and its process/time-filtered
reports pass the raw-sample and histogram checks. Session has 153 files samples
versus 114 IPC samples; peers have only 34 versus 14. Image validation and
sealed-source preparation appear in the files samples, but this sparse pair
cannot explain the remaining CPU gap. Kernel sampling and syscall tracing are
now available after niltempus changed the host permissions. The next diagnostic
pair completed with external runner `32c2d61`, at 997 Hz with 26 stackless
syscall-entry tracepoints. Both `~/.local/state/t331/kern-{files,ipc}-01` reports
pass event, loss, identity, interval and histogram validation. Session ppoll
entries are 169,454 files versus 24,693 IPC over 1,000 frames; the peer adds
54,333 versus zero. Stacks include launch-custody checks on waiting-read retries.
Profiler overhead is substantial, so these CPU timings are not acceptance
evidence. Review redundant retry work without weakening revocation boundaries;
t331 remains open. Reports and summary are under `candidate-03/kernel-*`.

October 18 follow-up: the ISB-only integration is signed and published as
`5632d5423c9344c709810e00697a3d464cad5600`. Provlita
`78e14e3528137449aa83ad2ed6bc8768c6efefeb` and Lom
`9245bd05b95cec795819fbb9cee14b472722dc9a` now pin Rust SDK `fbde253`;
both full client checks passed before signing and publication. The earlier
client pins below describe the preceding migration checkpoint.

The isolated comparator is signed and published on `performance/t331-isb-cost`
at `7b636c5ad9bf38c11d430f9d590baa08d52946f2`, including the shared admission
optimization and mapped SDK imports. Its frozen historical branch and captures
remain unchanged. Six supplied-owner peer controls pass; the actual native
Session/CLI check also passes. New diagnostic-only
request scopes count thread CPU and process-wide allocations during each
interval. A control verifies allocation/reallocation counts and zero overhead
allocations for an empty scope; four paired supplied-owner runs pass. In those
64-frame runs, journal reads, reply encoding and read retries allocate little;
writes and outbound admission allocate more. Scopes overlap and must not be
summed. These fixtures do not reproduce the full native Session loop and do not
establish the cause of its cost gap. Evidence is under
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/`, especially
`request-attribution-tests-01.log`, `request-attribution-baseline-01.log` and
`comparator-native-check-01.log`.

Mapped ISB builders are signed and published: Rust
`c6ad4fa04395565297beeecaa18edc459ad9cb01`, C
`25be47d4292db5da3f877f64d8cb782c43eb5d7c`. Both exact imports pass Sophia's
snapshot checks. Rust passes 909 workspace tests and strict Clippy; C passes
the full suite and Nim binding smoke test. The builders bound allocation,
exclude the writable mapping from fork, and unmap before sealing and verifying.
The comparator now fills that backing directly with the same pixel routine as
IPC; six paired peer controls and four canvas controls pass. Sophia's shell
image, C lock and independent oracle export controls also pass
(`mapped-export-tests.log`). This removes the staging copy but proves no cost
improvement. Journal-read pairing now has identity-based evidence, described
below. No native capture has run for this candidate;
all four unchanged +10% gates and the latency gates remain open. Muse is offline;
Codex owns integration/publication and Claude assists with bounded SDK and
diagnostic work.

The shared optimization published as `4f243332f` validates and sizes outbound content with the SDK's
borrowed value counter instead of allocating encoded buffers for admission.
Production runtime all-target tests and strict Clippy pass; a differential
control preserves encoded charges and refusals. The same change is on both
comparator wires. Across 64-frame supplied-owner runs, ISB admission allocation
plus reallocation counts fell from 1,737 to 1 (panel) and 2,377 to 0 (bulk);
IPC admission counts roughly halved. The remaining single allocation is within
a process-wide interval, not proof of allocation by that function. See
`request-attribution-borrowed-01.log`, `borrowed-charge-tests-02.log` and
`borrowed-runtime-all-tests.log` in the current evidence directory. This does
not replace native cost or latency gates. Bemenu `2a1a73fbec1e0c5552d965b3b7aa3fd6b2e4977b`
and Kleis `5b314213dbb2733ec14d40d3e7c6f3fd5d9b8cee` now publish the mapped
C SDK pin; both isolated client gates pass, including Kleis build and format
checks. Its obsolete byte-upload C build input was removed.

External qualification tooling now publishes schema-3 ISB-versus-IPC campaigns
at `niltempus@879b672`: 72 arms per refresh rate, preserving both workloads,
three restarted pairs, idle/load, separate CPU/allocation/latency methods, and
all existing gates. Its campaign, comparison, provenance, historical pilot and
calibration controls pass, with strict Clippy and formatting clean. Passive
journal identity traces and their negative controls also pass. The broader
runtime gate passes 705 tests; the three older comparator fixtures now use
authenticated ISB on the files side and pass separately, with their resource,
lease and backpressure assertions preserved. Strict all-target Clippy and
formatting pass. The first attended ISB latency arm captured successfully, but
its IPC pair failed during negotiation. The comparator's unauthenticated
process-start path inherited production's content denial; the narrow IPC-only
repair now passes a Session-start regression and its unfixed negative control.
Replacement binaries and short-path campaigns `isb60b`/`isb120b` are frozen.
The replacement's first 12 arms captured successfully, with terminal recovery
verified for every arm. The first idle pair at 60 Hz exceeds the +10% limit for
panel CPU, panel allocations and bulk CPU. Two separate diagnostic pairs have
since completed. Preview-store reuse, borrowed inventory comparison and the
idle cursor no-work path are validated below. The four-arm panel cost screening
completed with clean recovery. Allocations fell substantially, but the paired
gap remains +20.44–21.00%; CPU remains +37.57–37.84%. See
`candidate-03/screening-analysis.json`. Prioritize remaining wire-service and
peer CPU costs, preserving authentication and service semantics, before another
full qualification block. A separate symbolized perf pair is prepared below
and awaits attendance; further source changes should wait for its hotspots.
Full cost and latency qualification remain open;
no installed desktop has been changed.

## October 18 repaired first-pair results

Campaign `~/.local/state/t331/isb60b` captured all 12 first-pair arms on the
repaired comparator `7b636c5ad9bf38c11d430f9d590baa08d52946f2`. Every recovery
log reports restored termios and keyboard mode, restored keyd state and no
emergency recovery. The bounded Session-plus-peer cost comparisons are:

| Measure | ISB relative to IPC | First-pair +10% result |
| --- | --- | --- |
| Panel CPU per uploaded MiB | +36.30% to +36.67% | Exceeds limit |
| Panel allocations per frame | +30.19% to +30.65% | Exceeds limit |
| Bulk CPU per uploaded MiB | +11.43% to +11.47% | Exceeds limit |
| Bulk allocations per frame | -46.93% to -46.86% | Within limit |

Evidence: `t331-isb-cost-20261018/candidate-02/first-block-analysis.json`, with
the immutable arm captures under the campaign root. These are single-pair
diagnostics, not the three-pair idle/load verdict or complete latency gates.
The next step is to explain the native panel CPU/allocation excess and bulk CPU
excess using separate diagnostic runs; preserve this candidate and its captures.
Do not relax the gates or subtract measured overhead to make the result pass.

The native counters locate the panel allocation excess in Session:
1,514–1,517 operations/frame versus IPC's 1,145–1,147; peer counts are
72–73 versus 71–72. Read-only review finds roughly 57–60 additional Session
allocations per additional owner pass in both workloads. This is a correlation,
not attribution: inspect per-pass work before changing protocol behavior.
Authenticated requests also check live launch custody through pidfd polling,
which the supplied-authority fixtures do not exercise. ISB's extra mapping and
validation faults remain a separate bulk CPU hypothesis.

A separate panel diagnostic pair is built from the same comparator revision
with `comparator-request-attribution` (including phase profiling). Plans and
artifact hashes are under `candidate-02/native-attribution/`; its launcher is
`~/dev/niltempus/.artifacts/t331-native-attribution-20261018/run.sh`. Both
profiles pass device-hidden configuration checks. Both attended captures
(`prof-files-01` and `prof-ipc-01`) completed with clean terminal recovery.
They use the existing recovery wrapper and produce diagnostic evidence only:
no campaign environment readback or acceptance verdict. Frozen cost artifacts
and campaigns remain unchanged. No authentication, polling or ACK policy was
changed to prepare this diagnostic.

The workload-aligned analysis is
`candidate-02/native-attribution-workload-analysis.json`; the separately labeled
`native-attribution-analysis.json` covers the whole diagnostic capture, including
its idle tail. Lifecycle and authority account for most of the allocation
difference, while component wire service accounts for most of the owner-thread
CPU difference. Request-handler intervals alone do not explain that CPU gap.
These are nested intervals with process-wide allocation counters, not exclusive
stack attribution. Static review confirms repeated preview-store map creation
and cloning, native service-request vectors and temporary idle-cursor collections;
see `native-service-allocation-audit.md` in the same directory.

Comparator `bc0270c79097363ef135cda8c9c70b5eba122be1` adds diagnostic-only nested
scopes for native requests, native waits, authority waits, lifecycle native
service, authentication polling, server turns and image-channel service. Counter
tests, native CLI strict Clippy, feature-off checking and scoped formatting pass.
Both nested diagnostic captures (`prof-files-02` and `prof-ipc-02`) completed
with clean recovery. `nested-attribution-workload-analysis.json` brackets the
1,000-frame workload with cumulative snapshots. Files lifecycle-native service
observes 422,256–422,958 allocations over 14,004–14,058 calls, versus IPC's
291,333–294,219 over 8,219–8,344 calls. Authority waiting accounts for only
23,349–23,357 versus 20,500–20,560 operations, ruling it out as the dominant
allocation interval. Files wire subscopes measure about 51 ms authentication
polling, 220 ms server service and 113 ms image-channel service; these diagnostic
thread-CPU intervals are not qualification measurements or exclusive call stacks.

The first shared fix retains `PreviewImages::live_stores` when the complete
store-to-renderer-group map is unchanged. Generation invalidation still runs
before assignment, and every retain and stale-snapshot cleanup still runs
afterward. The new storage-reuse regression fails under the old unconditional
clone and passes with the guard; it also checks disabled-owner pruning,
renderer-group changes and store removal. Both production and comparator pass
35 preview tests; production backend library/tests strict Clippy and scoped
formatting pass. Evidence is `preview-store-reuse-{tests,unfixed,clippy}.log`
and `preview-store-reuse-comparator-tests.log`. The comparator change is
`fbca2d4c3`; the production change accompanies this note.

This removes one map clone per unchanged synchronization, normally called twice
per native-service pass. It does not remove construction of the input map or
other preview/cursor collections and has not yet been measured in a native cost
pair. Those are the next bounded-storage candidates; preserve separate evidence
for each change. Authentication, deadlines, service order and ACK behavior
remain unchanged; t331 remains open.

The next inventory change is published as production `6f367ceb8` and comparator
`910414b7c`. It compares borrowed store/group pairs before constructing a map.
All current keys must occur, and every supplied pair must match; conflicting
duplicates fall back to collection, preserving the old last-entry-wins rule.
Changed inventories are moved into place only after generation invalidation.
Cleanup still runs on unchanged inventories. The isolated allocation control
records 2,000 allocations for 1,000 old collections and zero for 1,000 unchanged
borrowed comparisons. Both trees pass 35 preview tests and the allocation
control; strict backend Clippy passes. Evidence:
`preview-inventory-borrow-tests-03.log`,
`preview-inventory-borrow-comparator-tests.log`, and
`preview-inventory-borrow-clippy-02.log` under `candidate-02/`.

A separate cursor change returns before constructing output/head collections
when the path is non-atomic or no head has a pending atomic cursor. The old
per-output service already returned or skipped every head before any mutation,
clock read, commit or error in those cases. Pending cursors, including those on
disabled heads, retain the existing path. The 13 cursor-owner controls and 316
libdrm-event controls pass (one opt-in test remains ignored); strict Clippy and
scoped formatting pass. Evidence: `idle-cursor-{tests,clippy}.log`. These controls
and static review do not measure physical cursor latency or native cost savings.
The next native cost screening will measure the combined bookkeeping changes
before another full qualification block; frozen earlier captures remain intact.

The cursor change is published as production `5c81e518c` and comparator
`890215a3e3d4fcc2155f45c67afc63a03d792065`. Fresh CPU-only and Rust-global
allocation release builds at that comparator revision exclude all profiling
and trace features. Four panel plans (ISB then IPC, CPU then allocations) and
device-hidden profile checks pass; both source trees match their signed declared
revisions and are clean. The attended launcher is
`~/dev/niltempus/.artifacts/t331-bookkeeping-20261018/run.sh`; evidence and hashes
are under `t331-isb-cost-20261018/candidate-03/`. New run destinations are
`~/.local/state/t331/book-{cpu,alloc}-{C,A}-01`. All four captured successfully,
with restored termios, keyboard mode and keyd state and no emergency recovery.
This is a
four-arm cost screening without campaign environment readback, not the full
three-pair idle/load cost and latency qualification.

| Panel measure | ISB | IPC | ISB relative to IPC |
| --- | --- | --- | --- |
| CPU ns per uploaded MiB | 9,845,151–9,857,028 | 7,151,086–7,156,433 | +37.57–37.84% |
| Allocation operations per frame | 1,161–1,164 | 962–964 | +20.44–21.00% |

The earlier candidate recorded 1,587–1,590 ISB allocations/frame and
1,217–1,219 IPC allocations/frame. The observed reduction is substantial on
both wires, while the paired gap narrows from about 30% to about 21%. CPU shows
no corresponding closure: Session remains about 1.309 ms/frame versus IPC's
0.978 ms/frame, and the peer about 0.423 ms versus 0.280 ms. These separate
screenings do not establish a statistically controlled between-candidate CPU
change. Retain the unchanged +10% cost ceiling and full latency requirements;
t331 is not complete.

## October 18 sampling profiler preparation

The existing external `run-cpu-profile` runner captures inherited user-space
CPU samples at 199 Hz with 8 KiB DWARF stacks, then verifies and filters reports
to the outer Session and peer process IDs and workload interval. Its explicit
device-hidden inheritance/filtering control passes (`candidate-03/perf-control.log`).
The comparator remains `890215a3e3d4fcc2155f45c67afc63a03d792065`; separate
CPU-only release binaries were built with `CARGO_PROFILE_RELEASE_DEBUG=1`.
Both have `.debug_info`, `.symtab` and `.eh_frame`; profiling/allocation/trace
features are absent. Build records and artifacts are separate from acceptance
binaries. Two observed panel plans and device-hidden configuration checks pass;
source trees are clean at the declared signed revisions.

Prepared descriptors and hashes: `candidate-03/perf/ready.json`. Attended tty4
launcher: `~/dev/niltempus/.artifacts/t331-perf-20261018/run.sh`. Fresh capture
destinations are `~/.local/state/t331/perf-{files,ipc}-01`; both completed.
The normal recovery wrapper and environment observer remained active. Reports
are `candidate-03/perf-{files,ipc}-no-inline-report.json`; the summary is
`perf-user-summary.json`. Inline expansion repeatedly invoked addr2line and
made reporting slow; disabling that expansion preserved recorded callchains,
symbols and exact sample-count checks. The original slow report attempts were
stopped while processing peers. These runs are diagnostic, not cost or latency
acceptance.

The original denied probes remain in `perf-kernel-access.log` and
`perf-syscall-access.log`. After this pair, niltempus set
`perf_event_paranoid=-1`, `kptr_restrict=0`, mounted tracefs and made syscall
event metadata readable. Kernel-clock and syscall-read probes now pass. These
permission changes do not add kernel information to the existing captures.
Restore the original sysctls (2 and 1 respectively) and unmount tracefs after
profiling.

Within the workload interval, files Session has 153 samples (6 unknown leaves)
and IPC 114 (9 unknown); peers have 34 and 14, both fully symbolized. All four
reports have zero lost samples and agree with independently filtered raw sample
counts. Files validation accounts for 16 Session self samples; sealed-source
preparation accounts for 12 peer self samples. These are leads for a denser
kernel-inclusive capture, not precise attribution of the CPU difference.

The kernel/syscall mode is published in external integration revision
`32c2d616725a3ce3da41a566f82b1374d2926a5c`. Old descriptors retain user-only
sampling. The new mode verifies actual recorded event attributes, loss counts,
TGIDs, workload timestamps and CPU histogram totals. Syscall-entry counts are
not durations. Both real-perf controls passed with retained unfiltered output
in `candidate-03/kernel-profile-controls.log` (18 tests). Strict Clippy passed
(`kernel-profile-clippy.log`). The full serial xtask suite passed 415 tests,
13 ignored (`kernel-profile-tests-02.log`); the first parallel run hit an
unrelated launcher fixture's `ExecutableFileBusy`, retained in
`kernel-profile-tests.log`. Scoped formatting and diff checks passed.

The fresh pair reuses comparator `890215a3e` and the first pair's symbolized
CPU-only binaries. Both plans and device-hidden desktop configuration checks
pass (`kernel-perf/preparation.log`). An initial generated configuration had
unsafe permissions and was rejected before launch; the corrected inputs use
private permissions, and the rejected attempt remains in
`kernel-perf-preparation-01`. The native wrapper and environment observer are
unchanged. This pair supplies diagnostic evidence only.

## October 18 kernel and syscall results

Both captures completed and their reports passed all validation. Evidence:
`candidate-03/kernel-{files,ipc}-report.json`, `kernel-profile-summary.json`,
and the retained `~/.local/state/t331/kern-{files,ipc}-01` directories. The
recorders noted 7 and 14 out-of-order events respectively; reports show zero
lost samples and no lost-event warnings. CPU histogram totals equal the
independently filtered raw counts. Data files are 214 and 166 MiB respectively.

The following counts cover the verified 1,000-frame workload intervals, not
the entire 60-second recordings:

| Observation | Files | IPC |
| --- | ---: | ---: |
| Session CPU samples | 1,671 | 1,238 |
| Session unknown leaves | 43 | 50 |
| Peer CPU samples | 580 | 353 |
| Peer unknown leaves | 0 | 0 |
| Session ppoll entries | 169,454 | 24,693 |
| Peer ppoll entries | 54,333 | 0 |
| Session read entries | 73,618 | 25,396 |
| Session recvmsg entries | 22,144 | 668 |
| Peer recvfrom entries | 86,307 | 12,006 |
| Peer sendto entries | 25,996 | 10,001 |
| Peer memfd_create entries | 1,000 | 0 |
| Session mmap entries | 1,015 | 13 |

The files stacks include `LaunchCustody::is_live` through
`Authority::authorize`, authenticated export checks, `read_now` and
`retry_waiting`. This confirms the mechanism exists in the measured native
path; it does not assign every ppoll entry to authentication. Validation has
103 Session self samples; sealed-source preparation has 46 peer self samples.
The corresponding IPC canvas fill has 49 peer self samples.

Instrumentation cost is visible: functions named `perf_*` account for 302
files Session self samples and 157 peer samples, versus 199 and 54 on IPC.
These counts are a visible subset of profiler overhead, not a correction that
can be subtracted to reconstruct uninstrumented timings. Kernel percentages
and the profiled CPU gap therefore cannot establish the optimization budget.
The useful result is the syscall-volume difference and the identified call
paths. Next review waiting-read retry work while preserving authentication,
delivery and revocation checks; any accepted change still needs an unprofiled
paired measurement and the unchanged full qualification gates.

## October 18 first attended block

The initial deep evidence path exceeded the Unix-socket address limit while
creating the environment monitor. The claimed failed arm remains under
`t331-isb-cost-20261018/campaigns/t331-isb-60000-20261018/`; no Session launched.
Replacement campaigns used short roots `~/.local/state/t331/isb60a` and
`isb120a`, preserving the plans, artifacts and ordering. External tooling
`c832a1f54418b749303fd28e116443006c551d3b` now checks that address at freeze and
before an arm claim; boundary and no-write tests, strict Clippy and format pass.

In `isb60a`, `idle-p1-panel-latency-C` captured all outcomes and recovered.
`idle-p1-panel-latency-A` failed with `ContentAdmissionRefused`, reason 4,
denied capabilities 384. Session downgraded Granted to Unavailable on its
unauthenticated compatibility start, which the isolated IPC baseline retains
deliberately. The peer then reported message kind 160 instead of a welcome.
The block stopped; tty4 text mode, keyboard mode and termios were restored,
with no emergency recovery. The attempt, wrapper, Session and recovery records
are retained beneath that arm. This is an invalid pair, not a measured cost
regression. `native-first-block-result.json` in the current evidence directory
indexes both failures. No old attempt will be overwritten or resumed.

Comparator repair `7b636c5ad9bf38c11d430f9d590baa08d52946f2` preserves Granted
on ComparisonIpc only; unauthenticated NineP remains refused and the native
protected-baseline opt-in guard is unchanged. A device-hidden protected-child
test exercises `ShellComponentProcesses::start`, negotiated content grants and
retained process custody. Restoring the old downgrade reproduces message kind
160; restoring the fix passes. Session library/test strict Clippy passes with
and without native-session. See `claude-ipc-start-*` logs under
`isb-client-migration-20261018/`. Production source is unchanged.

Replacement optimized artifacts are staged separately under
`niltempus/.artifacts/t331-isb-cost-20261018-r2/`; their builds, hashes, 24
profile checks and new plans are recorded in
`t331-isb-cost-20261018/candidate-02/`. `ready.json` binds the repaired comparator
and runner to the short-path campaigns `~/.local/state/t331/isb60b` and
`isb120b`. The ordinary peer is byte-identical to the previously tested
optimized peer. The old first-block command now delegates to the new launcher;
each arm still requires the foreground attended console and stops on failure.

## October 18 initial frozen native campaigns — superseded

The signed comparator `bebb9db9b69b5eda1eae63e1124374f808abd8f6` and external
runner `879b6729375b9330304abadd867b85718b5025fd` bind three optimized host/peer
builds: ordinary latency, CPU-only and Rust allocation counting. Diagnostic
trace and attribution features are absent. The staged ordinary peer passes all
six device-hidden controls; 24 explicit profiles and the core configuration
pass the prepared Sophia checker. No native arm has run.

Evidence is under
`~/.local/state/sophia/development-evidence/t331-isb-cost-20261018/`:
`final-artifacts.json`, `final-*-build.{jsonl,log}`,
`frozen-latency-peer-controls.log`, `frozen-profile-checks-02.log`,
`native-preparation.log`, `campaign-freeze.log` and `native-ready.json`.
Binaries are staged outside evidence in the external integration checkout's
ignored `.artifacts/t331-isb-cost-20261018/` directory.

The 60 Hz campaign digest is
`bc2fa379aa4acfec6e0cb6cf64d176a146d6bd5773088763ea1d73a2782b7a1b`;
the 120 Hz digest is
`c49da844db420dbee13b67e2dde60d82c7c31d144937a595926580b8b2fc1f54`.
Each freezes 72 ordered arms across both workloads, three restarted pairs,
idle/load and separate latency/CPU/allocation methods. Profiles explicitly
select DP-1, card0, output 1, 1920×1080, scale 1 and VRR off, continuing the
historical target without hardware autodetection. Runtime observations must
still confirm it. `operator-commands.txt` contains the exact attended tty4
commands. Preserve failed or incomplete attempts; a frozen matrix is not a
passing measurement.

## October 18 journal read pairing

The comparator's opt-in trace records journal commit ranges, completed reads,
ACKs and shell request/outcome identities without payloads or path-time
allocation. It is excluded from cost builds. Synthetic controls cover joined
and split reads, partial records, rereads, skipped reads, missing outcomes,
ambiguous keys and transaction mismatches. Two controls and both supplied-owner
ISB fixtures pass; the feature-enabled 9P suite passes 162 tests and strict
Clippy is clean. These are device-hidden diagnostics, not native measurements.

Evidence is under
`~/.local/state/sophia/development-evidence/isb-client-migration-20261018/`:
`claude-journal-pairing-{control,isb,clippy}.log` and
`claude-journal-pairing-isb/{isb,isb-4mib,isb-512}.journal-trace.json`.
All three traces report zero dropped records, unmatched outcomes or violations.
Every matched outcome is delivered in a separate read from its receipt, joined
by key and transaction identity. Every receipt and outcome has a covering ACK.
The 64-frame panel trace has 708 commits, 643 reads and 644 ACKs; 578 reads
complete one record and 65 complete two. Its 64 candidates produce 128 outcomes,
and 64 ImageBegin requests produce 64 resource statuses. The bulk fixture has
128 ImageBegin requests and 962 one-record reads plus 65 two-record reads.
Negotiate, FrameDemand and ResourceRetire have no matching outcome by contract;
their absence is not counted as a failed join.

Raw commit/read/ACK tables retain ordinals, owner turns and spans so the joins
can be audited. This establishes the supplied-owner delivery pattern, not its
cost in a native Session. Read completion timing, batching and ACK policy are
unchanged; the trace does not authorize changing them.

## October 18 ISB-only integration checkpoint

t331 is implementing the October 18 Immutable Shared Buffer migration.
t338 is closed; its [delivery resumption](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md#current-resumption)
owns that result. The [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#immutable-shared-buffers--october-18)
requires production CPU images to use sealed sources without byte fallback.

The ISB-only SDK revisions are signed and published: Rust
`fbde253ef297e282357080600a560f1824cd6b50`, C
`14c8fce5d0d75d21b43ee6eb7a01232c0abd1399`. Both remove shell/lock byte
writers and public upload helpers, refusing retired candidates before spending
submission identities. Passive codecs remain for negative controls. Native
launchers adopt settled Accepted/Retained offers atomically; failed adoption
preserves custody, and late matching Accepted cannot resurrect retirement.
Admission uses the content grant epoch, not the connection epoch. Rust's owning
session exposes image setup, submission, cancellation and adoption. C consumers
must rebuild against changed public layouts. Both snapshots are imported and
verified in the uncommitted Sophia worktree.

The already published client migrations remain Provlita
`be198914684362fac2a39c454aa8f657664803b3`, Lom
`dcac836e72e75467bbd7fd8e4543e71d03a0a84b`, Bemenu
`7abb08cbdf2752dc9a3ad6b8f6b8cd754b5b2656` and Kleis
`919bc9f9a70838abd4b3bff04c402ec807679db0`. These use the preceding ISB SDK
pins; compatibility with the new removal pins still needs their external gates.
Finished Bemenu/Kleis migration worktrees were removed.

Sophia removal work is on `performance/t331-isb-only`, based on
`001265f6d8a4e49eccb959c451e77b28cf0d3205`. Exports no longer expose upload
slots and refuse byte Begin/End before custody. Resource stores retain sealed
mappings and leases only. Stopped validation retains its charge until completion;
retirement retains backing until ResourceReleased enters the journal. Store,
reconnect, native and composition fixtures use real sealed mappings.

Evidence is under
`~/.local/state/sophia/development-evidence/isb-client-migration-20261018/`.
The pinned runtime all-target run passes (`isb-only-runtime-pinned-08.log`);
workspace compilation passes (`isb-only-workspace-check-04.log`). Both SDK
workspace/full checks, Rust strict Clippy and formatting pass. Independent C/Rust
lock peers pass sealed submission, presentation and unlock; protected-launch
peers each pass three revocation/replacement cases. C launcher/dock and native
adoption export gates pass. Engine/renderer composition tests pass. Claude's
independent Go oracle passes 96 checks, including ten replacement image/refusal
controls, with Go tests/vet and scoped strict Clippy clean
(`claude-oracle-test-04.log`, `claude-oracle-go-tests-01.log`).

Integration now includes the shared C peer and all three hosts. Conformance
passes 4/4 and backend receipt/mutant controls 3/3. Session component accounting
passes 10/10 with actual bar/native allocations and sealed images. The GPU proof
loop passes 15 controls and its CLI passes six. Production proof launch now uses
Factotum/Startup and requires absolute agent and PAM-helper paths; only the
declared authentication relay is permitted among inherited sockets. Test hosts
supply afid verdicts explicitly and do not claim real launch acceptance.
The new proof-specific Factotum/Startup-to-client path has been reviewed but
not run against a real GPU launch; generic launch tests do not substitute for it.

The migration exposed an admission race: ChannelReady could precede the image
owner's admission opt-in. Ready-but-not-enabled now returns EAGAIN before
custody, preserving offer/submission identities for retry. The deterministic
channel test covers retry and closed-channel refusal. QEMU's lock stand-in now
uses authenticated sealed-image churn with a live allocation; compilation and
14 tool controls are not renewed QEMU timing evidence. Strict Clippy passes for
runtime, backend, conformance, Session and CLI with native-session/test-support;
formatting and both snapshot manifests pass. The full native workspace run
exposed stale reconnect fixtures and ambient configuration dependencies. A
private-config rerun cleared the latter; reconnect fixtures now pass all ten
controls using real allocations and sealed uploads. The descriptor transport needed a
retained image worker: it now services completions across reconnects, forwards
worker readiness while paused, and drains custody before final shutdown.
Runtime all-target tests pass (`isb-only-descriptor-runtime-tests.log`). Native
Session reruns pass 1,055 library tests, 27 launch-authentication tests and 14
component-connection tests (`isb-only-session-private-02.log`); strict Clippy is
clean (`isb-only-final-clippy-11.log`, including native-session/test-support;
`isb-only-session-default-clippy-03.log` for default Session features). The final
Session run also passes all seven process-owner integration controls
(`isb-only-session-final.log`). Both protected process-custody and launcher
evidence controls now use the real authenticated launch owner, with sealed
images and explicit settlement before retirement (`isb-only-custody-final.log`).
The obsolete byte-upload peer is removed. The metadata compatibility path reports
CPU content unavailable instead of granting an unusable path. Publication identity
and remote verification are recorded in `isb-only-publication.txt` beside the logs.

Next: update external client pins and gates, then carry the production changes
to the isolated comparator and
rerun the matched campaign. All four +10% cost gates remain open. Per-request
allocation attribution and journal read pairing are still required. Preserve the
red panel result and frozen comparator `6c87721fa` (integration `4b16974`) under
`~/.local/state/sophia/development-evidence/t331-b3-20261017/native-candidate/`.
No installed desktop or physical acceptance claim is made.

## Historical implementation checkpoint — before t338 closure

t338 remains in implementation. Codex owns shell/Session integration; Claude
owns the lock runtime lane. Muse remains quota-limited and has not started the
Rust producer. The [delivery plan's current resumption](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md#current-resumption)
records the implementation and evidence boundaries.

Shell sealed admission now connects real 9P and SEQPACKET to bounded validation,
mapped resource backing and separate terminal reservations. The persistent
owner survives disconnects and reconciles against exact retired stores.
Session installs it for authenticated components using their reserved budgets,
shares a monotonic resource clock, polls workers after revocation and guards
shutdown. Wire tests cover supplied afid authority, success, cancellation,
unsent-offer disconnect cleanup, topology snapshot replacement and legacy
absence. These are not protected-launch or native performance results.

C producer, BAR and lock adapters, explicit lock r2 selection and cancellation
are published through `e777235`; an independent full check passes. Rust
producer/adapters and cancellation parity remain open. Claude's lock runtime
now selects r2 and publishes coherent immutable discovery after the sealed
grant, with readiness-gated refusal. Lock authenticated Hello/Source acceptance,
Session submitted-worker shutdown qualification, SDK adoption and the native
CPU comparison remain to finish. The installed desktop is unchanged.

t337 design acceptance is recorded October 17 under niltempus's conditional
approval. Independent lifecycle peers Rust `8f7dc5d` and C `b2ca17d` passed
cross-review. The coordinated change widens both role topology contracts and
the shell ResourceStatus reason bound; the [shared normative contract](../../sophia-image-channel.md)
owns exact custody and layouts. Signed spec/evidence pins are Rust `8ad08de`
and C `ca61d21`. The preserved 54-case corpus and source hashes are in that
contract. Model tests do not prove production authentication, live discovery
fids, asynchronous validation or GPU completion.

t338's passive codec checkpoint is reviewed, published and vendored at Rust
`5c41aeb` and C `e146897`. It includes authoritative image KDL rows, explicit
classes, the distinct Rust lock mirror, shell image bodies and explicit r9/r2
envelopes. Old entry points retain their vocabulary. Review caught and fixed
LockSubmitted accepting ImageBegin at r1; the pre-existing shell receipt KDL
ceiling is corrected to 278. The Go oracle is integrated as `42548ad9b` and
`fd805a2eb`, with revision selection tested before reason-13 decoding.

Both Rust protocol suites, strict Clippy, shell field-offset tests, C's 54-case
codec controls and targeted Sophia schemas pass. Signed snapshot identity and
spec equality pass; the affected Sophia C/Rust/Go shell and lock export run
passes 62 tests. One existing test requiring an independently built upload peer
remains ignored. Claude's final 117,756-case differential per role on the exact
SDK heads has only the expected opposite-role discovery split and byte-identical
accepted round trips. The plan records scope, negative controls and harness
hashes. No independent review blockers remain for this codec checkpoint.

Next: finish Rust producer/adapters and lock receive/accept integration, then
qualify complete custody and native performance. Rust's
contextual codecs now check coherence and received FD counts; production
readers must use those entry points. Complete admission parity is not claimed. The
[first slice](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md#t338-first-slice-and-ownership)
keeps default callers byte-only. Explicit revision-aware paths admit shell
reason 13 only for r9 Rejected; legacy decoders and emission remain <=12.
Other reason fields must not widen.

CPU content uses sealed memfd per image. A shared launch window is not pursued
without measured need. DMA-BUF remains behind t339/t304 and t289 safe-completion
proof. No installed desktop or production transport changed during acceptance.

t331 remains open, waiting on t338. All four +10% gates remain unchanged; the
panel allocation result is accepted red as permission to proceed, not a pass.
Rerun all four after t338; per-request allocation attribution and journal read
pairing remain required. The frozen B2/B3 evidence remains comparator
`6c87721fa`, integration `4b16974`, under
`~/.local/state/sophia/development-evidence/t331-b3-20261017/native-candidate/`.
No quiet hold is active.

## Pre-acceptance checkpoint — October 17

The following records the state before coordinated contract acceptance.

The independent t337 lifecycle peers are reviewed and published: Rust `8f7dc5d`
and C `b2ca17d`. Both exercise real descriptor transfer and local custody models;
neither proves production authentication, live discovery fids or GPU completion.
Cross-review found no remaining blockers. Next is the owner's conditional
acceptance change: both topology contracts and the shell KDL reason bound
together, then opening t338. The proposed plan records the precise test limits.

October 17 owner direction: t331 remains open, waiting on t338, with the panel
allocation result accepted as red and all four +10% gates unchanged. Rerun all
four after t338; per-request allocation attribution and journal read pairing
remain required. t337 acceptance is approved in principle, conditional on reviewed
lifecycle/custody peers. The ADR remains proposed until that condition and the
coordinated contract/KDL amendment are satisfied. t338 uses sealed memfd per image;
the shared launch window is not pursued without measured descriptor cost. See the
[owner direction](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md#owner-direction--october-17-2026)
and [t331 criteria](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#t331--reduce-shell-allocation-overhead).

B3 validation is complete in signed Rust SDK `01474a9` and C SDK `5246f96`.
The final fault tests cover terminal status with four outstanding writes, a stale
head, partial transport output and cancellation with unissued chunks. Both SDK
suites pass, as does strict Rust Clippy. The independent review found no blockers.
Production adoption `ec410a09a` is signed and pushed. Snapshot checks, C
export/lifecycle gates, the full default Sophia suite, formatting and offline
metadata checks pass. The final source commits add only the reviewed test-scope
clarifications; their repeated snapshot checks also pass.
Comparator merge `6c87721fa` is signed and pushed with those exact archives;
its repeated snapshot checks pass. Integration remains `4b16974`. The matched
native B2/B3 pilot completed twelve arms: eight files/IPC cost arms for panel and
bulk, plus four separate progress arms. Offline replay reproduces all 12,000
exact presentation joins, journal reports and eight cost reports. Every arm
records normal console recovery. Both Claude sessions and Muse report no work
during the strict hold, now released. The installed desktop is unchanged.

All four +10% gates still fail. Panel CPU is +14.02–14.29% and allocations
+45.49–46.01% versus IPC; bulk CPU is +15.71–15.76% and allocations
+54.06–54.21%. Files owner passes per frame are 17.548–17.607 for panel and
90.179–90.384 for bulk. The result below separates improvements from the
remaining gap. Evidence, frozen plans and offline replay are under
`t331-b3-20261017/native-candidate` in the evidence directory below.

The test scopes differ deliberately: Rust forces kernel socket backpressure;
C limits each dispatch to one output byte. Rust's queued chunks are dropped by
the terminal status in its cancellation test. Enqueuing ResourceCancel alone
does not preempt those chunks. Prioritizing cancellation by dropping unissued
same-resource chunks is a follow-up investigation, outside B3; it needs its own
ordering and custody review before implementation.

The server's one-msize input budget remains the main limit on bulk pass counts.
Four client credits do not imply four pixel writes serviced in one owner pass.
The pilot supports prioritizing descriptor-path design for bulk content, with
producer/Sophia CPU, GPU work, latency and memory measured together. It does not
prove a descriptor speedup or solve the panel's remaining per-pass costs. The
descriptor implementation is not yet approved. The
[accepted plan](../plans/nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
owns the remaining performance gate. Evidence is under
`~/.local/state/sophia/development-evidence/t331-b3-20261017/`.

niltempus subsequently directed the shared descriptor design review. Its
[proposed lifecycle and byte layouts](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
answer the nine lock questions for both roles, with sealed memory first and
DMA-BUF capture behind the renderer ownership decision. This is design work;
t331's measured failure and the current byte contracts remain in force.
The draft now includes exact discovery/limits/formats layouts and a C/Rust
custody fixture matrix. Thirty-two packet vectors received independent review;
19 companion-object cases received independent review, then the draft switched
to common record headers with 21 revised cases. Formats now follow topology
generation; limits remain fixed per connection. Independent real ancillary/seal
kernel fixtures now pass in Rust SDK `05bf018` and C SDK `9bca0db`, with the
Rust shell-client suite, targeted strict Clippy and C `make check` passing.
Test-only decoders now pass all 53 draft cases in Rust `47397e5` and C `c6d7837`,
including truncated-prefix controls and real sealed-file length checks. They
use a fixed admitted sealed-only context, not general production codecs.
Follow-ups Rust `7977ba7` and C `90307ac` assert specific refusal categories and
compare generations across actual discovery/limits/formats objects; matching
changed generations pass and individual mismatches refuse. Both targets,
strict Rust Clippy and C Clang ASan/UBSan pass.
Rust lifecycle peer `8f7dc5d` and C peer `b2ca17d` are signed and published;
their targets pass, as does targeted strict Rust Clippy. Independent review
closed cancellation, metadata, deadline, packet-shape and deferred-release
findings. Real Source/SCM_RIGHTS is exercised,
but control/journal/worker steps remain fixture models; authenticated endpoints,
real discovery fids and GPU safety are not proved. The
[fixture checkpoint](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md#sealed-lifecycle-peer-checkpoint--october-17)
records the scope. No vendor pin or runtime changed. The fixture condition is
satisfied; the coordinated acceptance amendment remains next, and t338
implementation remains unopened.

## B2/B3 native pilot — October 17

The matched candidate is comparator `6c87721fa`, integration `4b16974`, Rust
SDK `01474a9` and C SDK `5246f96`. The timing peer uses Rust; these figures do
not establish C client performance. Each arm offered 1,000 frames at 60 Hz in a
60-second session. The bulk arm adds a 4 MiB upload to each panel frame. CPU and
allocation instrumentation run separately, with separate progress arms. The
replay's `comparison.json` uses whole-workload bounds, without interpolation or
subtracting instrumentation overhead.

| Workload and metric | Files | IPC | Files excess |
| --- | --- | --- | --- |
| Panel CPU, ms/uploaded MiB | 8.982–8.991 | 7.866–7.878 | 14.02–14.29% |
| Panel allocations/frame | 1,663–1,666 | 1,141–1,143 | 45.49–46.01% |
| Bulk CPU, ms/uploaded MiB | 1.6206–1.6209 | 1.4003–1.4006 | 15.71–15.76% |
| Bulk allocations/frame | 5,218–5,220 | 3,385–3,387 | 54.06–54.21% |

CPU includes all threads in Session and the peer, including foreign code, but
excludes other processes. Allocation counts cover their Rust global allocations
and reallocations, excluding foreign allocators. This synthetic byte-upload
workload does not measure a GPU producer's readback cost. It is a pilot, not the
full qualification matrix or an uninstrumented latency result.

Compared with the first-slice pilot, files CPU fell about 5.8% for panel and
4.0% for bulk; allocations fell about 10.5% and 8.6%. IPC CPU rose about 1.6%
in both workloads, so the narrower ratios are not entirely files improvements.
These are sequential campaigns, not repeated-run confidence intervals.

Separate progress arms bracket files owner passes/frame at 17.548–17.607 for
panel, against IPC 8.019–8.043, and 90.179–90.384 for bulk, against IPC
45.069–46.208. The previous files figures were about 22.6 and 102.3. Bulk still
has 67.863–67.999 pixel-bearing owner passes per frame; panel has 2.997–3.000.
These are observed activity counts, not exclusive wake causes or CPU attribution.
They support the one-msize input-budget explanation and rule out the earlier
69/4 bulk-pass projection for this implementation.

Keep t331 open with its existing gates. The recommended next design work is the
[proposed descriptor channel](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md),
starting with its accounting and shared lock/shell ownership decisions. It may
avoid repeated bulk byte transport, but no measured result yet establishes that
benefit. Keep the panel's request/pass and allocation costs explicit: three
pixel-bearing passes cannot explain its entire gap. Do not widen the server
budget, claim a descriptor pass, or start another fixed-cost slice from this
pilot alone. No candidate task is promoted and no new transport is implemented
by this result checkpoint.

## October 17 implementation and validation history

Production `a3c94bb86` implements the approved reusable writer (`ee22309ba`) and
imports signed Rust SDK `9d56625` and C SDK `30b7c1f`. Both source branches and
Sophia's first-slice adoption were pushed. Later checkpoints remain local as
noted below. The [decision](../decisions/7mls2e2q-reuse-a-shell-transaction-writer-without-changing-custody.md)
and [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#approved-reusable-writer-and-cumulative-ack-slice--october-17)
remain authoritative. No desktop installation has run.

The reconciled baseline captured all eight arms at comparator `363e06998`: merge `10d25e680`
carries production `116075541` and its existing SDK optimizations; the later
commit adds per-arm diagnostic output only. Offline replay reproduces all 8,000
frame joins and the cost reports. The original two bulk-allocation arms overlap
reviewer activity; clean recaptures now supply their comparison. The held-ACK Rust regression and C
negotiation-probe guard pass. Comparator `b67cb38d5` now carries the implementation;
its deterministic ledger reduces panel requests by about 30% and bulk requests
by about 16%, including startup. Fourteen native arms have now captured: eight
candidate cost arms, four separate progress arms, and clean recaptures of the
two provisional baseline bulk-allocation arms. Offline replay reproduces all
14,000 exact presentation joins, journal/cost reports and console recovery.
Comparator `9e3c741f8` adds separate owner-pass activity diagnostics; integration
`4b16974` reads the new format while retaining historical captures. These are
observations of work within passes, not inferred wake causes, and are excluded
from CPU/allocation builds. The completed campaign is under
`t331-reuse-20261017/native-candidate`; its exact comparator `9e3c741f8` and
integration `4b16974` identities remain in the plans and reports. The temporary
baseline checkout at `363e06998` was removed after its clean recapture.
The completed baseline lives under
`~/.local/state/sophia/development-evidence/t331-reuse-20261017/native-baseline`.
Any further native capture requires attended readiness and a fresh quiet window.

CPU-20 remains superseded preparation, with no capture. Cost-15 and perf-18 are
historical evidence, not qualification of this candidate. No quiet hold is active.
The pilot remains above the +10% ceiling: panel CPU +23.03–23.34% and allocations
+62.61–63.16%; bulk CPU +22.31–22.37% and allocations +68.48–68.62%. The full
matrix remains open. Request reductions alone do not explain those costs.

niltempus has accepted the [B2/B3 plan](../plans/nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
and directed implementation. The contract now permits queued final-fragment
submit and a four-write pixel window. The capture prerequisite is complete.
B2 is signed as Rust `7722502` and C `574a1eb`, adopted by signed Sophia `406f73f31`.
Rust's shell-client suite and strict Clippy pass; the full C suite passes.
Independent tests hold and reverse both replies, preserve early Submitted after
a failed record, reject incomplete-record retry and keep earlier C fragments
sequential. Initial candidates `b8b7b55`/`c0f0375` passed startup exchanges with
metadata and content negotiation on oldest supported export `116075541` and
current `feaf3cbc7`. The review follow-ups passed the current export again.
These are startup exchanges, not the entire export failure matrix. Logs are under
`~/.local/state/sophia/development-evidence/t331-b2-20261017/`.
The production export's incomplete/absent-staging and backpressure precedence
fixture passes for both writer paths. Further compatibility fixtures and B3
qualification remain open. No B2 native performance result exists yet.

The review follow-ups restore C's record-write ESTALE classification and preserve
a complete successful submit reply on terminal failure in both SDKs, even when
the record reply or journal event is missing. The ordinary AwaitSubmitted path
uses the same terminal rule; live progress still waits for the journal and its
covering ACK. Tests retain Unknown when no authoritative reply or valid event
establishes custody, and malformed journal data still fails validation. C reports
record ESTALE as a stale session; Rust retains its previous protocol-error
category. API-PARITY records that difference and B2's C consumer rebuild
requirement. Both signed follow-ups are upstream; B3 changes now sit above them locally.

Publication is complete: both signed SDK candidates and Sophia's result notes
are upstream. Exact archives have been imported locally. Both snapshot checks,
the Rust SDK suite and C production export/lifecycle gates pass. The full default
Sophia suite passes after adding the missing native-session guard to
`output_snapshot_allocations.rs`; that test also passes with its feature enabled.
Formatting, metadata and diff checks pass. After niltempus unlocked signing,
the guard was committed as `d3a94f878`, followed by B2 adoption `406f73f31`.
The installed desktop is unchanged.

B3 export coverage has begun locally: the real-socket split-upload test now
issues a short head and its tail before reading replies, checks tail EINVAL and
recovers the exact bytes. A direct export fixture verifies EAGAIN leaves the
cursor unchanged, queued tails refuse EINVAL while live, and a closing binding
answers ESTALE without more custody. Both pass. These tests do not establish
SDK windowing or expiry timing.

Both B3 SDK windows are signed and published: Rust `365e5f3`, C `63efea3`.
Documentation follow-ups `f3df891`/`303dfd0` record cancellation after a permanent
head refusal; the implementation bytes are unchanged.
Sophia `bba813fa8` records the export fixtures above. Rust gathers
four adjacent canonical units with separate tickets, matches replies by tag and
settles in issue order. Its tests cover credit refill across eight chunks, reversed
replies, short/EAGAIN heads with invalidated tails, End after drain and a terminal
status before write replies. C windows one borrowed buffer, with independent
tests for reversed replies, cursor recovery, idle wake, repeated backoff, explicit
low-level retry, contradictory tails and fencing. C's full suite passes. Rust's
shell/protocol suites and strict Clippy pass. Both also test iounit fragmentation;
C's eight-slot attached-client fixture proves a three-write window and refill.
The inline Rust window avoids a per-burst allocation; this is a storage property,
not a measured allocation-gate result.

C keeps the low-level client clockless and adds upload_retry; sophia_ss owns an
independent supplied-clock deadline and reports the earlier submit/upload wake.
Five control slots are reserved: default C session storage now has nine request
slots, while direct eight-slot clients use three upload credits. Both C structs
grow and consumers must rebuild. API-PARITY records the buffer/ticket distinction,
fragment scheduling and progress-or-time versus time-only retry. Recovery counts
remain in fixtures/instrumented evidence. Claude's C review found no blocker;
the control reserve correction is applied. Claude also reviewed Rust `365e5f3`
and C `63efea3` without adoption blockers. No B3
snapshot, comparator ledger or native pilot has run. No quiet hold is active.
SDK logs are under `~/.local/state/sophia/development-evidence/t331-b3-20261017/`.
Next: complete old/current export, remaining fault and fairness checks before
adopting B3; then collect
matched native evidence. The existing +10% gates remain unchanged.

The B2-only comparator checkpoint is signed and published as `87d135484`.
Its device-hidden supplied-owner diagnostic passes four 64-frame arms; files
panel counts remain 72 walk, 72 open, 648 read, 1,477 write and 68 clunk. Bulk
counts remain 136 walk, 136 open, 6,661 write and 132 clunk; reads are 1,032
against 1,031 in the earlier reusable-writer diagnostic. These totals include
startup. B2 removes a dependency wait, not requests. This fixture has no Session
owner loop, physical presentation or qualified CPU/allocation measurement.
Evidence: `t331-b2-20261017/candidate-ledger`, including per-arm source counters
and `comparison.json`. The comparator's existing mixed-activity labels still
include the previously reported Other overlap; do not infer wake causes from them.

Comparator `c014ad37f` adds an opt-in ordered shell trace. Three four-arm,
64-frame runs prove every steady B2 submit was dispatched before its record
reply finished sending: 320/320 panel and 512/512 bulk. Negotiation remains
sequential. The observation is dispatch, not socket arrival; it conservatively
proves that submit did not wait for the record reply. A separate fixture checks
partial reply delivery and a sequential negative control. A compile-time guard
rejects combining the trace with cost instrumentation; its allocation and timing
overhead are not performance evidence.

The extra bulk read is startup grouping. Runs 01 and 02 returned the first 168
journal bytes together; run 03 returned 112 then 56. Every later event-read
offset and reply length matches, including the final parked read. Thus the
traced 1,031/1,032 totals differ by one Tread and one Rread header, with identical
journal payload. This reproduces the earlier counter difference; the historical
untraced capture cannot identify its particular split. Evidence lives under
`t331-b2-20261017/ordered-trace-01`, `ordered-trace-02` and `ordered-trace-03`.
The first two include the separate progress sampler; the third omits it. All
ran before the trace commit, whose final changes only format code and clarify
dispatch wording. Raw summaries retain the earlier word "arrival"; interpret
their request phase as dispatch. B3 compatibility, fault and neighbor-service
checks are next. No B2/B3 native cost result exists yet.

niltempus approved completing B3 validation and one matched native pilot before
deciding whether to prioritize the proposed descriptor channel. The +10% gates
remain unchanged. The new supplied-clock socket fixture proves that partial
pixel scratch at one millisecond before the idle deadline does not renew it;
expiry fences later burst writes, and a late write or clunk from that fid cannot
affect a successor binding. All thirteen socket fixtures pass against current
export `0d79bd170` and oldest supported export `116075541`, using test-only
overlays on the latter. This establishes these export behaviors, not complete
B3 SDK interoperability or neighbor fairness. Logs and the old-export overlay
are under `t331-b3-20261017/`. No desktop or native measurement was started.

The same direct-export backpressure/tail fixture also passes both revisions.
C candidate `303dfd0` passes all three independent vector and native-file session
tests against both exports, including allocation, upload, cancellation and ACKs.
The test now accepts an explicit absolute `SOPHIA_C_SDK_SOURCE` directory so a
candidate can be tested before replacing the vendor pin; default runs still
check the pinned SDK. Rust candidate `f3df891` passes independent metadata and
content startup against both exports. These do not prove a full four-credit
window against production or neighbor fairness; those gates remain open.

Rust test-only follow-up `10c9226` adds an independent upload peer. It queues
five canonical chunks followed by End, checks every ticket becomes Stored,
then cancels a second resource. Against both exports the host verifies all
262,144 accepted bytes exactly; both runs pass, as does strict example Clippy.
The generic ignored fixture accepts an absolute `SOPHIA_SHELL_UPLOAD_PEER`
executable. This proves the multi-chunk path and End/cancellation interoperability,
not the maximum simultaneous wire depth or neighbor latency. The ordinary C
peer also exercises transport fragmentation with a 4,096-byte msize. Remaining
fault audit, depth/fairness diagnostics, adoption and the native pilot are open.

The fault audit found a Rust custody error before adoption: a zero or oversized
Rwrite removed the outstanding tag before validation, so teardown could label
issued bytes DroppedUnsent. Signed Rust `c1d3515` preserves Unknown and permits
Stored only for a valid positive reply within the issued length. The regression
failed before the fix and passes after it. The full shell suite and strict Clippy
pass. C already failed closed; signed `17f4b0f` adds matching invalid-count tests
and parity wording, with its full suite passing.

Comparator `e9dafb67a` stages those exact archives for qualification; production
has not adopted B3. Both snapshot checks pass. Two four-arm 64-frame traces
have identical pixel totals: panel 192 writes / 11,796,480 bytes; bulk 4,608 /
280,231,936. Short replies, EAGAIN, invalidated tails and other pixel errors are
all zero. B2 overlap remains 320/320 panel and 512/512 bulk. The first run used
Rust `10c9226`; the second includes the custody fix. Evidence is under
`t331-b3-20261017/ordered-candidate-01` and `ordered-candidate-02`.

The server dispatch trace sees at most one panel or two bulk pixel writes before
their replies finish sending. This does not measure the client's four-credit
window: the server reads at most 65,536 bytes per connection turn. A 4 MiB
payload plus request headers needs at least 65 input turns, so 69/4 was never a
valid server-pass prediction. The accepted plan now records this bound; B3 does
not change it. Native benefit remains unmeasured.

A separate two-export driver fixture keeps four writes queued for 128 turns,
completes 448 writes within the cumulative read budget, and answers a neighbor's
event read and control request on every supplied-owner iteration. One run reports
42.2 microseconds median, 74.4 microseconds p99 and 76.7 microseconds maximum
for that iteration. These are unqualified fixture timings, not native Session
latency or real-role performance. All ten Unix-driver tests pass. The remaining
fault review and native pilot still control adoption and qualification.

## First-slice native pilot — October 17

All fourteen arms captured 1,000 frames at 60 Hz, 1080p, scale 1, VRR off and
idle host load. Offline collection, journal metrics and exact kernel joins
reproduce the recorded reports. All console recovery checks pass. The three
other agents report no crunch activity during the hold; `quiet-window.txt`
records their confirmations and release. This is a pilot, not the full gate.

| Workload / measure | Files | Paired IPC | Conservative excess |
| --- | --- | --- | --- |
| Panel CPU ms/MiB, ordinary allocator | 9.538267–9.548299 | 7.741651–7.752949 | 23.03–23.34% |
| Panel allocations + reallocations/frame | 1,857–1,860 | 1,140–1,142 | 62.61–63.16% |
| Panel + 4 MiB CPU ms/MiB, ordinary allocator | 1.687707–1.688053 | 1.379412–1.379823 | 22.31–22.37% |
| Panel + 4 MiB allocations + reallocations/frame | 5,708–5,711 | 3,387–3,388 | 68.48–68.62% |

The clean baseline bulk-allocation recaptures are 6,078–6,080 files and
3,384–3,386 IPC operations/frame. The earlier panel baseline was 2,082–2,086
files and 1,142–1,144 IPC. Candidate allocations are lower, but neither workload
passes +10%. CPU and allocation figures above come from separate build modes;
do not substitute allocation-instrumented CPU for the ordinary-allocator result.

The IPC panel CPU arm also rose from 7.529965–7.541559 to 7.741651–7.752949
ms/MiB, about 2.8%. The files arm fell from 10.405274–10.418833. The ratio
improvement therefore includes comparator drift; it is not entirely a files-side
saving. Projections based on allocations per historical pass remain hypotheses,
not grounds to change the gate or assign the remaining gap to another task.

Separate progress runs bracket Session passes over the peer's full workload:
22.615–22.672 per panel frame versus IPC 8.018–8.040; 102.297–102.389 per bulk
frame versus 45.014–45.532. Files observes about five record, five submit and
three pixel passes per panel frame; bulk observes eight, eight and 72. These
categories overlap and are not wake causes. There is no matched native progress
baseline for the first slice, so historical pass counts do not prove its exact
pass saving. The results support testing B2/B3; they predict no automatic pass.

Evidence: `~/.local/state/sophia/development-evidence/t331-reuse-20261017/native-candidate/`
contains immutable run directories and `offline-replay/{summary,comparison}.json`.
The latter records exact ratios and false +10% verdicts for both cost measures
on both workloads. The disposable replay script is
`/tmp/t331-replay-reuse-candidate.py`; reports and raw logs are the retained evidence.

## SDK review follow-ups and candidate ledger — October 17

The subsequent [B2/B3 design](../plans/nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
was reviewed as proposed, then accepted by niltempus on October 17. It preserves
tail fencing, submit errno precedence, per-ticket custody and canonical-chunk
idle renewal. The frozen first-slice capture remains unchanged. B2 is now a local
SDK candidate as described in Current resumption; B3 has signed source candidates
but is not yet adopted into Sophia.

Signed Rust `9d56625` proves that the next retained-fid record is outstanding
while the covering ACK reply is withheld. Signed C `30b7c1f` waits for Negotiated
before discovery even when the caller has already acknowledged Submitted; a
scripted delayed-Negotiated test covers that ordering. C still waits for the
confirmed ACK before sending its next record, as documented in both parity files.
The full Rust shell suite, strict all-target shell Clippy, full C suite, both
vendor gates, Sophia runtime tests and independent C/Rust startup checks against
the production export pass. Production adoption is `a3c94bb86`.

Signed comparator merge `b67cb38d5` keeps IPC isolated and passes the supplied-owner
progress diagnostic on both wires. Across 64 frames, including startup/teardown:

| Request | Panel before → after | Panel + 4 MiB before → after |
| --- | --- | --- |
| Walk | 391 → 72 | 647 → 136 |
| Open | 391 → 72 | 647 → 136 |
| Clunk | 388 → 68 | 644 → 132 |
| Read | 648 → 648 | 1,032 → 1,031 |
| Write | 1,541 → 1,477 | 6,725 → 6,661 |
| Total | 3,359 → 2,337 | 9,695 → 8,096 |

The retained writer removes steady transaction lifecycle requests, with its
one-time discovery included. Write counts fall by 64 per workload; the single
bulk-read difference is not evidence of a general read reduction. IPC continues
to issue no 9P requests. This fixture has no Session owner loop or physical
presentation: its zero owner-pass counter is not a native result. CPU and
allocation savings were unmeasured at that checkpoint; the native pilot above
now records them. Evidence is under
`~/.local/state/sophia/development-evidence/t331-reuse-20261017/sdk-followups`
and `candidate-ledger`, with the comparison in `candidate-ledger/comparison.json`.

### ACK cadence finding and remaining attribution

Claude's independent review reproduces the lifecycle arithmetic: panel loses
319 walks, 319 opens and 320 clunks across 64 frames; bulk loses 511, 511 and
512. The one-time reusable-node probe accounts for the extra walk and open.
The measured write reduction is only one per frame in each workload. Do not
carry the earlier hypothetical panel ACK reduction from 11 to 5 into the pilot.
The unchanged pixel writes and nearly unchanged reads are expected for this slice.

The source offers an explanation for the limited ACK batching: the server retries
waiting reads after requests, and Rust sends eligible ACKs when its bounded drive
finds no further progress. Journal entries delivered separately can therefore
each trigger an ACK before the next entry arrives. The diagnostic's aggregate
counts do not identify which event pair shared a drive call or prove this cadence
in the native owner loop. In particular, the old 53/152 steady-frame ledger must
not replace the reconciled 64-frame totals, which include startup and teardown.

Removing requests does not establish an equal reduction in Session passes.
Clunks are not awaited, and several requests can share a pass. Predictions of
about 17 panel passes or a particular allocation gap remain hypotheses. The next
diagnostic must report owner passes on both wires, attribute setup, record,
submit, pixel, flush-only and cadence work where directly observable, and retain
unattributed or overlapping work explicitly. Do not infer those causes from
request totals or add diagnostic logging to CPU/allocation acceptance arms.

End-of-pass pending-read service, a different ACK cadence, queued submit and
pipelined pixel writes remain possible follow-up investigations. They are not
authorized implementation extensions of this slice; the matched native pilot
must establish the remaining costs before selecting another change.
The [9P transport prior-art survey](yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md)
records independent research and the deferred t334/t335 candidates.

### Owner-pass observation method

Comparator `9e3c741f8` uses a thread-local, allocation-free scope around each
owner-loop pass. Scope exit counts completion even on early return or continue.
Setup observes core 9P walks/opens across roles; shell writes distinguish record,
submit and pixel activity. IPC classifies decoded resource chunks as pixels and
other decoded frames as records, with no artificial separate submit operation.
Cadence observes a due pacing/deadline predicate, not a completed presentation.
Other includes core reads/writes/clunks and IPC input bytes; it deliberately
overlaps refined categories. Mixed passes remain explicit. Flush-only means
observed output bytes without another category, and unattributed means no
instrumented observation; neither establishes that no other work occurred.
Mixed includes the Other overlap: a single shell write or decoded IPC frame can
set both bits. Other is not residual, and mixed does not mean multiple kinds of
shell work. Presence counters cannot establish ACK/read ordering or identify
which event pair shared a reply; that requires an ordered trace.

Tests cover allocation-free accumulation, repeated activity counted once per
pass, mixed/unattributed/flush-only categories, bounded formatting and historical
schema parsing. The native-feature build check and both-wire supplied-owner
diagnostic pass. That fixture has no real owner loop, so native attribution still
requires separate progress captures. Diagnostic schema 3 cannot be mixed with
cost instrumentation. Evidence is in `t331-reuse-20261017/pass-ledger`.

The three release modes are built separately and hashed at their cargo-slot
paths. Baseline allocation executables retain their original hashes through
hard links within the original cargo slot, not binary copies in evidence.
Fourteen fresh plans pass source/artifact checks and device-free argument
validation. Initial preparation refused a changed preflight hash; the current
integration preflight was rebuilt and explicitly pinned, and the refusal log
is retained. The helper performs no builds and refuses existing run directories.
No quiet hold or desktop installation is active.

## Reconciled native baseline — October 17

The eight-arm baseline uses comparator `363e06998`, Rust SDK `f1cd924`, and
integration `fd2cb6c`. Each arm presented all 1,000 offered frames at 60 Hz.
Independent `collect`, `join` and `cost-metrics` replay reproduces the saved
frame joins and cost reports. Evidence and replay are under
`~/.local/state/sophia/development-evidence/t331-reuse-20261017/native-baseline`.

| Workload | 9P CPU ms/MiB | IPC CPU ms/MiB | 9P Rust alloc/realloc per frame | IPC Rust alloc/realloc per frame |
| --- | --- | --- | --- | --- |
| Panel | 10.405274–10.418833 | 7.529965–7.541559 | 2,082–2,086 | 1,142–1,144 |
| Panel + 4 MiB | 1.697965–1.698256 | 1.375569–1.375945 | 6,079–6,081 (provisional) | 3,386–3,388 (provisional) |

CPU comes from the CPU-only arms; allocations come from separate Rust-global
arms. CPU covers all threads in Session and the peer, including foreign code;
allocations cover their Rust global allocators only. Other processes are excluded.
These are conservative measurement bounds, not confidence intervals across runs.
The baseline predates reusable writers and ACK batching; it cannot establish
their effect or qualify the candidate.

Claude's review pane reported read-only searches and git reads at
11:56:53–11:58:58 UTC during the requested hold. Approximate realtime mapping
places the bulk-allocation intervals at 11:57:37–11:57:54 and
11:58:42–11:58:59, so both remain provisional, with all samples retained.
The CPU arms and panel-allocation pair ended before that activity. Muse reports
no activity. The quiet hold is released. See `quiet-window.txt` and
`offline-replay/summary.json` for provenance and timing limits.

## Implemented writer and ACK slice — October 17

The export reserves one bounded buffer at open, moves it through assembly and
accepted custody, and recycles it only after a covering ACK. Refused assemblies
retain their errno and readable bytes; malformed or foreign-epoch submit
envelopes do not poison valid staging. Expiry and revocation permanently fence
the token. Both SDKs probe after negotiation and its ACK, use legacy transactions
only after lookup ENOENT, retain successful writers and replace refused ones.

Rust batches automatic ACKs at existing drive boundaries. C preserves its
explicit cumulative ACK call and confirmed-ACK barrier; no automatic policy was
inserted into a manual API. Independent tests consume three records and send
one ACK in both SDKs. Manual control and snapshot holds remain covered.

The full Rust shell suite, strict shell Clippy, full C suite, both immutable
vendor gates and Sophia's runtime test suite pass. Both independent startup
peers pass the real export. Six new export tests cover reuse, refusal, replay,
ACK barriers, cancellation, expiry, revocation, idle failures and backpressure.
The real-wire fixture observes reusable writes and proves that the covering ACK
precedes the next record. These are protocol checks, not physical acceptance.

Evidence is under `~/.local/state/sophia/development-evidence/t331-reuse-20261017`.
The reconciled supplied-owner diagnostic completes 64 frames on each wire and
workload. Its per-arm totals include startup and teardown; do not present them
as steady-frame counts. Files panel: 391 walks, 391 opens, 648 reads, 1,541 writes
and 388 clunks (3,359 requests). Files bulk: 647 walks, 647 opens, 1,032 reads,
6,725 writes and 644 clunks (9,695 requests). IPC issues no 9P requests. This
fixture has no Session owner loop: it cannot attribute native owner passes or
establish performance savings. Raw journals and counters are in `baseline-ledger/`.

## Reconciled request ledger — October 17

Comparator peer `9c527d566` has the same workload as batching-14 `bf19952dd`.
It sends panel Begin, three canonical pixel chunks, End, FrameDemand, a grouped
Candidate, then Retire of the previous resource after Presented. The first
frame omits Retire; allocation setup precedes the measured loop. The grouped
Candidate is one file transaction and three IPC records.

The bulk case first sends a separate probe Begin, 69 pixel chunks, End and Retire,
waits for release, then runs the complete panel sequence. Negotiated msize is
65,536, its write payload bound 65,513, and canonical chunks are 61,440 bytes.
Neither larger msize nor short-write avoidance reduces these pixel-write counts.

| Steady-frame requests | Panel | Panel plus bulk |
| --- | ---: | ---: |
| Transaction walk/open/clunk | 15 | 24 |
| Upload walk/open/clunk | 3 | 6 |
| Record and submit writes | 10 | 16 |
| Pixel writes | 3 | 72 |
| Journal reads | 11 | 17 |
| ACK writes | 11 | 17 |
| Total | 53 | 152 |

ACK and read counts match batching-14 but can vary with scheduling. Clunk is
fire-and-forget, walk/open already share a setup batch, and idle relay counts
record skipped core turns. Requests do not each cause a flush. IPC sends ten
panel records and receives six; the probe adds 72 sent and three received.
Those record counts are not owner passes or transport syscall counts.

`Staging::new` uses `Vec::new`; it constructs an empty object without allocating.
First record bytes, fid tables, request decoding and owner passes need separate
allocation attribution. Ratios of allocations to passes do not establish an
exclusive causal cost per pass. The reusable writer predicts 15/24 fewer requests,
not a native gate verdict or a fixed number of eliminated passes.

## Earlier resumption — October 16

The audit identifies two concrete copy paths to measure: completed journal reads
allocate a temporary payload before copying it into the server reply buffer,
and the SDK copies upload payloads into its output buffer. Neither is a measured
explanation for the whole cost gap. Rio's request reuse is less novel for Sophia
than it first appears; its event waits do not remove custody checks.

Exact sizing now removes the two reallocations previously measured when gathering
four records. Unbuffered reads still allocate once; encoding into warmed
output storage and empty pending reads allocate nothing. The follow-up below
confirms one SDK payload copy, zero staging allocations on a second same-size
upload, and one socket send per upload in the isolated fixture. The approved
journal scratch experiment is now implemented: warmed journal reads allocate
nothing in all five fixture cases. Cost-11 native allocations fell only about
0.4%; CPU changed +1.8% for panel and -1.0% for bulk against cost-10. The gate
still fails. Both payload copies remain. SDK vectored output targets
copy traffic, not the warmed allocation count. The approved t331 slice excludes a new vectored
output ownership API; implementation needs a separately reviewed scope. t332
remains investigation only. Keep the installed desktop untouched.

## Maintained references checked October 16, 2026

| Project | Observed activity | Relevance |
| --- | --- | --- |
| [9front](https://github.com/9front/9front) | `a411fb63c`, October 16 | Complete system; rio, drawing, file services and kernel mechanisms. |
| [9legacy](https://9legacy.org/patch.html) | Patch page updated October 8 | Conservative fixes and individually reviewable kernel/library patches. |
| [plan9port](https://github.com/9fans/plan9port) | `b6564bd96`, August 26 | Unix-hosted libraries and applications, including lib9p and devdraw; not a complete OS. |
| [Go p9](https://github.com/hugelgupf/p9) | `3fd948847`, June 25 | 9P2000.L client/server; response and encoding storage reuse, separate payload vectors. |
| [diod](https://github.com/chaos/diod) | `de51d1ee1`, June 23 | C 9P2000.L server; request queues, cancellation and worker ownership. |

These dates establish maintenance, not recent performance work. Linux and QEMU
remain references from the [earlier comparison](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#t331-established-9p-implementation-comparison).

The complete 9front source tree is at `~/src/9front`, revision
`228167f32bf5b14fcb41e52eab527a1c8531638a` (September 29), with shallow history.
This audit reads that checkout, not the newer upstream head. Source hashes,
downloaded Go/diod files and maintenance records are retained in
`~/.local/state/sophia/development-evidence/t331-plan9-comparison-20261016/`.

## Rio: reuse has limits, and waits include cancellation

In [xfid.c](https://github.com/9front/9front/blob/228167f32bf5b14fcb41e52eab527a1c8531638a/sys/src/cmd/rio/xfid.c),
`xfidallocthread` recycles Xfid records with their workers and channels. It creates
more when the free list is empty; the inspected path has no fixed pool cap.
Sophia must preserve its explicit bounds rather than copy that growth policy.

Rio still allocates a message buffer per request. In
[fsys.c](https://github.com/9front/9front/blob/228167f32bf5b14fcb41e52eab527a1c8531638a/sys/src/cmd/rio/fsys.c),
`filsysproc` allocates `messagesize + UTFmax`, reuses it for the response, then
frees it in `filsysrespond` or `filsyscancel`. Reusing Xfid does not pool every
wire buffer.

`xfidread` waits on a particular window's data channel, window deletion and the
request's flush channel. The lesson is to represent every reason a request must
resume, including cancellation and lifetime changes. It is not permission to
wait only for data. The [rio manual](https://9p.io/magic/man2html/4/rio) also
explains that the draw device multiplexes itself: rio does not mediate application
access to it. Rio is not a comparable authenticated bulk-upload benchmark.

Sophia already retains connection input/output vectors and waiting/unwritten
queues (`crates/sophia-9p/src/connection.rs`) and a driver read buffer (`unix.rs`).
Waiting reads are small records, not new workers. An empty journal read returns
Pending at the tail without allocating. Retrying checks authority; delivery
checks remain live through partial writes. Rio's design does not eliminate these
obligations. The [change-token rejection](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#t331-change-token-audit)
still applies.

## Go p9: separate payload vectors expose a remaining SDK copy

At the pinned revision, [transport.go](https://github.com/hugelgupf/p9/blob/3fd948847ea46b413f8b98c8f226c6bb716807be/p9/transport.go)
gets an encoding buffer from `dataPool`, then passes header, encoded fields and
payload as separate vectors to `WriteTo`. Payload cleanup follows send. This
avoids concatenating payload into the encoding buffer. Whether it uses one
vectored syscall depends on the writer; this is not kernel zero-copy.

Sophia's vendored `sophia-9p-client/src/pipeline.rs::submit` still copies both
body slices into `out_buf`. Earlier cursor fixes avoid shifting sent prefixes,
but not this initial copy. A nonblocking vectored API must retain payload ownership
through partial sends; the current borrowed-slice API cannot keep the caller's
slice. Preserve byte/tag charges, flush reserves, secret scrubbing and terminal
settlement. Measure this path before designing a new ownership API.

## Diod: useful queue discipline, not a reason to add threads

[srv.c](https://github.com/chaos/diod/blob/de51d1ee1bd5ccf1d8c16b96227c8bb03ec50106/src/libnpfs/srv.c)
keeps queued and working request lists under server synchronization and uses
pthread workers in thread pools. `np_req_alloc` still allocates each request.
This provides a cancellation and ownership comparison, not evidence that threads
will improve Sophia's inline owner service. No scheduler change follows.

## Concrete candidates

The narrowest server-side candidate is completed-read storage. In
`sophia-9p/src/journal.rs::read`, a nonempty read gathers retained records into a
fresh vector. `connection.rs::read_now` returns a Read reply, then
`wire.rs::encode` copies its payload into the retained output vector. Temporary
storage can allocate or grow even though output storage is reusable.

An isolated fixture should count allocations and both copy stages for one-record
and multi-record reads, including partial offsets. Compare bounded scratch reuse
with a read-into-buffer design. Preserve acknowledgement retention and the
node/epoch delivery guard: acknowledgement must not free bytes still needed by
an unwritten reply. This is a source-proven copy path, not a whole-frame saving.

The second candidate is the SDK upload copy. Server-side
`connection/in_place.rs` already receives uploads into admitted storage; a generic
call for zero-copy reception would duplicate existing work. The client copy is
a different ownership boundary. Broad request pooling is lower priority without
allocation evidence.

## Isolated completed-read measurement

`crates/sophia-9p/tests/journal_read_cost.rs` measures the real Journal read and
wire encoder separately, with output capacity prepared outside the measured
region. One test runs all cases to avoid unrelated test threads polluting the
process-wide allocator counter. Assertions verify payload bytes, unchanged
retention until acknowledgement and final record release. The baseline below
predates exact sizing; the fixture now rejects reallocation regressions.

| 1,000 reads | Read allocations | Read reallocations | Encoding allocation operations |
| --- | ---: | ---: | ---: |
| One 256-byte record | 1,000 | 0 | 0 |
| Four records, 1,024 bytes | 1,000 | 2,000 | 0 |
| Four records, offset 17, count 700 | 1,000 | 2,000 | 0 |

All three cases also perform 1,000 empty-tail reads with zero allocation
operations. The test passes. Output is retained as `journal-read-cost.txt` in
the comparison evidence directory. These results isolate allocation churn; they
do not count native per-frame frequency, copied bytes or socket writes.

### Exact sizing result

`Journal::read` now reserves `min(count, tail - offset)` after validating the
offset and handling empty reads. All three baseline cases now use one allocation
and zero reallocations per completed read. Two additional cases request
`u32::MAX` bytes from partial offsets; the allocation equals the readable range,
not the requested count. Encoding and pending reads still allocate nothing.
Output is retained as `journal-read-cost-after.txt` in the same evidence directory.

This removes buffer growth, not the initial allocation or either payload copy.
Acknowledgement retention, read bytes and delivery guards are unchanged. The
same change and fixture are in comparator `840a6824f` to keep shared-core costs fair.
Native CPU/MiB and allocations/frame have not been measured for this change.

## Upload staging and copy accounting

`tests/upload_copy_cost.rs` exercises the vendored SDK's real `Pipeline::write`
and `poll` with a scripted Unix peer. Version negotiation finishes and its thread
joins before allocator snapshots. The fixture measures only queueing, then
overwrites the caller's payload before polling. The peer receives the original
bytes and returns a matching Rwrite. This confirms ownership of the queued copy;
it does not benchmark a shell export or authentication.

| Payload bytes | First staging allocations | First allocated bytes | Second staging allocations | Wire bytes per request |
| ---: | ---: | ---: | ---: | ---: |
| 256 | 2 | 407 | 0 | 279 |
| 4,096 | 2 | 4,247 | 0 | 4,119 |
| 65,513 | 2 | 65,664 | 0 | 65,536 |

No staging reallocations occur. The second same-size request allocates no bytes.
This is two requests per fresh connection, not a mixed-size steady-state claim.
The source's `submit` copies the payload once into `out_buf`, in addition to
23 bytes of framing. A 4 MiB upload therefore copies 4 MiB at this stage,
excluding framing and any compaction or allocator movement; this is a derived
byte count, not a measured 4 MiB workload.

An own-process strace records one successful `sendto` per Twrite for all six
requests, with the byte counts above. No partial send, EAGAIN or vectored send
occurs in this fixture. This does not predict syscall counts under native
backpressure. Retained evidence is `upload-copy-cost.txt` and
`upload-copy-syscalls.txt` in the comparison directory.

The journal fixture now reports logical bytes gathered and encoded separately.
For 1,000 reads of 1,024 bytes, each stage copies 1,024,000 payload bytes:
2,048,000 total. The partial 700-byte case copies 700,000 at each stage. These
counts follow the inspected slice copies and validated payload lengths, not
hardware copy counters; they exclude kernel traffic and allocator movement.
Results are in `journal-copy-cost.txt`.

### Journal scratch experiment

The owner approved this experiment. Connections now retain one scratch payload
only for exports that enable buffered reads. The default export method preserves
existing behavior; shell Events uses the journal's buffered method, forwarded
through `AuthenticatedFiles`. Encoding still copies into independent guarded
output before recycling the payload. Retained capacity cannot exceed the
negotiated read limit (frame limit minus 11); reset and close release it. The
shell contract accounts for this additional retained memory explicitly.

The fixture recycles a pre-sized buffer across 1,000 reads for each of the five
cases, measuring zero allocations and reallocations. The original fresh-buffer
cases still measure one allocation per read. Evidence is retained as
`journal-buffered-read-cost.txt`. Core tests verify reuse while earlier replies
are queued, partial delivery followed by revocation, pending/error cleanup,
oversized replies and scratch, and version reset. Existing journal tests retain
acknowledgement coverage. The full production core suite and 106 runtime library
tests pass (one runtime test ignored); strict core all-target and runtime library
Clippy pass. The comparator passes the same suites and Clippy checks. Native
savings were then tested in cost-11, below.

### Native cost-11 result

All eight 60 Hz pilot arms completed on comparator `8bbef6e00` (production
source `deae479d6`), with 1,000 frames each. Offline replay matches costs,
journal metrics and all kernel joins; console recovery is clean. CPU results
below come from ordinary-allocator runs; allocation counts come from separate
instrumented runs. Ranges are accounting bounds, not confidence intervals.

| Workload | 9P CPU ms/MiB | IPC CPU ms/MiB | 9P allocations/frame | IPC allocations/frame |
| --- | ---: | ---: | ---: | ---: |
| Panel | 11.526354–11.537126 | 7.641611–7.653430 | 2,321–2,324 | 1,140–1,143 |
| Panel + 4 MiB | 1.791618–1.791863 | 1.398314–1.398578 | 6,476–6,478 | 3,385–3,387 |

At interval midpoints, 9P allocations fell 0.39% and 0.38% versus cost-10.
CPU rose 1.83% on panel and fell 0.96% on bulk. The current CPU gaps to IPC are
50.8% and 28.1%; allocation gaps remain about 103.5% and 91.3%. Neither workload
passes the +10% ceiling. This supports a small allocation reduction, not a
repeatable CPU win or acceptance of the scratch candidate. Exact sizing and
scratch reuse changed together, so this pilot cannot separate their effects.

Raw captures and `offline-replay/summary.json` are under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-native-cost-11/`.
The quiet hold is released; Claude reports no work after its initial 13:06:38Z
status read and confirmation. All run directories are occupied. Further work
should attribute the remaining Session allocations before another transport API
change; the observed journal reduction is too small to explain the overall gap.

Direct encoding into queued output may remove a copy too, but needs a separate
review of rollback, output bounds and delivery guards. SDK owned vectored output
likewise remains separately scoped. Neither the copy counts nor strace establish
CPU cost, native frequency or the share of t331's remaining gap.

## Validation and limits

### Service-progress diagnostic implementation

Comparator `47ac3f2f7` adds `comparator-progress`, separate from CPU, allocation
and phase-profile features. Combining them fails compilation. Hot-path hooks
only increment relaxed atomic counters: no allocation, timestamps, formatting
or I/O. The existing sampler emits one bounded progress record every eighth
tick and at lifecycle boundaries; the peer emits at frame boundaries. Snapshots
have their own timestamp bounds and are not atomic across fields.

Counters cover Session passes, component passes, core polling turns/flushes,
accepted/stale readiness handoffs, core read/write bytes, encoded replies,
pending-read attempts, peer content polls and admitted upload/receive waits.
Core totals include every server in the process. Encoded replies include errors
and are not presentation completions; use the journal for completed frames.
The wait count excludes authentication setup. Counts can reveal amplification,
but do not by themselves prove that authority checks or idle passes are useless.

The full core suite passes with instrumentation. Focused tests prove counter
updates/snapshots allocate nothing, pending attempts do not count as replies,
real-socket byte totals are exact, and accepted/stale handoffs increment the
right counters. Runtime tests verify bounded records and explicit progress mode;
strict Clippy passes for core, runtime/peer and native CLI. The mixed-mode
negative compilation check fails as intended.

Integration `59bee7f` validates schema, process identity, monotonic counters and
timestamps, duplicate/missing fields, mixed modes and sample bounds. Its tests
and strict library Clippy pass. Native reports preserve progress samples with
`costs: null`; cost/latency arms reject progress records. Artifact, foreground
tty, duration, journal, kernel-join and recovery checks remain in force.

Four optimized one-minute plans are frozen in
`t252-authenticated-20261015/t331-native-progress-12`: files/IPC, panel/bulk,
1,000 frames each at 60 Hz. Preparation and device-free argument validation pass.
Build artifacts stay in Cargo slots; no native arm has run. The helper now selects
these fresh diagnostic captures. A new coordinated quiet window is required.
Analysis must align counter snapshots with the offered-frame interval and retain
boundary uncertainty rather than divide whole-session startup totals by frames.

### Service-progress results

All four progress-12 arms completed on comparator `47ac3f2f7` and integration
`59bee7f`. Offline journal and native-join replay match the stored reports,
with 1,000 frames each and clean console/keyboard recovery. An independent
raw-record parse matches every stored progress sample and verifies per-process
identity, order and monotonic counts. Costs remain null.

The interval spans the first peer frame-start progress snapshot through the
last frame-end snapshot. Session lower bounds subtract snapshots wholly inside
that interval; upper bounds use enclosing snapshots. This retains sampler
boundary uncertainty instead of including whole-session startup. Values below
are counts per offered frame, rounded for display; they are not CPU times or
statistical confidence intervals.

| Counter | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | ---: | ---: | ---: | ---: |
| Session/component passes | 33.08–33.16 | 8.03–8.20 | 120.07–120.12 | 45.02–45.51 |
| Core polling turns | 71.29–71.46 | 12.17–12.36 | 245.27–245.39 | 49.15–49.67 |
| Core flush calls | 138.32–138.62 | 16.06–16.40 | 489.28–489.47 | 90.04–91.01 |
| Accepted handoffs | 32.08–32.13 | 0 | 119.06–119.09 | 0 |
| Stale handoff fallbacks | 0 | 0 | 0 | 0 |
| Core replies encoded | 52.94–53.03 | 0–0.021 | 151.97–152.03 | 0–0.021 |
| Pending-read attempts | 30.08–30.12 | 0–0.021 | 114.06–114.08 | 0–0.021 |
| Peer content-poll calls | 38.567 | not instrumented | 87.046 | not instrumented |
| Peer wait API calls | 32.568 | not instrumented | 114.637 | not instrumented |

The handoff is exercised; stale observations do not explain the remaining gap
in these intervals. Files runs roughly four times as many Session passes for
panel and 2.7 times as many for bulk. This supports investigating service
amplification. It does not prove those passes unnecessary: authentication,
revocation, deadlines and protocol dependencies can require them.

Core counters include other 9P services even in the IPC arm, so do not label
their totals as shell-only traffic or infer one server's polling frequency.
Source inspection also confirms FileWire::wait_for_io can return immediately
when service is pending: peer wait counts are calls, not poll syscalls or sleeps.
The next bounded investigation should account for the roughly 53 panel and
152 bulk replies by request kind and SDK transition, then identify which
dependent exchanges force additional owner passes. Do not infer a new cache,
upload concurrency or skipped authority checks from these counts alone.

Evidence is `t331-native-progress-12/offline-replay/summary.json`, alongside
replayed journal/offers/samples and raw captures. It retains exact count bounds,
boundary snapshots and source hashes. All output directories are occupied.
Claude reports only initial 13:41–13:42Z status reads and confirmation, no later
crunch work during the hold. The hold is released.

### CPU attribution review after cost-11

Reclassified the retained perf-08 stacks by exclusive leaf, selecting exactly
the earlier Session/peer PIDs and whole-workload counter intervals. Selected
sample counts match all four original reports. This reuses existing evidence;
it is not a profile of the new scratch candidate. Values below are summed sample
periods across 1,000 frames, in milliseconds, not exact function runtimes.

| Exclusive leaf group | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | ---: | ---: | ---: | ---: |
| Recognizable allocator functions | 59.2 | 37.1 | 93.3 | 83.2 |
| memcpy/memmove | 115.3 | 80.2 | 639.9 | 544.6 |
| memset | 7.0 | 2.0 | 44.1 | 31.1 |
| Unknown leaf | 36.1 | 44.1 | 44.1 | 41.1 |
| Other leaves | 1,042.1 | 586.8 | 4,668.0 | 3,472.4 |

Recognizable allocator leaves explain about 22 ms of the sampled panel
difference and 10 ms of the bulk difference. This grouping misses inlined or
unresolved allocation work and does not include allocator-induced cache effects;
it cannot establish the full cost of allocating. Still, allocation counts alone
are a poor guide to this observed CPU gap. Copy leaves differ by about 35/95 ms,
but missing caller stacks prevent assigning most of that difference to a
particular transport copy.

The strongest named user-space differences include syscall wrappers, clock
reads, custody checks, server wake service and the Session loop. The bulk
`__vdso_clock_gettime` leaf difference is about 128 ms. Grouping clock leaves by
their nearest visible application caller puts 88/41 ms in the files/IPC Session
loop; files client poll_io accounts for another 24 ms. These are execution
samples, not call counts. A clock optimization would be premature: more clock
work may simply indicate more service turns. Perf-09 independently puts excess
on-CPU polling in several owners, not only the server poll already targeted by
the readiness handoff.

The next diagnostic should count service turns and useful progress per offered
frame on the current candidate: Session loop passes, component service passes,
server turns/flushes, client waits, requests completed and bytes advanced. Count
readiness-handoff use and fallback separately. Use fixed numeric counters and
flush snapshots outside the measured hot path; do not add formatted records or
a clock query to each call. This can distinguish extra protocol progress steps
from repeated unchanged-state service before selecting a patch. Preserve all
authority checks and deadlines; no liveness cache or once-per-loop skip follows
from these stacks. A subsequent CPU profile must use the same current candidate.

Evidence: `cpu-attribution-review.json` (source hashes and complete exclusive
leaf maps) and `clock-callers-review.json` in the Plan 9 comparison directory.
No new native run, runtime patch or acceptance change was made for this review.

The owner questioned whether the current metric is guiding the right work.
Allocations/frame is diagnostic, not a measure of responsiveness. CPU/MiB here
includes the complete Session-plus-peer workload; it does not isolate 9P codec
or transport cost. Keep the existing +10% gate until explicitly revised, but
attribute CPU stacks and allocation owners before using it to choose another
optimization. Frame deadlines, input-to-presentation latency, idle CPU and
bounded memory remain distinct product outcomes. No matched rio-versus-IPC
benchmark has been run here. Rio's separate draw path makes an unadjusted
comparison with Sophia's upload/admission/presentation workload misleading.

Inspected pinned upstream source and corresponding Sophia paths at `01fca2595`;
source hashes are retained. The follow-up changes completed-read capacity and
adds regression assertions. The full production and comparator 9P suites and strict all-target
Clippy pass. Both copy fixtures pass, and strict Clippy passes for the new upload
fixture. No native benchmark win, hardware run or task completion
follows from this experiment. Plan9port and 9legacy are catalogued;
their performance paths have not yet been audited here. Task state stays in
todo.md; this is supporting research for t331, not another execution queue.

## Connections

- [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md): accounting, bounds and acceptance criteria.
- [Cost-10 results](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#t331-readiness-handoff-pilot-results): mixed results motivating this comparison.
- [Independent uploads](gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md): separately scoped concurrency investigation.

### Extra-pass attribution and approved fixes, October 16

Source inspection explains the steady reply totals without treating every
Session pass as redundant. The panel frame uses five transactions at five
requests each, three slot-setup/cleanup requests, three pixel writes, and
11 event reads plus 11 acknowledgements: 53 replies. Bulk uses eight
transactions, two slots, 72 pixel writes and 17 read/ack pairs: 152 replies.
This is a source-derived accounting, not a per-request trace of every pass.

Launch relays amplify the cost of those passes. Previously each relay did
flush, nonblocking turn and flush on every LaunchBook poll. Files has two
such relays and the comparator's IPC path has one. Typical progress-12
intervals have 33 owner passes and 138 flushes for panel, or 120 and 489 for
bulk. Subtracting four relay flushes per files pass leaves six and nine;
IPC's typical 8/16 and 45/90 pairs fit two per pass. Aggregate core counters
cover all servers and cannot be called shell-only traffic.

The owner approved two separate fixes. Production `d817e6d8a` retains every
custody/retirement check but uses observed readiness and explicit relay
expiry instead of idle transport work. SDK `3edcd28` queues fresh writer walk
and open together, then waits for both successful replies. The shell export
executes those synchronous requests in connection order; the generic 9P API
does not gain this guarantee. Capacity fallback, failed/partial setup,
reordered replies and terminal-slot cleanup have deterministic coverage.
No writes are made concurrent, and no upload fid is cached.

Comparator `f5d0f0932` adds eight diagnostic counters: relay dispatch, idle
skip and expiry, plus walk, open, read, write and clunk requests. They use the
existing relaxed-atomic-only path. Schema 2 records remain bounded below
Linux PIPE_BUF; integration `fd2cb6c` validates them and retains schema-1 replay.
Progress mode still cannot supply CPU or allocation evidence. The relay-only
pilot keeps the old SDK so its effects can be measured separately. Native
benefit remains unproved, and the unchanged full gate still decides t331.

Four one-minute plans are frozen in `t331-native-relay-13`, with exact source
revisions and artifact hashes in `campaign.json`. All four pass preparation
and device-free argument validation, and their capture destinations are empty.
The initial validation omitted the native-scanout environment opt-in and was
refused before runtime setup; that refusal is preserved alongside the passing
checks. The installed `t252-check` selects the new plans. No native arm has run.
Claude's small push finished at 14:55:18Z, outside any new measurement window;
a fresh hold must precede the attended pilot.

Repository-wide formatting still reports the two pre-existing native-scanout
files; changed files are formatted. The default-feature workspace test command
stops at the existing `output_snapshot_allocations` test importing a native-only
module without a feature guard. The affected suites use `native-session` and an
empty configuration root. Neither limitation changes the native acceptance gate.
The full workspace run on production `70685cac7` passes with
`--features sophia-session/native-session`, `XDG_CONFIG_HOME` set to an empty
root and eight build/test workers. Its complete log, the default-feature
refusal and the formatting report are retained in `t331-native-relay-13`.

### Relay-only native results, October 16

All four relay-13 arms completed on comparator `f5d0f0932`, integration
`fd2cb6c`, with the previous SDK `d8558f4`. Independent offline replay matches
journal metrics, all 1,000 kernel/frame joins per arm and every raw progress
record. Console and keyboard recovery are clean. Evidence and hashes are in
`t252-authenticated-20261015/t331-native-relay-13/offline-replay/summary.json`.
Bounds bracket the first upload offer through the final terminal event;
counts below are per offered frame and include every core server in Session.

| Counter | Files panel, relay-13 | Files bulk, relay-13 | Previous panel / bulk |
| --- | --- | --- | --- |
| Session passes | 33.366–33.440 | 120.176–120.332 | 33.082–33.155 / 120.069–120.116 |
| Core polling turns | 5.134–5.167 | 5.141–5.170 | 71.287–71.464 / 245.272–245.389 |
| Core flushes | 5.993–6.001 | 8.990–9.001 | 138.321–138.621 / 489.275–489.465 |
| Encoded replies | 52.938–53.035 | 151.839–152.035 | 52.938–53.031 / 151.965–152.031 |

Relay dispatch and idle-skip counts match within two boundary calls on each
files arm: about 66.7 panel and 240.5 bulk per frame, with zero relay expiry
work. The shared fix also removes IPC's steady core flushes: both IPC arms
record zero. IPC Session passes are 8.028–8.814 panel and 44.991–45.071 bulk.
The wide panel upper bound includes work across sampling boundaries; it does
not establish a steady pass regression.

Request kinds independently match the source accounting: files panel has
about six walks, six opens, eleven reads, twenty-four writes and six clunks
per frame. Bulk has ten, ten, seventeen, 105 and ten respectively. This
confirms that reply traffic remains while idle relay service disappears.
The remaining Session-pass gap is expected: this pilot deliberately omitted
SDK batching. Progress instrumentation cannot establish CPU/MiB or allocation
savings, and t331 remains open.

The quiet hold is released. Claude reports no heavy work, packing, transfers
or SSH during the window; it made brief local Markdown, zk and Git inspection
calls without exact timestamps. That report is retained in `quiet-window.txt`.
Next, comparator `bf19952dd` carries the already-tested SDK pin for a separate
batching pilot. All relay-13 capture directories are occupied and must not be
reused.

### Batched writer native results, October 16

All four batching-14 arms completed on comparator `bf19952dd`, integration
`fd2cb6c` and SDK `3edcd28`. Independent replay matches journal metrics, all
1,000 frame joins and raw progress records in each arm. Console recovery is
clean. Evidence and hashes are in
`t252-authenticated-20261015/t331-native-batching-14/offline-replay/summary.json`.
The bounds use the same upload-to-terminal interval as relay-13.

| Counter per frame | Files panel | Files bulk |
| --- | --- | --- |
| Session passes | 27.426–27.469 | 109.977–110.203 |
| Core polling turns | 5.133–5.162 | 5.137–5.166 |
| Core flushes | 5.996–6.001 | 8.987–9.001 |
| Encoded replies | 52.975–53.035 | 151.740–152.035 |

Against relay-13, batching removes about six panel and ten bulk Session passes
per frame: roughly 18% and 8.5%. Reply counts remain about 53/152, consistent
with combining setup waits rather than dropping protocol operations. Idle
relay skips still cover almost every dispatch; expiry and stale handoff counts
are zero. IPC records 8.018–8.040 panel and 45.020–45.078 bulk passes per frame,
with zero steady core flushes. These are diagnostic counts, not cost results.

Claude reports no crunch activity during the hold from 15:25:26Z to release,
except its local-socket confirmation. The report is retained in
`quiet-window.txt`. All captures are occupied; the hold is released. Next,
measure CPU and allocations separately on this candidate before considering
the unchanged full gate. t331 remains open.

### Cost-15 results and fresh profiling, October 16

All eight separate CPU/allocation arms completed on comparator `bf19952dd`,
integration `fd2cb6c` and SDK `3edcd28`. Offline replay matches stored costs,
journal metrics and all 1,000 frame joins in every arm. Console recovery is
clean. Evidence is in `t252-authenticated-20261015/t331-native-cost-15`, with
raw-log hashes in `offline-replay/summary.json` and bounded comparisons in
`offline-replay/comparison.json`.

| Metric | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | --- | --- | --- | --- |
| CPU ms per uploaded MiB, CPU-only | 10.078–10.085 | 7.695–7.707 | 1.6784–1.6787 | 1.3725–1.3729 |
| Allocations/frame, Rust-global | 2,088–2,090 | 1,140–1,142 | 6,089–6,091 | 3,386–3,388 |

Compared with cost-11, files CPU falls 12.51–12.65% for panel and 6.30–6.33%
for bulk. Allocations fall 9.95–10.15% and 5.95–6.00%. These are comparisons
between pilots, not repeated confidence estimates or whole-desktop savings.
Both relay readiness and SDK batching changed; their individual cost effects
cannot be separated by this pilot. The paired IPC gaps remain 30.77–31.06%
and 22.25–22.31% for CPU, and 82.84–83.33% and 79.72–79.89% for allocations.
The +10% gate fails; t331 remains open and the installed desktop is unchanged.

The hold is released. Claude reports light local source edits, zk/Git reads
and small local copies, but no builds, rendering, tests, heavy jobs, packing,
transfers or SSH during the window. Exact light-work timestamps were not
supplied; retain that limitation rather than claiming a completely idle host.

The owner requested fresh attribution before another optimization. Four
one-minute CPU profiles are prepared in `t331-native-perf-16` with the exact
cost-15 CPU-only artifacts. Sampling uses `cpu-clock:u`, 997 Hz, monotonic
clock and `dwarf,8192` stacks, matching perf-08. All source/artifact checks,
device-free validation, launcher syntax and a small permission smoke pass.
No desktop capture has started; a fresh hold precedes the attended run.
Analysis must select the Session/peer PIDs and the upload workload interval,
compare exclusive leaf periods and resolved callers, and report missing
stacks. User-space samples omit kernel execution and cannot alone explain
all process CPU or allocation counts. Do not use sampled costs for acceptance.

Perf-16 follow-up: only the first files-panel arm ran. Its process capture
completed and all 1,000 journal uploads and cost records replay, but the
presentation join fails because native completions use out-fence retirement
and observation-time fallback. Console recovery is clean. `result.json` says
`captured_only`; it precedes postprocessing and does not prove a joined report.
No `native-samples.json` was written, and the launcher stopped before IPC/bulk.
The timestamp reducer marks out-fence completion as non-kernel without marking
it as a missing page-flip timestamp; that explains the logged combination
`observation_fallback missing_kernel_timestamp=0`. It does not establish why
out-fence completion won throughout this run or implicate sampling as its cause.

The retained partial profile has 1,230 user-space samples in the workload
interval for Session 7616 and peer host PID 7701. The peer reports namespace
PID 1 in counters; perf identifies its unique host process. Selection and raw
hashes are in `t331-native-perf-16/analysis/partial-profile.json`. Without the
matched IPC profile, this cannot attribute the remaining differential cost.
The hold is released; Claude reports only light local Git/text work.

A fresh four-arm attempt is prepared in `t331-native-perf-17` with unchanged
binaries, sampling settings and strict presentation checks. Preparation,
device-free validation, artifact hashes and permission smoke pass. No capture
has started. If the same refusal recurs, investigate completion selection
before another run; do not relax acceptance or overwrite perf-16.

### Perf-17 panel attribution and bulk refusal, October 16

Perf-17 produced a valid files/IPC panel pair on the unchanged cost-15
artifacts. Offline journal/cost replay and all 1,000 presentation joins match
both reports. The bulk files arm completed its uploads and cost replay but
failed the exact kernel timestamp join; the launcher stopped before IPC bulk.
All three captures recovered the console cleanly. Raw evidence is under
`t252-authenticated-20261015/t331-native-perf-17`; selected samples, host PIDs,
intervals, source hashes and full leaf maps are in `analysis/profile-summary.json`.

| Exclusive leaf group, sampled ms across 1,000 panel frames | Files | IPC |
| --- | ---: | ---: |
| Copy | 136.4 | 79.2 |
| Recognizable allocator functions | 52.2 | 31.1 |
| Fill | 6.0 | 9.0 |
| Unknown | 41.1 | 35.1 |
| Other | 951.9 | 570.7 |

Selection uses Session/peer host PIDs 13508/13592 for files and 14421/14505 for
IPC, bounded by the workload counters. The peer counter PID is namespaced;
its unique perf command identifies the host PID. There are 1,184/723 samples,
including 119/148 single-frame stacks. User-space sampling omits kernel CPU,
misses some inlined/unresolved work and includes sampling perturbation. The
instrumented CPU totals are not acceptance measurements.

Largest named exclusive differences include memcpy (+57.2 ms),
`LaunchCustody::is_live` (+47.1 ms), `__vdso_clock_gettime` (+41.1 ms) and
`__internal_syscall_cancel` (+37.1 ms). Custody performs a zero-time poll of
both kernel identities plus active-flag checks; preserve its fail-closed
contract. Inclusive differences put component service at +189.6 ms and the
files transport turn at +139.4 ms. Those stacks overlap and must not be added.
They support investigating repeated service and concrete copy owners rather
than treating allocation count as the entire CPU gap. No cache, skipped
custody check or speculative optimization is authorized by a sample.

Bulk records 436 kernel completions, ending at native frame 437; frame 438
starts observation fallback, which persists for the remaining 567 records.
`frame_retirement.rs` uses a signaled out-fence when no pending callback is
available. `head_completion.rs::synthesize_out_fence_callback` permanently
sets `OutFenceAuthoritative`; `queue_page_flip_callback` then discards later
page-flip events. This explains persistence, but the first race/ordering cause
is still unproved. The sticky mode protects against mixing kernel and synthetic
serial bases, so removing it would not be a safe profiling fix.

The hold is released; mini Claude reports no crunch activity beyond its
confirmation and release message. All three captures are occupied. Before any
new native run, inspect event-pump ordering and reproduce the first transition
deterministically while preserving retirement safety and timestamp accuracy.
The partial bulk profile cannot support a paired differential claim. t331
remains open; no desktop installation or acceptance follows.

### Completion ordering reproduction, October 16

Ordinary `service_native` pumps every DRM card once, then performs retained
image, preview and frame-service work before `retire_ready` polls the fence.
A page-flip event can become readable after that first drain. The head ledger
still looks empty, so a signaled fence selects the irreversible fallback.
Only the hard-stall path currently performs a final collect-and-retire pass.
The same empty-ledger/fence choice exists in singleton and mirror retirement.

A device-free test in `tests/support/native_head_completion.rs` drives the
production native poller and physical-head reducer in both arrival orders.
An empty first read followed by fence synthesis causes the later, timestamped
event to be discarded; collecting that event before fence selection retains
PageFlipPreferred and the exact timestamp. All six head-completion tests pass,
including the existing topology-reset and late-event serial safety checks.
The test controls readiness and models the retirement choice; it does not run
DRM hardware or prove the precise event-arrival instant in perf-16/17.
The first test command used the filename as a filter and selected zero tests;
the corrected `head_completion` filter ran all six.

The narrow repair to evaluate is a card-scoped nonblocking recheck immediately
before the first transition from PageFlipPreferred to fence authority, only
when a signaled fence has no queued event. Route a newly collected event and
its timestamp through the existing ledger; otherwise retain fence fallback.
Do not add another card read to ordinary successful event retirement, reset
sticky authority, fabricate timestamps or wait indefinitely for a lost event.
The recheck must handle shared-card siblings, read failures, draining and
mirror retirement without duplicate intake. An event that becomes readable
only after the recheck can still require fallback, so this is not a guarantee
of kernel timestamps on every frame. Any grace interval or exact submission
identity redesign is a separate decision, not hidden inside this repair.

No runtime fix is implemented by this checkpoint. Existing native artifacts
remain frozen; no further physical run is prepared. First implement and test
the bounded recheck, then evaluate its diagnostic benefit without relaxing
native acceptance.

### Bounded completion recheck implementation, October 16

Signed production `93c542e17` and comparator `075fdfa21` now recheck the affected
card immediately before first fence fallback in singleton and mirror retirement.
The recheck runs only for PageFlipPreferred with no queued callback after a
signaled fence. Events and timestamps use the existing card router; a collected
event wins, otherwise the existing irreversible fallback remains. Heads already
using fence authority and ordinary queued-event retirement add no card reads.
A read/routing failure propagates instead of synthesizing a completion; mirror
service records the error while preserving its existing cleanup path. Drain
retirement uses the same singleton/mirror paths.

The two new checks cover recheck eligibility across event/fence/topology states
and routing both heads of a shared card with their distinct serials/timestamps.
The earlier arrival-window reproduction now calls the production completion
selection method. Existing late-event and topology-reset safety checks remain.
Production backend library tests pass (353, one ignored), DRM-event integration
tests pass (316, one ignored), and strict library Clippy passes. Comparator's
eight head-completion tests pass. These device-free fixtures exercise poller,
head state and selection, not a live DRM card or an end-to-end hardware fault.

This closes the stale first-drain window when the event is already readable at
the recheck. It cannot promise kernel timestamps if the event arrives later or
is lost. The strict native join remains unchanged. No custody checks, limits,
protocols or installed desktop files changed. A fresh four-arm profiling pilot
on the repaired comparator follows; old captures remain retained.

### Perf-18 complete paired profiles, October 16

All four profiles completed on repaired comparator `075fdfa21` and integration
`fd2cb6c`. Offline replay exactly matches journal metrics, costs and all 1,000
presentation joins per arm. Each arm has 1,003 native completion records, all
kernel-timestamped, with clean console/keyboard recovery. This is a successful
pilot on the repaired candidate, not proof that the recheck executed or that
all future event/fence races are eliminated. Evidence and raw hashes are under
`t252-authenticated-20261015/t331-native-perf-18/analysis`; `profile-summary.json`
contains selections and leaf maps, and `callers.json` contains caller grouping.

| Exclusive leaf group, sampled ms across 1,000 frames | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | ---: | ---: | ---: | ---: |
| Copy | 121.4 | 79.2 | 601.8 | 554.7 |
| Recognizable allocator functions | 49.1 | 44.1 | 76.2 | 80.2 |
| Fill | 8.0 | 6.0 | 33.1 | 36.1 |
| Unknown | 41.1 | 44.1 | 38.1 | 41.1 |
| Other | 943.8 | 542.6 | 4,329.0 | 3,432.3 |

The selected files/IPC samples number 1,160/714 for panel and 5,063/4,132 for
bulk. Single-frame stacks number 225/151 and 814/728 respectively. Selection
uses host Session/peer PIDs and the whole upload interval. User-space sampling
omits kernel time, lacks many callers and perturbs execution; sampled periods
are not exact function runtimes. These profiles cannot replace cost acceptance.

The strongest bulk exclusive differences are libc syscall wrappers (+232.7 ms
for `__internal_syscall_cancel`), vDSO clock reads (+130.4 ms), custody liveness
checks (+107.3 ms), Session loop (+54.2 ms) and copies (+47.1 ms). Grouping all
syscall-named leaves by their nearest visible Sophia caller attributes a
+252.8 ms bulk difference to SDK `Pipeline::poll` (panel +38.1 ms). This names
user-space call-path samples, not kernel syscall duration or counts. Custody
callers split between file authentication polling (+55.2 ms bulk) and authority
authorization (+52.2 ms). Both checks retain their existing security contract.

Recognizable allocator leaves differ by only +5.0 ms panel and -4.0 ms bulk in
this sample. That does not make allocation free: inlining, unresolved frames
and cache effects remain outside this grouping. It does make raw allocation
count a weak guide to the largest measured CPU opportunities. Copy callers
remain mostly unresolved; do not assign their difference to a particular
buffer from this profile alone. Inclusive stack totals overlap and are not
additive.

Next investigate the SDK's bounded `Pipeline::poll` read/write cycle. Source
inspection shows each call writes queued output then attempts input reads;
the shell drive may call it repeatedly while draining replies and scheduling
follow-up requests. Count useful bytes/replies against WouldBlock reads and
writes in isolated diagnostics, and examine whether actual readiness can be
retained safely across the wait/drive boundary. Preserve EOF/HUP handling,
partial frames, backpressure, fairness budgets and all upload barriers. Do not
add a readiness syscall before every read or cache custody liveness on the
strength of these samples. A measured redundant-call pattern should precede
an optimization proposal.

The hold is released; mini Claude reports no crunch activity beyond its
coordination messages. All four capture destinations are occupied. No further
attended run is needed to begin the SDK investigation. t331 remains open;
installed desktop and acceptance thresholds are unchanged.

### SDK socket-call investigation, October 16

The isolated SDK upload fixture on `3edcd28` now has retained syscall evidence
in `t252-authenticated-20261015/t331-sdk-io-19`. A temporary test-only marker
surrounded each ShellConnection poll; external strace counted only calls
inside those markers. Handshake and scripted-peer pumping were excluded.
Three optimized runs each covered a normal 128 KiB upload and the existing
short-write/protocol-EAGAIN fixture. All six tests passed. The marker patch,
executable identity, raw traces, summaries and hashes are retained; SDK source
was restored and its worktree is clean. No production counters were added.

| Post-handshake client calls | Normal, all three runs | Short-write/EAGAIN, three runs |
| --- | ---: | ---: |
| Shell poll calls | 18 | 99–102 |
| Successful socket sends | 16 | 23 |
| Socket reads returning bytes | 14 | 21 |
| Socket reads returning WouldBlock | 34 | 124–127 |
| Socket sends returning WouldBlock | 0 | 0 |
| Read bytes | 609 | 686 |
| Sent bytes, including protocol/retries | 131,952 | 349,082 |

Sixteen normal Shell polls contain multiple WouldBlock reads. However, all but
one repeated empty read within a call follow a successful new send. The peer
could have answered that send in a real concurrent deployment; these are not
proven redundant reads. Even an empty read without an intervening send can
observe an asynchronous event, so absence of a send is not a safe skip rule.
The fixture alternates peer and client on one thread after handshake, making
it useful for transition counts but unsuitable for native readiness frequency.
The short-write fixture's EAGAIN is a 9P error reply, not socket backpressure.
Its extra empty polls reflect deadline retries in the busy-driving fixture;
strace perturbs their number. Do not extrapolate these counts to CPU savings.

Source attribution matches the trace: Pipeline::poll flushes queued output,
then reads to WouldBlock within fixed budgets. FileWire::drive drains replies,
queues dependent requests and repeats while state changes. This explains two
empty reads around useful progress without establishing that either is safe
to suppress. A broad readiness cache is not justified by this experiment.

A narrower clock opportunity is explicit in source: FileWire::drive evaluates
Instant::now before every drive_submit_retry call; the retry method immediately
returns when the current submission is not in RetryWait, and also need not
read time for same-pass refusal or event-driven progress. Move the clock read
behind those state checks, retaining the existing deadline comparison when
it actually controls retry eligibility. Existing scripted tests cover retry
after events, deadline expiry and no retry within the refusal pass. This is
the next bounded candidate, not a measured CPU win; no SDK behavior change is
implemented at this investigation checkpoint. Keep custody and socket error
handling unchanged. Broader socket scheduling needs independent readiness/
EOF/backpressure evidence before implementation.

### Conditional retry clock implementation, October 16

Signed SDK `510a434` is pushed on the t331 performance branch and vendored in
production `e7cf62692` and comparator `9c527d566`. The drive loop no longer
reads time before calling retry handling. Retry handling queries Instant only
after finding RetryWait, rejecting the refusal pass and checking for event
progress. The existing deadline comparison, request bytes and custody rules
remain unchanged. No socket read is skipped and no liveness result is cached.

The SDK workspace tests, all-target Clippy and formatting pass. Sophia's signed
vendor gate passes, as do 12 runtime interoperability tests in each checkout,
including independent C protocol and custody checks. Existing scripted cases
exercise deadline retry, event-driven retry and refusal-pass suppression.
Native savings are unmeasured; the change removes a concrete unnecessary clock
query but does not establish its share of the sampled clock cost.

The next four-arm CPU-only pilot, `t331-native-cpu-20`, compares the current
files/IPC panel and bulk workloads without perf sampling or allocation
instrumentation. It includes both the completion recheck and conditional retry
clock since cost-15; a difference from cost-15 cannot isolate the clock change.
This pilot cannot supply allocation evidence or replace the full paired gate.
