---
id: h38nf8bk
date: 2026-10-20
kind: plan
tags: [plan, rendering, gpu, 9p, performance, t339]
---
# Qualify GPU native shell images through owned capture

## Current resumption — October 23, 2026

The same-host, same-peer ring-01 comparison remains authoritative: DMA uses
38.6% more Session-plus-peer CPU at 1920x24 and 33.0% less at 1920x512.
Upload-begin-to-kernel latency has no resolved difference. Keep owned-ring
default and DMA experimental and explicitly opt-in; t339's promotion gate
remains unmet. All eight captures had exact joins and clean shutdown.

Profile05 confirms that notifying Session of the physical destination-prepare
reply removes the roughly 10 ms settlement stall. It replaces a deadline wake
with a ring wake; owner passes stay near 17 per offer versus R's 12. There is
no resolved CPU or upload-to-kernel latency improvement. The remaining excess
is about four fd-ready waits plus one prepare notification per offer.

Signed Sophia `26f4c583d` adds per-role component readiness counters without
changing polls, subscriptions or scheduling. It separates the 9P internal wake
reader from peer sockets, image traffic, CPU image-worker and capture completion.
Profile06 completed both captures cleanly with the same host and peer binaries.
R takes 12.082 owner passes per frame and G 16.988. G has two internal 9P wake
observations per frame, absent in R, and about three more wire-socket readiness
observations. CPU-worker completion in R swaps one-for-one with capture
completion in G. Roles overlap; these are not exclusive pass or CPU costs.
Evidence: `~/.local/state/t331/t339-gpu-profile-06/analysis/REPORT.txt`,
`audit.json` and the independent `claude/` review.

Signed candidate `6a2a774bf` flushes already-journaled capture publications at the end
of maintenance, instead of deferring their delivery until another owner pass.
It keeps custody checks and consumes no new 9P request or image packet.
The ordinary ResourceStatus drain still precedes SourceReleased. Profile07
confirms both internal wake observations disappear, but overlapping readiness
means only one owner pass is saved: 16.988 to 16.004 per frame. CPU intervals
overlap and delivery latency is unchanged; this is not a small-bar CPU fix.

The focused runtime socket regression passes all five capture-intake cases.
It holds event reads across admission and release and proves delivery through
the publication flush alone, while a Source already queued on the image socket
stays unread. Removing the flush calls makes the test fail. Strict all-target
clippy passes for runtime and Session with native-session and capture enabled.
No new SDK contract, protocol record or dependency is introduced.

Profile07 completed cleanly: 2,120 exact joins, 2,123 monotonic completions,
zero protocol errors and retained native resources, quiescent shutdown and
clean PAM/VT. Evidence is `~/.local/state/t331/t339-gpu-profile-07/analysis`.
Session CPU is 1230–1250 us/frame and peer 670–690 us, versus profile06 G's
1225–1245 and 660–680 us. Upload-begin-to-kernel median is 16,316 us versus
16,313 us. Remaining excess over R is the prepare notification plus roughly
three peer-wire readiness observations; completion is a one-for-one swap.

Signed candidate `58443cd06` journals completed ResourceStatus and SourceReleased in
order before one flush. It retries only an exact offer whose release is held
for its status, moves store events and the whole existing outbox in FIFO order,
updates output pressure, and preserves journal credits and retry on saturation.
No new input, completion claim, wire format or SDK batching layer is involved.
The SDK already supports bulk journal reads and cumulative ACK batching.

The gated runtime regression now holds one read before settlement, then takes
Source, fails the capture, releases and flushes without an ordinary service turn.
That single read must deliver exactly ResourceStatus then SourceReleased. Wrong
grant and unknown-offer refusals leave the store status pending. Admission still
flushes without consuming a waiting Source. All five runtime intake cases pass;
Session capture tests and strict all-target runtime/Session clippy pass with
native-session and experimental-shell-capture. Full-journal retry is reviewed
and retains existing custody tests; no new full-journal wire fixture was added.

Profile08 is the bounded G-only diagnostic against profile07, with the same peer,
fresh producer allocations and instrumentation. Package and command:
`~/.local/state/t331/t339-gpu-profile-08/PLAN.md` and
`sudo /usr/bin/python3 ~/.local/state/t331/t339-gpu-profile-08/ssh/run.py`.
The native CPU, pass-count and delivery effect of coalescing is not yet measured.

No source reuse, cached prepare bounds, GL flush deletion or further producer
synchronization change is planned. The small-bar CPU regression remains
unresolved. GPU execution, physical allocation peaks, input latency and full
render-start delivery latency remain unmeasured.

## Prepare notification result and readiness diagnostic — October 23, 2026

Profile06's 1,800-offer windows have wire sockets R 9.082 / G 11.986,
internal wire wake 0 / 2.000, image socket 0 / 0.891, image-worker completion
1 / 0 and capture completion 0 / 1 per offer. Authentication, negotiation and
listeners are quiet. Every fd-ready wait includes a component descriptor;
metadata-shell or relays can still overlap. G's role sum exceeds component_any
by 1.888 observations per frame, which does not count distinct overlapping
waits. The prepare notification adds one ring wake; deadlines are one in both.

Profile06 Session CPU/frame is R 950–970 us / G 1225–1245 us; peer is
490–510 / 660–680 us. Total is 1440–1480 / 1885–1925 us. Upload-begin-to-kernel
medians are 16289 / 16313 us. Each run has 2,120 exact joins, 2,123 monotonic
completions, clean protocol accounting, zero retained native resources,
quiescent shutdown and clean PAM/VT. Perf records no lost or throttled events.
These are single diagnostic runs, not new acceptance intervals.

Code review identifies a bounded scheduling opportunity: component input
service precedes capture maintenance, whose admission and release calls mark
the journal pending and ring its internal reader. A publication-only flush
at maintenance end drains that reader and retries waiting journal reads,
without accepting sockets or receiving Source packets. Channel publication
retries remain in ordinary service. ResourceStatus is still journaled by the
ordinary outbox drain; the flush does not skip that ordering or manufacture
native completion. Transport errors remain subject to the existing connection
owner's next turn. Peer protocol batching remains unexamined.

Profile05 uses signed Sophia `b69fe94e3`, integration `7aa233d` and the unchanged
peer `d6e50a53…`. All 2,120 generations join exactly, all 2,123 completions use
the monotonic kernel clock, protocol tally is zero, and shutdown is quiescent
with zero retained native resources. PAM/VT cleanup succeeds without residual
processes. Evidence: `~/.local/state/t331/t339-gpu-profile-05/analysis/REPORT.txt`,
`custody-audit.json`, `control-comparison.json` and `control-review/`.

DMA upload begin to UploadAccepted falls from 11,535 to 1,453 us median.
That event is emitted after Accepted, SourceReleased(CaptureComplete), local
image_forget and frame destruction: it observes settlement, not isolated
admission. R's RingAccepted is emitted immediately on ResourceStatus, so the
two boundaries are not interchangeable. image_forget is local bookkeeping,
not a wire turn. Upload begin to kernel completion stays 16,369 to 16,340.5 us:
the same vblank is reached; no delivery improvement is established.

Paired 1,800-offer windows give owner passes 17.032 to 16.993, fd-ready waits
14.035 to 14.001, deadline wakes 1.996 to 1.000 and ring wakes 0 to 1 per offer.
R profile01 had 12.076 passes and 10.076 fd-ready waits. Profile05 CPU/frame is
Session 1,230–1,250 us and peer 635–655 us, total 1,865–1,905 us versus profile04's
1,945–1,985 us. The midpoint change is about 4.1%, below the frozen 6.2% variation
floor, with one diagnostic run per candidate. Untouched copy/import code also
became faster; do not credit that variation to the notification change.

Profile06 emits cumulative saturating component counters in sophia_owner_wait.
Each role counts at most once per wait, roles can overlap, and waits count only
when component descriptors were registered. owner_fd_ready minus component_any
counts waits ready only outside components, not all metadata-shell or relay
readiness. WireWake is the existing first 9P descriptor; Wire is listener and
streams. No descriptor identities are logged. Compare paired cumulative windows
without interpolation, reject resets, and retain the existing custody/cleanup
checks. No performance qualification follows from this diagnostic alone.

Validation: the 9P wake-reader position test, component counter tests, combined
wait fixture and diagnostic numeric allowlist tests pass. Strict all-target
clippy passes for 9P, runtime and Session with native-session and capture enabled.
The public SDK wire contract and client APIs are unchanged.

## Small-bar profiling and optimization — October 23, 2026

niltempus requested root-cause profiling and optimization of the small-bar
regression, and approved bounded producer-buffer reuse if allocation/import
churn dominates. The measured excess is about 339 us/frame in Session and
193 us/frame in the peer; per-function attribution is not yet established.
The first implementation adds default-off producer stage CPU/wall counters
and an explicit GPU R/G `capture-profile` mode in integration tooling. Normal
CPU comparison plans reject this instrumented mode. A two-run h24 profile
package is prepared at `~/.local/state/t331/t339-gpu-profile-01`: R then G,
60 seconds each, with user-space perf call stacks and unchanged fresh-buffer
allocation. Signed pins are Lom `f7e36c72` and integration `7b7fe06`; the peer's
locked build, seven tests and strict Clippy pass. Both package check-only
passes verified pins and idle tty4, and all 26 package manifest entries verify.
The R profile completed with clean native shutdown, protocol tally and PAM/VT
cleanup. All 2,120 frames joined exactly, but post-run cost dispatch rejected
`capture-profile`, so G never launched. The existing offline collect/metrics/join
commands recovered the joins under `t339-gpu-profile-01/analysis/r-recovery`,
with input hashes and the original evidence unchanged. Perf covers both processes;
its limited samples and truncated stacks support coarse attribution only.
Signed integration `2fcee54` fixes that dispatcher, with a focused regression
test and strict Clippy passing. The G-only continuation `t339-gpu-profile-02`
completed cleanly with unchanged host, peer and profiling recipe. Both profiles
have 2,120 exact joins, 2,123 monotonic completions, zero retained imports and
quiescent native shutdown. R's outer exit remains 1 from the recorded parser
failure; recovery does not rewrite it as a successful launcher exit.

Diagnostic process CPU/frame reproduces the excess: R Session 935–955 us and
peer 485–505 us; G Session 1265–1285 us and peer 675–695 us. G producer stage
thread CPU/frame is setup 29.6 us, draw 296.3 us, release 99.4 us, export 7.5 us
and texture destruction 11.7 us, each with exactly 2,000 observations. These
brackets include the calling thread's syscall CPU. User-space perf estimates
peer CPU at about 337 us for R and 352 us for G; their small difference suggests
much of the excess is syscall/kernel work, but does not independently measure
individual syscalls. Samples and truncated stacks support coarse attribution.

The host's whole-run capture setup/copy/cleanup partition averages about
1.4/94.7/3.6 us per offered frame. It imports 2,120 sources with no cache hits;
the Session owner also takes about five extra turns per frame. These costs
explain why a small copied bar can beat the DMA path, but the per-turn CPU and
the import/copy split are not independently measured. Raw counters, process
windows, perf reports and custody audit are under `t339-gpu-profile-02/analysis`;
producer attribution is in its `producer/summary.md`. These diagnostic runs
do not replace the frozen uninstrumented comparison.

Lom `b8ec26a`, pinned by integration `7aa233d`, removes the producer's redundant CPU wait between drawing
and submitting the foreign release on the same queue. The final release fence
must still prove both completed before descriptor export, followed by bounded
wgpu cleanup. Every failure after drawing is submitted retains the source and
target; uncertain release additionally retains its pool and fence. Re-profile
the combined draw-plus-release cost, because the wait moves between brackets.
Do not add persistent command objects or source reuse yet: setup/export/drop
account for only about 49 us/frame, and the release's internal syscall split
is unmeasured. Lom's full offline gate and the pinned peer's seven tests pass.
The existing renderD128 smoke and two-process Vulkan/EGL pixel oracle pass,
including earlier-capture independence, frame cap and readback after export;
receipts are in `development-evidence/t339-20261023/release-wait-01`. Unknown
completion errors were reviewed, not fault-injected. The G-only re-profile
`~/.local/state/t331/t339-gpu-profile-03` completed with all 2,120 exact joins,
2,123 monotonic completions, clean protocol tally, zero retained resources and
restored PAM/VT state. Combined draw/release CPU was 395.72 -> 390.93 us/frame
and wall time 676.41 -> 590.56 us/frame. Whole-peer CPU intervals 675–695 and
670–690 us/frame overlap; Session stays 1265–1285 us/frame. One profile each
supports a wall-time improvement, not a resolved CPU improvement or acceptance
verdict. Keep the simpler wait sequence and investigate host costs next.

Sophia `23426521e` adds opt-in source-import CPU/wall timing nested inside Copy,
plus source eviction and transient-import counts, carried through renderer,
backend and reduced Session diagnostics. Copy still subtracts only measured
Cleanup; the first glFlush and all source/capture lifetimes are unchanged.
Renderer checks report 302 passed, 26 ignored; six diagnostics tests pass.
Strict Clippy passes for the renderer crates and native capture backend/Session,
and the release build passes. Changed-line formatting and diff checks pass;
pre-existing formatting differences remain in scanout/context.rs and diagnostic
records.rs. The prepared profile04 freezes the same peer and profiling recipe
as profile03, with this instrumented host. Import includes any cache eviction
inside lookup, but not driver validation deferred until draw/flush. Compare
paired cumulative observations, check for resets, and keep whole-run averages
separate from the kernel-anchored CPU window.

Profile04 subsequently passed all 2,120 joins, 2,123 monotonic completions,
protocol and shutdown checks, with PAM/VT restored and no retained resources.
The paired 1,800-offer observation measures setup/copy/cleanup at
1.31/91.88/3.35 us CPU per offer. Import is 31.49 us inside Copy, including
about one eviction per offer; there are no cache hits or transient imports.
Sixteen source imports remain cached during steady state and all are retired
at shutdown. Whole-run Import averages 31.81 us. Session CPU is 1270–1290 us
and peer 675–695 us per frame, overlapping profile03. User-only perf retains
727 samples without reported loss or throttling; it does not independently
attribute kernel syscall cost. See `analysis/REPORT.txt`, `custody-audit.json`,
`cpu-diagnostic-results.json` and `control-comparison.json` under profile04.

Sophia `b69fe94e3` forwards the existing Session owner notifier to destination
preparation and uses the existing SignalSender for publication and disconnect.
The worker retains one wake attachment for its Session lifetime. OwnerWake is
cleared before queue inspection at the start of a pass. No cached bound,
prepared-ahead allocation or new polling loop replaces physical preparation.
The first attachment rings once, and a refused command enqueue can ring
spuriously; notifications are only hints to inspect state. Capture installation
itself still uses the old unnotified reply and is outside this change.
Backend worker tests pass 43 with one ignored; Session capture tests pass 77
with nine ignored, plus the subprocess target. Strict all-target Clippy passes
for backend and Session with native capture. One old shutdown fixture needed
its feature-gated native argument; no shutdown implementation changed.
The preparation-delay and CPU effects still require a native measurement.

The reuse design preserves the fresh API and adds a consuming unsafe reuse
operation, with same-preview identity and the original four-frame bound.
The sequential peer needs one reusable frame. Native code confirms that
CaptureComplete requires observation of the exact EGL copy fence signalling;
the peer must also observe Accepted and forget the offer before reuse.
FOREIGN acquire must restore the layout expected by wgpu before drawing;
release remains fenced. Unknown completion retains the allocation and command
objects. Rewriting the same source BO must leave its earlier captured
destination unchanged in the native oracle before any timed reuse comparison.

## Current owned-ring comparison result — October 22, 2026

Ring-01 ran R/G/G/R at each height on HDMI-A-1, output 1, 1920x1080@60,
scale 1, VRR off, RX 7900 GRE. Both arms use the same GPU scene, SDK, peer
executable and Sophia executable. R pays GPU readback into a Vec followed by
the CPU copy into one persistent SDK ring; G exports a fresh producer-completed
DMA-BUF for owned capture. This removes arms03's host-revision confound.

| Surface | Owned-ring CPU/frame | DMA CPU/frame | DMA versus owned-ring |
| --- | ---: | ---: | --- |
| 1920x24 | 1.376 ms | 1.908 ms | 38.6% more CPU |
| 1920x512 | 2.789 ms | 1.868 ms | 33.0% less CPU |

CPU covers 2,000 kernel-anchored intervals after 120 warmup frames. The table
uses arm midpoints; repeat ranges are R [1372.5,1380] and G [1905,1910] us at
h24, R [2780,2797.5] and G [1865,1870] us at h512. Repeat widths are below
0.63%, within the frozen 6.2% veto. Both CPU differences are disjoint and
exceed 10% of the slower midpoint. The small regression is 27.9% when expressed
relative to the slower DMA result; 38.6% above uses owned-ring as the baseline.

Primary latency uses upload begin to first exact kernel completion for
generations 121–2120. Run medians are R 16.306/16.313 ms and G 16.318/16.297 ms
at h24; R 15.887/15.888 ms and G 16.325/16.315 ms at h512. Small ranges overlap.
The large difference is 0.4325 ms (2.72% relative to ring), below the frozen
10% slower-normalized threshold: neither height has a resolved latency change.
DMA still spends about 11.5 ms from upload begin to Accepted before submitting.
Producer rendering/readback/export before upload begin is outside this metric.

Each run has 2,120 unique workload joins and 2,123 native completions carrying
queried DRM monotonic proof, with no observation fallback. R's schema-7 records
observe slot 0 throughout, two configured slots, exact frame capacity and null
unexposed admission times/slot generations. G retains schema 2. Cleanup and
custody checks pass independently of the performance result.

Two repeats on one machine/output do not establish a general crossover size.
No GPU timing, peer/GPU allocation peaks, readback-byte counter, full render-start
or input-latency workload was captured. These remote runs add no visual claim.
The scoped conclusion is a large-surface CPU saving, not uniform promotion or
an automatic transport-selection policy. No exit criterion is waived.

## Matched GPU comparison result — October 22, 2026

Evidence: `~/.local/state/t331/t339-gpu-arms-03/analysis-01/REPORT.txt`,
`summary.json`, per-height CPU plans/results, per-run latency distributions,
and `MANIFEST.sha256`. The report retains the failed earlier attempts and all
measurement limits. The signed inputs are Sophia `bfa477e93`, comparator
`25840757f`, integration `8aa0c19`, and the unchanged frozen GPU peers.

The frozen order was I/S/G/G/S/I at each height on HDMI-A-1, output 1,
1920x1080@60, scale 1, VRR off, RX 7900 GRE. CPU covers 2,000 kernel-anchored
intervals after 120 warmup frames; latency distributions include all 2,120
frames. All repeat intervals stay inside the frozen 6.2% band; the maximum is
4.4%. Cross-arm CPU differences below pass the disjoint-interval and greater
than 10% slower-normalized rule. Two repeats are not a broad noise estimate.

| Surface | IPC CPU/frame | Sealed ISB CPU/frame | DMA CPU/frame | DMA versus ISB |
| --- | ---: | ---: | ---: | --- |
| 1920x24 | 1.140 ms | 1.603 ms | 1.883 ms | 17.5% more CPU |
| 1920x512 | 4.246 ms | 5.566 ms | 1.860 ms | 66.6% less CPU |

These are midpoints of mirrored-repeat CPU attribution intervals, not exact
per-frame execution times. At the large size DMA also uses 56.2% less CPU than
IPC; at the small size it uses 65.1% more. All G contrasts retain the master
versus comparator host-revision confound.

The predeclared submit-to-kernel latency median is about 4.74 ms for DMA,
versus 15.85/11.93 ms for sealed ISB at the small/large sizes. DMA's
upload-begin-to-Accepted median is about 11.51 ms, so it submits later in the
frame. A post-hoc upload-begin-to-kernel check gives medians of about 16.34 ms at the
small size and 16.33 ms at the large size; sealed ISB is about 16.14 ms
and 12.91 ms respectively. The large DMA interval is about 3.42 ms, or 26.5%,
longer than ISB. This is descriptive, not a newly predeclared metric, and still
excludes producer rendering/readback before upload begins. No full render-start
latency improvement is established.

Every run accepted and joined 2,120 generations with queried DRM monotonic
proof, clean protocol accounting, quiescent shutdown and empty native import
and frame-slot ownership. Each G run recorded 2,120 imports and evictions.
Frame-slot high-water was three and maximum in-flight per output was one.
Sampled Session RSS and logical upload-byte totals are retained; they do not
measure peer memory, GPU/VRAM peaks, capture allocation peaks or bus traffic.
Timing-disabled zero counters are not evidence that no work occurred. GPU
execution time, isolated source hold/capture queueing and input latency are
unmeasured. Native immutability controls, SDK/runtime tests, run-07 visual
confirmation and run-08 shutdown remain separate supporting evidence.

## Implementation and qualification history — October 22, 2026

niltempus requested completion of t339 with Claude and Muse working through
local herdr sockets. Preserve shell r10 owned-ring CPU clients and the lock
plane's ISB support. The first phase remains opaque LINEAR XRGB8888 on one
independently granted output/device, with explicit client opt-in.

Runtime and Session intake are committed in `73e667854`: configured/negotiated DMA grants,
physical destination reservation before ImageAdmitted, exact Source binding,
and submission to the existing completion owner. Session wire controls cover
custody, no publication before completion, invalidation, pre-use rollback and
status-before-source-release order. The stale-prepare regression proves a
second offer drains the timed-out first preparation and obtains its own bound.
Runtime default/feature suites and focused Session native-feature checks passed.

Sophia pin commit `c20fa0029` records the signed SDK pins: Rust `0706ea2c78cd16ba59ed209620354f30a2c78906` and C
`3ccc7d04c28309ebed6fd53f399af7c7a5682d80`, imported through Sophia's snapshot
checks. Both expose an explicit DMA producer/adapter lifecycle and preserve the
CPU ring default. Controls cover padded FD spans, no Source before admission,
CaptureComplete before native adoption, and discovery mask 6 with CPU grant 4.

Lom's shared GPU producer exports real modifier-0 XRGB DMA-BUFs on both crunch
AMD render nodes. RADV requires Vulkan DRM-format-modifier tiling with modifier
0; plain Vulkan LINEAR tiling cannot export the tested images. Each frame is
fenced and released to FOREIGN before its descriptor is exported. A stalled
export blocks new work and retains the device and resources through owner drop
until process exit. Validation layers are absent; barrier legality remains an
explicit residual risk, not a claim implied by successful pixel checks.

Muse's native EGL oracle consumed two Lom Vulkan frames on renderD128 and
verified their color patterns after exact capture completion. The first capture
remained unchanged after a later frame in a new allocation. This proves foreign
producer import and separate-frame independence, not source-storage reuse or
full Session presentation. Evidence lives under
`~/.local/state/sophia/development-evidence/t339-20261022/`: `FOREIGN-MANIFEST.txt`,
`foreign-oracle.log`, and `producer/MANIFEST-producer.txt` retain commands, hashes
and outcomes without binary copies.

Lom commit `acf1e32b1bc0c103a19972ffb3cfde39c1d4b074` supports protected panel
configuration `image-source "dma-buf"` as well as the CLI override; the default
stays owned-ring. It retains the GPU frame through CaptureComplete and emits
no CPU checksum for DMA content. The complete Lom repository check passed.
Claude's frozen `niltempus/tools/gpu-bar` peers share one GPU producer for I/S/G;
all 13 source files and both release binaries match the handed-over hashes.
Muse owns runner descriptors; Codex owns integration and qualification.

Run-01 stopped before graphics because the recovery chord was not armed.
Run-02 armed correctly but launch validation rejected a duplicate global GPU
grant alongside the component grant. Niltempus `333a3d1` removes the global
grant; the exact recipe-generated command now passes `session check-launch`.
Run-03 reached graphics startup but failed because public WM authentication
requires Factotum. Niltempus `1e6830d` supplies the agent and PAM helper
explicitly; the parser accepts the corrected command. The command
Run-04 then failed opening tty4: the prior seat-controller release reset the
device node to root:tty 0620 while the user's login remained active. Sophia
`94f8adcb3` makes the mode helper reuse stdin only when its tty name exactly
matches the requested target. Two real-PTY permission regressions pass; removing
the reuse makes the matching-target case fail. Server binaries are unchanged.
Run-05 reached Session and Hagia, but capture endpoint replies compared the
worker's held card number (226:0) with the grant's render-node number (226:128).
DMA negotiation was refused and no Lom frame appeared. The backend now records
the held card identity before requesting an endpoint, validates the reply
against it, and publishes the group's matched render-node identity. Generation,
epoch, output and store validation remain unchanged. All four endpoint lifecycle
tests pass, including the device-binding regression; backend clippy is clean.
A broader filter exposed an unchanged worker test expecting NotConnected where
an absent context now returns Unsupported; this fix does not change that path.
Run-05 restored display, keyboard and terminal modes. Run-06 used Sophia
`ea989aa04`: the endpoint opened and Lom negotiated r10 on renderD128, then
reported `image channel closed before Ready`. Lom enabled the SDK's owned-ring
adapter even when its renderer selected DMA; that adapter refuses Ready mask 6.
The client fix selects `enable_dma_image` for the DMA renderer. The timing G
peer had the same mistake; niltempus `f7f303e` selects the DMA adapter and allows
enough discovery space for its format row. Run-06 again restored the console.
Run-07 used Lom `5fb52e9` and Sophia binary `ea989aa04`. Niltempus saw the bar,
and three Lom candidates join exactly with Session presentation records and
kernel-timestamped native completions (one head, 2560x24 opaque panel, 60 Hz).
The pinned client issues Demand only after Accepted and CaptureComplete, so
this qualifies its DMA presentation route without claiming direct wire logs.
`session-qualification/run-07/postrun-presentation.json` preserves those joins
and the raw-log hashes. Protocol accounting and console restoration were clean.
The run still failed shutdown: `component content owners remain after native
shutdown`. Captured imports need an eviction turn after revocation and before
native disposal. Shutdown now drains acknowledged eviction within the existing
three-second reap deadline while the native owner is alive. The focused revoked
capture regression passes; all four Session feature combinations compile,
capture+native clippy is clean, and workspace-aware formatting passes. Run-08 verifies the fix on hardware: native exit 0, component owners quiescent,
zero live imports and frame leases, clean protocol tally, and restored TTY/keyd.
The remote PAM session also closed with no residual processes. Three candidate
joins match on DP-1 (2560x24, 60 Hz). This is host presentation evidence; DMA
routing is inferred from the pinned client, and no unattended visual claim is
made. Receipts are in `session-qualification/run-08/` and `ssh-run-08/`. The gate's host-log lookup
also needed correction: Session records are in `events.*.log`, not client stdout
(niltempus `3336b69`). Run-07 is presentation evidence, not an overall gate pass
or a performance result.
Failed attempts and recovery receipts remain under `session-qualification/`; latest parser acceptance is
under `launch-validation-04/` there. Run-03 recovery reports restored termios but unavailable KD/keyboard
mode readback, so complete console restoration is not claimed for that attempt.

The larger matched workload is a fully presented 1920x512 bar, alone on
HDMI-A-1 at 1920x1080@60, scale 1; the panel workload remains 1920x24. This uses
the admitted-target clause: existing coverage, panel extent and resource limits
exclude a 1920x1080 shell surface. `tools/gpu-bar/LARGE-SURFACE.txt` records the
limits without changing shell policy. I/S use comparator `4f5bdd564` with its
CPU texture cache enabled and profiling counters off; G uses `bfa477e93`
with capture enabled. That host-revision difference must remain explicit.
Niltempus commits `dcebdb2` (peers) and `1a2d616` (runner and CPU measurement
admission) are signed and pushed. The existing CPU sampler is unchanged;
its observer and analysis-plan builder now admit explicitly bound GPU I/S/G
at heights 24 and 512. The full xtask suite passed (455 tests), the CPU-plan
tests passed (36), and strict clippy passed. Source/wire/producer mismatches
are refused rather than relabeled as GPU runs.

Package `t339-gpu-arms-02` retained ten successful captures, each joining all
2,120 offered frames. Its second large sealed run failed observer finalization
at clean Session teardown; the last large IPC run did not start. All attempted
VT/PAM sessions recovered. HDMI-A-1 was observed as output 1, 1920x1080@60.
The CPU analyzer refuses all ten captures because completion records lack the
queried DRM clock capability. Those records are not amended, and no CPU benefit
verdict is claimed. `analysis-01/` records the refusal and capture inventory.

Niltempus `8aa0c19` fixes the observer race: a broken parent link counts as ended
only after rechecking that the recorded parent exited; a live-parent change
still fails. Focused binding tests (29), environment-monitor and diagnostic
checks, formatting and strict clippy pass. The between-read race itself is not
reproduced by the static fixture. Both Sophia hosts now query each opened card's
DRM clock capability even when no client requests a Present clock, and retain
that answer on native completion records. Unknown or unsupported capability is
reported as unproved. The focused capability-cache and diagnostic-field checks
pass; both release builds complete.

Six replacement plans and descriptors are frozen under
`~/.local/state/t331/t339-gpu-arms-03/`, with main `bfa477e93`, comparator
`25840757f`, integration `8aa0c19`, and unchanged GPU peers. `PLAN.md` preserves
the workloads, mirror order and thresholds. Each arm has a fresh PAM/VT4
session, 60-second runtime, 80-second Session watchdog, 180-second VT bound and
240-second supervisor bound. The remote launcher preserves the inactive herdr
login and checks CPU-plan construction and CPU-window analysis after each
capture, stopping on failure. All six descriptors and assembled launch vectors,
pins and wrapper syntax passed offline checks. Arms03 subsequently completed;
the result and measurement limits are recorded above.

Provlita has transparent padding, so the user's requested dock use needs an
explicit scope decision: validated alpha capture or retaining its owned-ring
path for this opaque-only phase. That question is pending; no dock appearance
changes are authorized. Kleis uses the separate lock protocol and is outside
this shell change. Run-07 confirms visible native Session DMA presentation; run-08 verifies clean
shutdown and console/PAM recovery remotely.

The matched comparison is complete, with the mixed result above. Export, import
and offline custody checks alone do not establish measured end-to-end benefit. Wider t343 qualification
is separate; t339's own comparison and correctness requirements remain in force.

## Scope and exit

Deliver one bounded GPU-native shell-image path under the existing
[image-channel contract](../../sophia-image-channel.md): producer-completed
DMA-BUF, same-device capture into Sophia-owned storage, immutable acceptance,
and source release only after proven safe completion. Keep current owned-ring
shell clients and ISB lock clients working. This is t339's source audit and comparison design, following
niltempus's October 20 instruction to proceed.

The first scope is opaque XRGB content on one admitted output/device. Reuse the
existing image service, renderer and presentation owners. Draw protocols,
direct composition from mutable producer buffers, direct scanout, changed
custody-dispatch defaults and changes to lock behavior are outside this slice.

Success requires correct custody and presentation plus a measured end-to-end
benefit over a matched GPU-render/readback baseline. Comparing with old IPC is
a separate product result, not proof of 9P efficiency or t331 acceptance.
No runnable experiment or native launcher is frozen by this plan.
Task state and execution order live in [todo.md](../../../todo.md).

## Source audit at 7a57dca1f

The audit reads production source without builds, device access or captures.
Paths below are relative to the repository; renderer context paths abbreviate
`crates/sophia-renderer-native-egl/src/gbm_platform/scanout/context`.

| Boundary | Existing implementation | Missing integration |
| --- | --- | --- |
| Wire metadata | Vendored `sophia-shell-protocol/src/shell_files/image.rs` defines DMA-BUF kind 2, planes, formats and coherence checks. | Codec support does not imply production admission. |
| Session policy | `crates/sophia-session/src/shell_component_connections/images.rs::configure_with` advertises kinds 1, empty formats and zero capture budgets. | Derive per-output/device formats from supported capture capabilities; reserve jobs and destination capacity before admission. |
| Channel/export | `crates/sophia-runtime/src/shell_transport/files/export/image_offers.rs` admits sealed sources, stores one `OwnedFd`, and reserves terminal journal credits. | Typed multi-plane custody and GPU-job handoff. Preserve metadata checks, terminal reservations and duplicate/late-source fencing. |
| Resource owner | Runtime `shell_transport/image_owner.rs::ShellIsbImageOwner` owns jobs beside the registry and survives connection replacement. | GPU job ownership, completion notification, stopped-interest/quarantine handling and settlement. GPU execution does not belong in the CPU validation loop. |
| Accepted resource | Runtime `shell_content/resources.rs::ContentResourceLease` owns a sealed mapping and exposes `bytes()`. | An owned GPU backing alternative with immutable identity, charges and retirement ownership. An unowned renderer ID is insufficient. |
| Rendering handoff | `crates/sophia-engine/src/compositor_graphics/content_identity.rs` assumes bytes; `crates/sophia-renderer-live/src/head_composition.rs` lowers content images to CPU layers. | Carry a GPU lease through existing identity/placement and lower to an already-captured owned image. Handle unsupported CPU consumers explicitly, without hidden readback. |
| Native capture | Renderer `renderer_images.rs::capture_renderer_image` copies imported content into owned storage; snapshot reuse tracks allocation and GPU uses. | A standalone shell-facing job/completion interface. Mixed-frame export captures while assembling a frame; shell release must not depend on a candidate or flip. |
| Completion | Renderer `image_capture.rs::capture_completion` and snapshot fences exist for specific paths. | Prove exact source reads ended on success, error and cancellation. Capture return, destination insertion, flush or FD closure alone is insufficient. |
| SDK | Generic codec/producer machinery exists, but the shell adapter neither requests DMA-BUF capability nor accepts it in `enable_image`. The generic producer can map the capability bit. | Extend the upstream shell request/opt-in layer and release ownership, then pin it. Do not implement by editing generated vendor snapshots. |

The X capture path supplies reusable implementation evidence, not shell
admission. `crates/sophia-renderer-live/src/native_scanout/owned_mixed_export.rs`
has different publication/release callers, and capture fallback can cross
devices. Shell first-phase same-device scope must be explicit.

Owned GPU storage is not a missing renderer primitive:
`crates/sophia-renderer-live/src/native_scanout/renderer_images.rs` already has
`LiveRetainedRendererImageSnapshot`, backed by an Arc, snapshot budget and
epoch; `LiveOwnedMixedCompositionLayer::Snapshot` consumes it. Reuse that
ownership where applicable rather than building a second snapshot registry.
The gap is connecting a completed capture to the shell resource/grant lease
and existing compositor-content path. Snapshot liveness and promotion must
not be mistaken for proof of source-read completion.

The [t289 plan](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md)
and current code contain snapshot reuse work; the
[renderer import summary](../../renderer-import-boundary.md#capture-execution-and-immutable-storage)
still describes fresh storage. Establish the selected runtime mode from code
and pin it in both arms. Do not silently change reuse policy between arms or
assume all remaining t289 work is a prerequisite.

## Contract constraints

First phase requires provider-completed rendering, no fence FD in the Source
packet, same-device capture and exact advertised formats/modifiers. Producer
completion cost belongs inside the measured workload. Explicit acquire-fence
transport would require a later protocol change.

Capture isolates future consumption from producer mutation but cannot prevent
a hostile producer racing the copy. Metadata and bounds remain independently
validated. XRGB capture must force alpha to one. Alpha-bearing images require
full premultiplication validation of the captured owned image, not a sampled
pixel or source-side scan. Transparent clients remain on ISB until that exists.

The comparator's current `Canvas::fill_pixels` produces opaque grayscale with
alpha 255. An opaque fixture can preserve that visible content, but GPU
generation is a new producer workload. Its results cannot replace C23's
CPU-fill measurements or establish performance for translucent bars and docks.

Existing renderer entry limits and pitch-based accounting do not alone prove
the shell contract's pre-admission destination reservation. Specify a supported
allocation bound, including overlap and quarantined work; refuse unknown
requirements. Logical pixel bytes are not a hard bound on opaque driver VRAM.
The accepted execution contract distinguishes known storage accounting from
portable hard GPU-resource isolation.

SourceReleased follows GPU completion or proven pre-use rollback, independently
of presentation. Resource retirement waits for retained scene and renderer
consumers. Timeout, channel closure, revocation and cancellation may stop
publication but cannot free in-use backing or return its capacity.

Older t304/t339 language about mandatory byte fallback and source release
“after the capturing flip” is superseded by the ISB amendment and the current
image-channel contract. Keep historical evidence, but implement today's rules.

## Implementation sequence

1. **Typed accepted backing and capture ownership.** Bind the existing owned
   renderer snapshot to a shell lease beside sealed backing, tied to resource,
   grant, connection epoch, renderer
   owner/device generation and content generation. Keep host handles in
   adapters. Define CPU/GPU access explicitly and preserve Engine placement and
   input semantics. Start with deterministic supplied-completion controls.
2. **Standalone capture executor.** Reuse the existing renderer owner through
   bounded jobs and its wait/wake mechanisms. Retain descriptors and reservations
   through uncertain GPU work; quarantine without busy waiting. A capture must
   complete or safely fail without frame submission. Avoid a second reactor.
3. **Role admission and settlement.** Extend source-kind handling, per-output
   formats, offer limits and epoch withdrawal. Publish Accepted after complete
   valid capture, then SourceReleased in journal order. Preserve fresh admission
   and authorization boundaries; journal pressure must retain custody.
4. **GPU content handoff.** Thread the lease through candidate/resource
   validation, scene retention and native lowering. Handle output replacement,
   renderer restart and target failure explicitly. Repaint must not re-capture
   mutable source storage or perform an implicit CPU readback.
5. **SDK and GPU fixture.** Use one independently granted render device and one
   producer implementation across arms. Extend SDK source-release and disconnect
   handling. Update both language contracts and conformance coverage before
   public promotion; retain ISB clients.
6. **Native qualification.** Freeze executable artifacts and launch plans only
   after lifecycle tests and device-specific pixel/custody checks pass.

Minimum deterministic controls cover wrong connection/epoch/device, malformed
planes/offsets/strides, unadvertised formats, exhausted resource/job/journal
capacity, cancellation before Source and after submit, completion after timeout
or revocation, duplicate/late Source, no early or duplicate release, disconnected
journal, renderer-generation replacement, consumer-held image after source
release and no use after retirement. Unsignalled completion must retain charges
while control/input progress continues.

Device controls must prove exact dimensions, generation/placement, alpha one,
and immutable captured pixels after the producer rewrites its source following
SourceReleased. A successful import alone does not prove these properties.

## Matched comparison

Use matched arms with the **same GPU producer**, pixel pattern, target, dimensions,
cadence, update count and presentation semantics:

| Arm | Path | Purpose |
| --- | --- | --- |
| I | GPU render, completion, readback, old comparator IPC bytes | Product baseline; not restoration of a production byte protocol. |
| S | Same GPU render, completion, readback, sealed ISB over files | Historical files baseline; requires the comparator host. |
| R | Same GPU render, completion, readback, owned-ring ISB over files | Current default; compare on the same host and peer executable as G. |
| G | Same GPU render and completion, DMA-BUF over files, owned GPU capture | Candidate. G/R tests the current data-path choice; G/I tests the historical product goal. |

Use an opaque 1920x24 panel and a separately reported larger fully presented
surface, initially 1920x1080 if supported by the admitted target. Every
generation changes content. Do not substitute a transfer-only bulk probe for
the large presented surface. A simple deterministic GPU pattern establishes
mechanism first; representative textured/drawing work is needed before broader
claims. All arms pay identical producer rendering and completion work.

Pin device/driver, format, output mode, scale, VRR, renderer reuse policy,
source-pool size, outstanding-offer bound and warmup. Keep allocations required
by each arm's real ownership contract and report them; do not hide setup or add
avoidable baseline allocations. Pixel qualification may read back separately;
the timed G arm must not read back a full frame merely to collect evidence.

Freeze signed source and artifact provenance. Retain all runs. Use balanced
G/S and G/I blocks with both orders represented; choose repeat count and
decision thresholds before observing treatment results, using measured
no-change variation. Differences below that variation are unresolved, not
equivalence. Refusals and outliers remain in the record.

Report Session and peer CPU/frame and workload bounds, latency distribution,
completion cadence, joined generations, allocations/retained-resource peaks,
faults, bytes read back/uploaded/captured, source hold time, capture queueing,
and GPU completion timing where directly measurable. CPU time is not GPU work;
submission elapsed time is not GPU execution time. Label missing GPU timing
instead of inferring it. Keep heavy attribution instrumentation separate.

Promotion requires correct joins, cleanup and custody, repeatable benefit and
no unacceptable latency/input or memory regression. A capture microbenchmark
or lower syscall count is supporting evidence only. The existing native
`capture_cost` test uses a CPU-filled GBM source and manual eviction; it is not
this GPU-producing shell experiment.

## Current next deliverable

The negotiated DMA intake, SDK adapters, destination reservation before
admission, and native owned capture path are implemented behind
`experimental-shell-capture`. Run-07 presented Lom's opaque panel through that
path, with three exact client/Session/kernel-completion joins. Its shutdown
failure is fixed in `73aaedd06`: Session obtains capture-import eviction
acknowledgment while the native renderer still exists. Run-08 verifies clean
shutdown, zero retained imports, and console/PAM restoration with that fix.

Arms03 completes the historical I/S/G comparison at 1920x24 and 1920x512. Correctness,
custody and cleanup pass; large-surface CPU benefit is resolved. Small-surface
CPU regression, the upload-to-presentation tradeoff and missing physical-memory
peaks prevent an unqualified promotion claim. Ring-01 now completes the requested
same-host, same-peer R/G comparison at those sizes: all eight captures pass,
large-surface CPU improves, small-surface CPU regresses, and neither height has
a resolved upload-to-kernel latency difference. Further attribution work is
recorded in Current resumption; no new qualification comparison is scheduled.
The existing promotion scope remains open; closing only the implemented path
would require an explicit scope decision. No exit criterion is silently waived.
Owned-ring remains the default; dock alpha and the separate lock protocol are
outside this opaque shell slice.

### Completion handoff and accounting requirements

#### First implementation prerequisite: reserve before capture

The October 20 reservation slice adds `experimental-shell-capture` in
renderer-native-egl and renderer-live, disabled by default. A non-cloneable
snapshot reservation holds one entry and a caller-proven allocation byte bound
before submission. Retention transfers that same charge into the existing
snapshot and reduces it to actual distinct backing-allocation bytes. Refusal
returns the original snapshot descriptors and the entire reservation, including
wrong-budget, invalid-generation, oversized-allocation and descriptor failures.
Generation invalidation does not release storage still held by a snapshot.

This is accounting infrastructure, not a completed first ownership/completion
slice. It does not implement the capture executor, Session mailbox, shell
resource binding, or source-completion receipts. Session still advertises zero
GPU capture jobs. Reserving capacity does not constrain the allocator by itself:
the executor must establish the destination allocation bound before GPU work,
and retain source descriptors and reservations through uncertain work.
`retain_reserved` neither proves source-read completion nor permits
`SourceReleased`.

Controls use ordinary memfds solely to exercise descriptor custody and allocation
accounting; they perform no DMA-BUF import or GPU work. Both library suites pass:
39 tests with the experimental feature and 35 with the existing GBM path and the
experiment disabled. Four new controls cover reservation limits, transfer and
shared-plane accounting, refusal ownership, and retry after a privately injected
charge-uniqueness failure. Muse reviewed the implementation read-only; the
suggested panic-to-refusal correction is included. Live wrapper refusal mapping
still needs an executor-level control once real snapshots can be supplied through
that path.

Evidence is retained at
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/reservation-01/`.
No native capture, admission change, or performance result accompanies this slice.

#### Private completion handoff

The next October 20 slice implements `shell_capture_jobs` privately in
renderer-live under the same disabled experimental feature. It has no public
API and no real completion supplier yet. Generic payloads let the lifecycle
controls own real descriptors and snapshot reservations without constructing
fake public GPU snapshots. Context-bound work stays on the renderer owner:
the executor is not Send, spawns no thread, and exposes a request wake; the
inbox can cross owners and exposes the completion wake.

Four shared permits bound queued/active jobs and untaken terminal receipts.
Quarantined jobs retain their permits and block new admission, including through
replacement mailboxes sharing the same graveyard. A failed enqueue returns the
whole request unchanged. A submission gate serializes enqueue with shutdown,
so an accepted job cannot fall between queue draining and receiver destruction.
Shutdown closing admission and finishing settlement are distinct published
states; the consumer observes settlement before draining to avoid reporting a
receipt that is concurrently arriving as missing.

Globally unique job IDs and exact epoch Arc identity bind private completion
proofs. `begin` marks submission before exposing the payload for import or any
GPU use. Unknown/duplicate begin is distinguished from a completed rollback.
Cancellation/deadline before submission allows rollback. After submission,
including after renderer completion but before receipt consumption, it forbids
publication and still requires exact completion before source release.
The private native driver now constructs a job completion only after consuming
an opaque completion from the exact native pending handle. Supplied completions
remain confined to private test children.

Explicit close and unwind settle selected/queued work as rollback and submitted
work as uncertain. The generic handoff alone requires a retained graveyard and
does not prove teardown ordering. In the real adapter, the entire source request
and reservation move into the native escrow before import/copy is attempted;
the generic submitted payload then contains neither. Native uncertainty retains
that escrow for process lifetime, as described below. EGL context/display
destruction alone is never a source-read completion event.

Ten supplied-completion controls cover these transitions with sockets standing
in for source planes; they exercise no DMA-BUF import, rendering or EGL fence.
Four mutations are detected: bypassing epoch identity, dropping quarantined
payloads, omitting receipt-time cancellation, and treating closed admission as
finished settlement. Restored library suites pass 49 tests with the feature and
35 with existing GBM behavior, with Clippy and formatting clean. Muse reviewed
the lifecycle and its races read-only; the native adapter/teardown obligation
remains explicit. Evidence is in sibling directory `handoff-01/` under the
reservation evidence root above.

#### Private native copy-fence observation

The `experimental-shell-capture` native module now owns a private, zero-wait
observer for the fence inserted and flushed by `NativeCaptureAllocation::draw`.
Its real entry takes the execution/display, allocation, generation and fence
from the same `NativePooledCapture`; callers cannot inject a successful status.
Only `CONDITION_SATISFIED` issues a non-cloneable, renderer-thread-bound proof.
The proof retains allocation and generation identity and matches the exact sync
handle. Each capture issues at most one proof. Timeout leaves it pending;
missing or mismatched ownership, errors and unexpected statuses refuse proof
and abandon reuse. Observation never takes the fence or its GPU-use guard.

This observer alone neither releases source FDs nor settles jobs, grants
publication or enables admission. The adapter below is now its real consumer. Its private fault
seam exercises supplied statuses with real native allocation accounting and
use guards, without EGL execution or device access. Seven controls cover
pending/once-only completion, errors, missing fences, generation ownership,
proof identity, retained allocation lifetime and already-abandoned reuse. Five
mutations are caught by test failures: treating timeout as completion, ignoring
use-generation identity, allowing repeat observation, and ignoring proof
generation or sync identity. The restored controls pass. Library checks pass
56 tests with the experiment and 35 on the existing GBM path without it; Clippy
is clean. Evidence is retained under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/native-proof-01/`.
No real GPU fence or native capture was tested in this slice.

The [EGL fence-sync contract](https://registry.khronos.org/EGL/extensions/KHR/EGL_KHR_fence_sync.txt)
binds the signal to preceding commands. The
[EGL 1.5 specification](https://registry.khronos.org/EGL/specs/eglspec.1.5.pdf),
sections 3.2 and 3.7, distinguishes resource termination/destruction and flushing
from completion. Consequently, destroying a sync or terminating an EGL display
must not manufacture this proof. The adapter binds that proof to the retained native request and shell job;
uncertain custody uses explicit process-lifetime retention rather than teardown
as a substitute proof.

#### Native source custody and mailbox driver

The default-off native API takes an owned, single-plane XRGB source plus a
pre-reserved snapshot budget, image ID and epoch. It uses the existing renderer
owner and capture pool, with no transfer/readback fallback or frame-assembly
coupling. Metadata/epoch/budget mismatch or lack of a process slot returns the
original request before any source use. The real allocation size, including
padding, is checked against the reserved bound before the draw; a successful
completion transfers that reservation into the retained snapshot.

A native pending handle owns the request, destination capture and exact context
identity. It is non-cloneable and stays on the renderer thread. Each poll performs
one zero-timeout native fence observation. A signalled exact-copy proof permits
source custody to return even if subsequent snapshot export or retention fails;
that failure returns the unused reservation. A retired or different context is
refused before any EGL call. A withdrawn epoch still requires copy completion;
it can settle custody but cannot publish a retained image.

**Uncertain-copy policy for this experiment:** four process-wide slots bound
submitted copies. Abandoning a pending handle, unwinding after possible source
use, receiving an uncertain fence result, or failing after entering native
capture keeps the source FDs, reservation and any owned pending capture for the
process lifetime. The slot stays charged and all further experimental native
captures are refused, including through replacement channels/contexts. There is
no reset, timer, destructor completion or automatic release credit. This is
intentional bounded retention, not a claim that EGL teardown finishes reads.
Errors after entering capture are conservatively uncertain even when they may
have preceded a draw. This can stop the entire experimental GPU-capture lane;
qualification must assess that availability tradeoff before production admission.

The private renderer-live driver connects these real pending handles to the
existing bounded mailbox. It checks exact snapshot-epoch identity before begin,
restores the original request on native pre-use refusal, and constructs a job
completion only from its own handle's opaque native receipt. Cancellation,
deadlines and generation withdrawal still forbid publication, including at
receipt consumption. Pending fences rotate to the tail with at most four native
begin/poll operations per service call and no repeated poll of one fence within
that call. A service unwind guard settles every remaining job even if its caller
catches the panic and retains the driver. Close parks native handles before
publishing uncertain receipts.

Controls use owned socket FDs, real reservation accounting, supplied native
statuses and a private scripted backend. They cover process capacity, retained
custody on abandon/unwind, refusal return, linear-source validation, padded
allocation bounds, context/epoch identity, pending completion, cancellation,
withdrawal, tail rotation, exact job/source matching and caught unwind. They do
not execute EGL import/copy, prove rendered pixels or measure performance. The
feature library suites pass 75 tests; the default GBM suites pass 35. Nine mutations fail at runtime: premature custody drop, reopened admission,
returned quarantine credit, bypassed allocation bound, wrong context or epoch
identity, head requeue, omitted unwind settlement, and timeout-as-completion.
Restored suites pass; Clippy is clean. Logs are retained under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/native-adapter-01/`.

#### Session completion handoff and connection binding

The next October 20 slice exposes an experimental renderer-live mailbox façade
and adds a private Session correlation owner. The façade accepts a reserved
capture request, returns an opaque job ID, and produces a non-constructible
completion receipt. Its executor services the existing renderer-live context
through a private bridge; it creates no context, thread or device. The inbox is
Send; the executor remains !Send. Requests wake the renderer and completions wake
Session. Pending fences still require explicit follow-up owner turns: the request
fd does not become readable when a GPU fence signals.

Completed receipts return the exact source descriptor and either the already
charged immutable snapshot or an output failure with its unused reservation.
Withdrawal/deadline flags survive receipt conversion. Pre-use rollback returns
the original request. Uncertain receipts return neither source nor reservation;
the native escrow's existing quarantine policy continues to own them. The live
snapshot wrapper adds no second allocation or budget charge. Public enum values
are not proofs: only the opaque receipt is issued by the mailbox/native adapter.

Session's bounded table retains the slot, connection/content grant epochs, full
image metadata and the single plane descriptor until the exact job settles.
DMA-BUF metadata stride is zero; offset/stride/fd index come from its plane row.
Mismatched metadata, duplicate offers/resources, full tables and closed queues
return source and destination reservation without creating a binding. Withdrawal
and resource cancellation keep old bindings alive; a reconnect with reused offer
or resource numbers cannot take their receipts. An unmatched receipt is retained
and closes further submission. A settlement is not permission to publish: the
resource owner must still revalidate live authority and reconcile its logical
shell reservation, even for stopped or uncertain jobs.

This is an opt-in integration prerequisite, not a running GPU shell path.
The Session owner is not installed in production connection state or its service
loop yet. Runtime's ContentResourceLease still exposes CPU bytes; logical shell
resource reservation/acceptance, compositor lowering and actual owner-turn fence
scheduling remain outstanding. Admission still advertises sealed-only, and
`experimental-shell-capture` is in neither Session's default nor native-session
features. No native GPU execution, snapshot-pixel validation or performance claim
is attached to this slice.

Controls use supplied private completions and real mailbox shutdown before GPU
use. They pin FD/reservation return, reconnect and generation isolation, metadata
matching, cancellation, capacity, exact-once receipt handling and fail-closed
identity faults. Six mutations remove these guards individually and fail at
runtime. The restored feature suites pass 91 tests; the default GBM suites pass
58 (one pre-existing ignored Session test in each configuration). All six new
Session controls also pass with native-session enabled. Clippy is clean. Two
compile-fail controls prohibit external receipt construction and moving
the executor between threads. Logs and the retained initial metadata-stride test
failure are under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/session-handoff-01/`.

#### Logical capture reservation and failed-copy settlement

The following October 20 slice connects the Session binding to the actual
runtime allocation/resource owner. `reserve_capture_image` checks the live grant,
output/allocation and topology, the opaque single-plane XRGB metadata, and the
existing content limits before issuing an opaque, non-cloneable reservation.
The ticket binds the full admitted metadata and the exact transfer identity;
identical grant/resource numbers in a different store cannot settle it.

Capture and CPU transfers share resource generation history, quotas and two
terminal-record credits. A capture transfer has a separate identity marker, so
`complete_isb` refuses both successful and failed CPU results without consuming
them or releasing capture credit. Cancellation, timeout, revocation and even
loss of the ticket retain the transfer and its charges. Logical staging,
reserved-resident and backing bytes are quota accounting; they do not describe
a CPU staging allocation or duplicate the renderer's physical allocation.

Session submission now requires the runtime ticket and checks its grant and
full metadata against the binding. A refused enqueue returns that ticket along
with the original source and native destination reservation. Pending jobs keep
their ticket through resource cancellation and disconnect. Resource observation
cancels publication interest while continuing to reconcile the native job.

For failed/unpublished work, Session consults the opaque native receipt before
settling runtime credit. Proven pre-use return or completed reads can produce a
resource failure status; uncertain work cannot enter reconciliation, including
after timeout. A usable successful snapshot keeps its ticket for the future
accepted backing path. Wrong-owner and clock errors preserve both ticket and
receipt for retry. Disconnected jobs settle against their retained old epoch,
without reopening authority or touching a successor. The first cancellation,
expiry or revocation reason remains authoritative.

Runtime's failure-accounting API is a trusted-owner operation, not a GPU
completion proof. It emits only ResourceStatus. Neither it nor the Session
wrapper emits SourceReleased, ResourceReleased, ImageAdmitted or acceptance;
those require their respective journal/owner transitions. The snapshot receipt
remains owned even after logical failure credit is returned. No renderer type
or dependency has been added to runtime.

Controls cover cross-class completion, exact owner identity, shared budgets and
generations, first terminal reason, clock recovery, retained uncertain credit,
authoritative allocation checks, retired-epoch settlement and Session receipt
binding. Eight mutations fail at runtime. The uncertainty-to-accounting control
uses a private supplied-disposition seam with the real resource registry; it is
not a GPU execution or public receipt-construction path. Selected restored
feature suites pass 270 tests and their default counterparts pass 222, with two
pre-existing ignored library tests in each selection. All ten Session handoff
controls pass with native-session enabled; three compile-fail controls pass.
Clippy and formatting are clean. Evidence is retained in
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/resource-reservations-01/`.

GPU-backed accepted leases, successful-copy resource publication, compositor
lowering, owner-loop installation/fence follow-up and native qualification remain
outstanding. The experimental feature remains off by default; production image
configuration remains sealed-only. No hardware run or performance claim is
attached to these accounting controls.

#### Shared snapshot ownership prerequisite

The accepted GPU lease needs a storage owner below both runtime and the native
renderer. `sophia-renderer-image` now owns the existing image identity, descriptor
planes and borrowed views, snapshot, budget/charge, epoch and experimental
reservation types. It depends only on rustix/std, with no renderer context, GBM,
EGL or runtime dependency. Native EGL re-exports those types and maps the three
storage errors into its existing refusal vocabulary; renderer-live keeps its
existing retained wrapper. The single-plane GBM admission adapter stays native.
Descriptor duplication and budget retention still use the same implementations
and charge, rather than another image registry or an erased backing trait.

An openly constructible `OwnedSnapshotData` creates only an **uncharged**
snapshot. Neither owning descriptors, charging them nor observing a current
epoch proves immutable capture or ended source reads. The native exact-copy
completion and opaque receipt remain the proof. `is_retained()` is an ownership
fact only; a future registry accept operation must not treat a caller-supplied
retained snapshot as a completion receipt.

Review identified an additional release requirement before successful GPU
acceptance. Shell resource leases have an outer Arc, but native imports and
queued scanout buffers retain the inner snapshot Arc. `try_clone()` also creates
a different snapshot with duplicated descriptors and the same physical charge.
The leaf's `custody_is_exclusive()` therefore checks both the snapshot and charge
strong counts. Future resource collection must require outer-lease uniqueness
**and** this inner-storage condition. Epoch invalidation never releases storage;
import-cache retirement is per charge and does not invalidate sibling resources.

Uniqueness alone does not make retirement progress: an idle native import cache
can retain a snapshot indefinitely. Session must mark imports non-cacheable and
request explicit native import eviction from **retirement/revocation intent**,
before waiting for exclusivity. `isb_release_pending` is too late as the trigger
if it already depends on collection. Frames and submitted GPU uses must still
drain through their existing completion owners. Do not claim a fixed release
latency from reference counts or from an eviction request.

The existing descriptor and budget tests moved with their implementation. New
controls distinguish uncharged data, same-snapshot consumers, duplicated
snapshot consumers, cache retirement and epoch invalidation. Their ordinary
memfd/socket descriptors test ownership, not GPU rendering. Evidence lives in
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/snapshot-storage-01/`.
Muse independently reviewed the design and extraction read-only; test results
are the parent's, not the reviewer's.

Selected validation passed: 88 default-mode tests, 156 feature-mode tests
(including the native-session handoff controls), three compile-fail docs, and
Clippy. Five deliberate mutations failed their runtime controls: ignoring either
ownership count, losing the shared charge on descriptor duplication, omitting
cache retirement, and treating uncharged data as retained. Restored controls
passed. The ordinary native Session configuration also type-checked with the
experimental feature; this was compilation, not a native launch.

At that prerequisite, runtime GPU backing, successful-copy acceptance and native
lowering were still absent. The following slice adds those storage paths; the
live owner-loop installation remains outstanding. Production GPU admission
remains off, and no device run or performance claim accompanies either slice.

#### Accepted capture leases and native lowering

The next October 20 slice gives the runtime resource store a concrete captured
backing under `experimental-shell-capture`. The resource lease retains the exact
snapshot Arc from the native receipt, including its existing physical charge.
There is no backing trait, replacement allocation, descriptor duplication or
pixel readback at acceptance. Runtime logical resource bytes and renderer
allocation bytes remain separate limits on the same storage.

The private Session entry accepts only a completed, unstopped opaque receipt.
It binds the requested renderer image ID, exact reservation, connection/grant,
resource metadata, and the full allocation snapshot recorded at reservation.
The registry rechecks the current allocation and topology: reusing an allocation
number with identical dimensions but a new generation cannot accept an old copy.
Refusals keep both reservation and snapshot. The lower public registry operation
is trusted accounting, not a verifier of GPU execution; retained storage alone
is never a completion proof.

Successful accounting emits the resource status once. It does **not** emit
`SourceReleased`. The completion owner must still retain the native receipt and
source-release obligation until that separate journal transition succeeds.
That live journal owner is not installed in this slice, and post-acceptance
receipt retention is not yet enforced by a dedicated pending-release type.

CPU consumers use `cpu_bytes()` or narrow to a `ContentCpuResourceLease` before
an infallible byte view exists. CPU rendering explicitly refuses GPU content.
Native head lowering carries the same snapshot Arc into the existing snapshot
layer; identity uses declared logical bytes without reading pixels. Native
imports, queued snapshots and duplicated descriptor owners continue holding
the existing physical charge after the runtime lease itself is dropped.

Retirement intent now exposes a bounded eviction operation across active and
disconnected epochs. It marks storage non-cacheable and requests the existing
native owner's import eviction. A failed callback retains the obligation and
both budgets. Collection requires eviction acknowledgment, the last outer
resource consumer, and the last inner snapshot/charge consumer. For a live
epoch, backing and credit then remain held until `ResourceReleased` enters the
journal and the owner confirms it. Disconnected epochs need no live journal
confirmation, but revocation does not bypass physical ownership. The Session
wrapper is present, but the live owner loop must still
service it even without a future composition frame and revisit after consumers
retire; otherwise custody is safely retained but progress can stall.

Supplied memfd storage controls exercise accounting, exact binding, CPU refusal,
native lowering and retirement. They do not fabricate native success receipts,
prove a GPU copy, or qualify DMA-BUF admission. Evidence is retained under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/accepted-leases-01/`.
Selected suites passed 666 test executions across configurations (including
three compile-fail docs; not a count of unique tests). Both Clippy configurations
and changed-file formatting passed. Eight deliberate mutations produced runtime
test failures, and restored controls passed. Muse's final read-only source review
found no accepted-lease custody blocker; builds and mutation results are the
parent's evidence, not independently run by Muse.
The standalone renderer-live default build has a pre-existing optional-native
dependency error in `renderer_image.rs` (unchanged since `7faa3a735`); CPU
compatibility checks use `gbm-probe` with the experimental feature off. That
substitute configuration must not be described as a standalone-default pass.
The eight native-session reconnect fixtures also fail before resource work:
their setup is refused because namespace recipes require normal Session with
Factotum launch authentication. The same eight setup failures were reproduced
at committed baseline `01dc5e19f` with the same features; current sources were
restored byte-for-byte. This slice does not relax that authentication requirement.

#### Bounded owner retention and retirement hooks

The October 20 owner-retirement slice retains each opaque completion receipt and
source in the same four-slot owner until resource accounting succeeds and the
exact connection's `SourceReleased` is journaled. Journal saturation retries;
other publication errors also retain custody. Only proof of the exact journal's
disconnection permits disposal without that append, after resource settlement.
The runtime export retains its independent status hold, so ResourceStatus must
enter the journal before SourceReleased. A completed copy that cannot be
accepted settles the original reservation without publishing the copied image;
uncertain completion never releases its source.

Pending jobs register their completion fd through the existing component wait
subscription. Completion retention and retirement cap the component maintenance
wait at 10 ms. This is a retry deadline, not a liveness or authorization cache.
Completion servicing still follows the existing component/runtime service gate;
there is no new owner loop. Installing the executor must explicitly preserve
progress and teardown when that gate stops servicing components.

Retirement runs on existing native service turns without requiring a future
frame or an open topology frame-service gate. A resource eviction visits every
current render store, including disabled heads, deduplicating shared workers.
Any busy/error store leaves the obligation pending. The Session operation
requires the native owner by reference: absence cannot acknowledge eviction.
The four-resource per-turn budget bounds traversal. An eviction acknowledgment
does not authorize ResourceReleased: exact snapshot/charge exclusivity remains
required, with maintenance continuing while native consumers hold storage.

Source review corrected the proposed integration seam: the context does not
expose `snapshot_budget()` / `snapshot_epoch()` accessors. Backend preview state
owns its persistent budget and store-generation epochs. A native context can
also reside on the existing renderer worker, whose loop waits for commands.
Session may not borrow that context or move its non-Send executor across owners.
The next slice must use the backend's budget/generation ownership and extend the
existing worker protocol; a Session-created budget or second EGL context is not
an acceptable shortcut.

Controls cover real Returned receipts under repeated journal pressure, bounded
slots across both lifecycle phases, publication-error retention, clock-refusal
retry, completion-fd subscription, shutdown refusal with pending custody, and
maintenance through the last sampled-storage owner. Pure backend controls pin
all-store traversal, shared-worker deduplication and error retry. The real image
journal fixture pins status-before-source ordering and rejects using capture
accounting to release a sealed offer. These are composition controls, not a
real GPU Completed-receipt integration verdict. Evidence is retained under
`t339-owned-capture-20261020/owner-retirement-01`.

Muse reviewed that slice read-only. The missing-native-owner acknowledgment was
removed following that review. At that point no executor was installed and
policy advertised zero capture jobs; native worker installation,
close-before-context-drop, graveyard retention and actual source-completion
controls remained open. The following slice addresses worker installation.

#### Existing renderer-worker installation

The subsequent October 20 slice supplies an experimental backend endpoint for
one explicit output on an existing renderer worker. It does not create a thread,
context or device. Inline exporters return Unsupported. The worker constructs
the non-Send executor on its own thread and closes it before context replacement
or drop. Capture service failures close capture work while keeping the renderer
available for its other commands.

The endpoint uses the backend's existing snapshot-budget Arc, with a separate
capture epoch so closing capture does not invalidate preview snapshots. The
driver rejects a foreign budget or epoch before native use. Persistent custody
survives mailbox replacement; uncertain jobs cannot regain capacity by opening
a fresh mailbox. Neither context teardown nor a close-command acknowledgment
proves that GPU reads ended.

With the feature enabled, the existing command channel uses SignalSender and a
WakeSlot. An active executor waits on command and request readiness together.
The wake is cleared before queue inspection. Fence-only work requests a 1 ms
follow-up; spending the turn budget requests an immediate next turn. Idle
mailboxes block without periodic polling. Service remains bounded to four
capture operations per turn. Feature-off command and wait behavior stays as it
was.

Generation, output and store changes invalidate capture epochs and retain close
intent until the command can be sent. Queue pressure retries on existing native
service turns, independently of component and frame gates; pending installation
or close caps the owner wait at 10 ms. Close commands are deduplicated while an
installation reply is outstanding. Endpoint publication rechecks the enabled
output, store, generation and epoch. Source receipts still belong to Session's
settlement owner; backend close does not publish SourceReleased.

Device-free tests cover pre-use source return, budget/owner identity, quarantine
retention across replacement, request and command wakes, fence-only progress,
capture-local error isolation and generation/output/store withdrawal. Evidence
and mutation controls are retained under
`t339-owned-capture-20261020/worker-installation-01`. These controls do not prove
real GPU completion, end-to-end Session installation or performance. Production
admission remains sealed-only with zero capture jobs.

#### Session endpoint coordination

The next October 20 slice connects the existing native maintenance turn to the
connections-level capture owner. There is one inbox consumer and one global
four-slot bound; jobs retain their exact connection/grant bindings. A private
link requests an endpoint only after a current reserved allocation supplies its
output. It refuses another output while that endpoint is pending or active.
The installation reply supplies the backend's budget and epoch; submission
requires those exact Arcs and revalidates the logical ticket and allocation.
Failure returns both source request and reservation unchanged to their owner.
The query and endpoint are not permission or source-read completion proofs.

Pending demand is tied to the original connection key. Disconnect or slot reuse
before the reply closes the pending endpoint; another waiting connection must
prepare again from its own live reservation. Once installed, the mailbox is
shared and survives individual connection replacement. Reservation expiry alone
may leave an idle endpoint installed, but cannot authorize submission. No
production ingress invokes preparation/submission with admission disabled.

Native lifecycle runs before endpoint polling; endpoint service precedes receipt
drain and eviction maintenance on the native turn, outside runtime/frame gates.
Shutdown or an invalid backend epoch withdraws submission/publication. Transient
presentation unavailability alone does not close capture. Close-queue pressure
keeps the exact intent for a later maintenance turn. Receipt reconciliation still
runs when installation fails. Existing ordinary component turns also reconcile
receipts; settlement is idempotent and preserves journal pressure.

An owner is replaceable only when closed and completely drained: pending jobs,
settlements and orphaned receipts all count. Uncertain reads retain source and
reservation, blocking replacement. The old direct installer is test-only, so
production cannot replace the inbox while leaving link destination facts stale.
Source publication routes only to the exact Connected slot/grant; a replacement
grant has a different journal, and publication errors retain custody.

Controls use real mailboxes with supplied endpoints and pre-use rollback. They
cover idle demand, request/close pressure, output mismatch, stale replies,
disconnected requesters, foreign budgets/epochs/tickets, owner replacement and
exact publication routing. The existing retained-receipt controls now also pin
replacement refusal during journal pressure and orphan retention. Evidence is
`t339-owned-capture-20261020/session-endpoint-01`. These controls do not constitute
a live GPU completion, DMA-BUF admission or performance result. Outer shutdown
still refuses disposal of unresolved capture custody; it does not fabricate
completion from backend teardown.

#### Pre-submit source retention (2026-10-20)

Session now privately retains an unsubmitted descriptor with its exact binding,
logical reservation and validation deadline while the existing native endpoint
is prepared. Waiting sources, submitted jobs, retained receipts and orphaned
completions share the four-slot bound. Capacity and identity refusals return the
original source and ticket. Pending sources participate in maintenance waits
and prevent final owner disposal.

The explicit handoff requires a renderer allocation byte bound and reserves
against the endpoint's exact physical budget before submission. It does not
infer destination allocation size from source bytes. A missing endpoint or
unavailable budget retains the source; a failed enqueue restores it and releases
only unused physical credit. The original deadline cannot be renewed by retry.

Before native submission, timeout, withdrawal or refusal can failure-settle the
exact logical ticket without GPU completion. The source descriptor and its slot
remain retained until the terminal status precedes SourceReleased in the exact
journal, or that journal is disconnected. Clock errors retain custody. After
successful enqueue, the existing opaque-receipt path owns all settlement;
uncertainty still cannot release anything.

Evidence: `t339-owned-capture-20261020/source-retention-01`. Supplied-descriptor
controls cover endpoint absence, timeout/journal pressure, old-grant disposal,
shared capacity, physical-budget refusal, expired submission, malformed/foreign
tickets, clock regression and actual native-inbox enqueue refusal. They prove
pre-use retention and rollback, not a real GPU copy or DMA-BUF admission.

The production offer path remains sealed-only. No live source-ingress caller
uses the new private retention API yet. Next are admission-time capture
reservation and kind-aware Source transfer, negotiated device/format and byte
bounds, and real provider-completed GPU qualification. The intake must derive
its deadline from negotiated validation/transfer limits. No GPU capability,
default behavior or performance claim changes in this slice.

#### Remaining source intake and qualification

Use the existing owner, not a second capture thread or context. The bounded
mailbox carries runtime-neutral job IDs, source descriptors and metadata; Session
retains the grant/connection/content-generation binding. The renderer alone may
produce a completion receipt for the exact job and renderer generation. A full
or failed enqueue returns source ownership and capacity unchanged.

Cancellation before submission permits rollback. After submission, cancellation
only forbids publication: the job must still reach proven completion or retain
its source and capacity in quarantine. Executor failure and generation change
must settle every pending job as uncertain, never fabricate success or silently
drop its reservation. The runtime owner reconciles even withdrawn jobs and
rechecks authority before publication. Tests must pin these transitions before
the GPU admission path is enabled.

The renderer owner retains its Rc/EGL context and performs context-bound fence
operations. Before enabling admission, wire the private handoff to the runtime
image owner and real context-bound completion operations, with the exact
renderer device generation. A successful completion identifies the copy job and retained
source descriptors; a destination fence for another operation grants nothing.
Keep fault injection private to tests. Stale or duplicate completions cannot
publish a resource, release a newer job or return its reservation.

Existing snapshot fences guard destination-allocation reuse and are polled
inside renderer operations. Their quarantine and internal EGLSync destruction
are not the standalone source-completion event; the new shell adapter observes
its own capture fence before it is transferred to those existing paths. Neither an
Arc-backed snapshot nor a current snapshot epoch supplies that event.

Specify how a retained image is charged against shell resource limits and the
renderer snapshot budget. The same backing can be constrained by both limits,
but mirrored reservations must not become two physical allocations or duplicate
release credits. Logical resource bytes and actual owned allocation accounting
remain distinct; every reservation has one owner and one terminal disposition.

Muse independently reviewed the source audit at 7a57dca1f on October 20,
read-only, and confirmed the source gaps and sequence. These handoff and budget
requirements incorporate that review. That audit review claimed no new tests.
The later implementation controls above do not claim GPU execution or performance.

## Connections

- [Portable-core proposal](../investigations/oofo1zrq-a-portable-9p-core-with-optional-drawing-and-surface-clients.md)
  separates optional drawing, immutable import and mutable pools.
- [Accepted image-channel decision](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
  and [current channel contract](../../sophia-image-channel.md) own custody.
- [Presentation versus execution](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
  preserves renderer neutrality and states GPU isolation limits.
- [Shared shell/lock review](x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
  retains existing lifecycle evidence; lock coverage is outside the first fixture.
