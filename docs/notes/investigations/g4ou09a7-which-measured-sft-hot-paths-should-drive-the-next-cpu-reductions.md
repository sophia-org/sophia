---
id: g4ou09a7
date: 2026-11-05
kind: investigation
status: investigating
tags: [investigation, cpu, sft]
---
# Which measured SFT hot paths should drive the next CPU reductions

## Question

Which active work can we remove from the native SFT path while preserving
custody, bounded admission, progress and atomic tear-free presentation?
niltempus admitted all five measured opportunities to t344 on November 5.
Execution order and task state remain in `todo.md`; the table below defines
scope and proof obligations, not five already-accepted implementations.

## Evidence

Control: signed host `29ee6d99f`, peer `67fd3f7e`, SDK `ae473881`, runner
`cf8b9e84`. The qualified `sft-01/changing` comparison reports combined
r10 [1060,1100] versus SFT [827.5,867.5] us/frame, difference
[-272.5,-192.5], one ABBA block. Receipt: `~/.local/state/sophia-frame-native-result-01`,
SHA256SUMS `8a39f4f391a58bfe3cff8ff102f1714b39a3b04457f06bb003d7339a870eb732`.
This is default-off reference-workload evidence, not installed-desktop adoption
or a fresh comparison with niri.
The recorded submit-to-completion median increased by about 0.32 ms in that
comparison; its cause is unassigned. CPU improvement alone does not close latency
acceptance.

The separate `sft-profile-01/changing` capture contains 1315 samples over 1501
frame intervals (generations 158 through 1659), zero lost samples and unchanged
bound thread sets. Owner 784, validation worker 39, other Session threads 240,
peer 252. Saved address-free reduction:
`~/.local/state/sophia-frame-hotpath-result-01`, SHA256SUMS
`1b9d99f8214585d08d1d3c11303a637f57ca99632e3183378da1304c8e1ce357`.
Independent capture audit seal:
`7c023f05d1e7c015d583852db4e2f186e92cddaccca05e8ce68c7f8baf18b40e`.
Raw addresses stay local. Profiling CPU brackets and latency are not comparators.

The five scopes below are disjoint in this decode. Their shares include necessary
work and are not predicted savings. Other/unresolved work remains outside them;
20/784 owner and 55/240 other-thread leaves are unresolved. GPU clock-state
equivalence with older reference runs is unrecorded; the reported saturated SMU
metric alone establishes neither load nor a faulty metric.

## Finding and resolution

| Order / scope | Samples / total | Smallest useful proof before a candidate | Required contract |
| --- | --- | --- | --- |
| 1. Component servicing | 358 / 27.22% | Separate input turns from publication flushes using the existing owner-turn/image-readiness fixtures; remove demonstrated duplicate work only. File-wire service is 186 samples, including 56 role-visit flush samples; image service within those is 36 (2.74% total). | Publication reaches parked reads in the same pass; new image input remains level-ready for the next input turn; authentication, custody, expiry, disconnect, ACK/backpressure and worker deadlines remain enforced. |
| 2. Native submission/completion service | 139 / 10.57% | Trace the existing order of component presentation observation and native completion in a deterministic fixture; prove any redundant traversal or delay before changing order. | Exact epoch/lease settlement, one terminal outcome, atomic display, input fairness and descriptor-less preparation/retirement deadlines. |
| 3. Peer SDK drive | 128 / 9.73% | Count drive/read/ACK operations in existing scripted file-wire tests, including buffered events, journal-only image transitions and an idle follow-up. | FIFO, bounded memory, retained-event/ACK frontier, cancellation and transport errors. Compare with the existing drive-boundary cumulative ACK batching; do not assume one ACK per event. |
| 4. GL composition on the renderer thread | 114 / 8.67% | Use existing retained/changing fixtures to prove redundant upload, allocation or draw work on the active path before changing it. | Pixel identity/version, buffer age/damage, resource lifetime, partial/full repaint equivalence and tear-free completion. No speculative Vulkan or SIMD rewrite. |
| 5. Peer enqueue including fill callback | 44 / 3.35% | Distinguish the inlined fill callback from outbox preparation with the existing enqueue/fill fixture; count copies/allocations before choosing the change. | Reserve before publication, failure leaves ring ownership valid, exact pixel bytes, no uninitialized exposure. The inclusive symbol is not an outbox-only cost. |

Proceed one coherent candidate at a time, first off the unchanged measured host.
For item 1, the control's shared `ShellFileWire::flush` repeats image-channel
service after ordinary owner input service. Its capture-only `flush_publications`
already separates those operations. The candidate reuses that boundary for all
publication flushes and removes the redundant capture-only wrapper; it adds no
end-of-pass framework.

The saved owner counters show about 7.7-7.8 passes/frame under SFT, one Service
expiry/frame and **zero ShellOutput expiries**. They do not identify the Service
expiry's cause or prove it removable. No cap removal, cached liveness, R1/R3
stacking, legacy R2 activation, or multi-mechanism tier bundle is authorized by
these shares. The metadata-shell R2 seam remains absent from this workload.

## Validation and remaining work

### Current resumption

All five scopes have separate signed, published source candidates. None is
merged or adopted. The first native qualification tested the publication-only flush:
`~/.local/state/t331/sft-flush-01/changing`, one r13-versus-r13 ABBA against its
exact parent. Only the host changes; the peer, SDK, runner, workload and gates
stay fixed. All four arms completed and the final seat check passed. Preliminary
saved reduction: candidate minus control [-42.5,+37.5] us/frame combined;
nonregression passes, but the interval includes zero. No demonstrated saving,
repeat or adoption. Independent result audit agrees, at
`~/.local/state/sophia-sft-flush-01-audit-claude-01`.
The SDK buffered-event drive comparison also completed:
`~/.local/state/t331/sft-sdk-01/changing`, host `29ee6d99f` fixed, peer
`67fd3f7e` versus pin-only `5d2fcd4`, both r13. Its twelve device-free suites
pass; owner command paths are checked explicitly. All four native arms passed,
cleanup and final seat checks completed, and the log emitted TEST SUCCESS.
Saved combined control bounds are [850,890], candidate [847.5,887.5] us/frame;
difference [-42.5,+37.5], pair midpoints +10/-15. Nonregression passes but saving
is inconclusive. Park this candidate without repeat or adoption; do not stack
it onto the next control. Root reduction: `~/.local/state/sophia-sft-sdk-result-01`.
The next admitted work is the deterministic full owner-service pass proof below,
off the unchanged measured host `29ee6d99f`, peer `67fd3f7e`, SDK `ae473881`.

The approved route toward niri's earlier 415–455 us/frame reference is: qualify
this SDK candidate once, then prove and remove redundant complete owner-service
passes rather than individual calls. Current SFT controls cost 847.5–887.5
us/frame; 450 combined is an engineering target, not a fresh comparison. Use
the real-owner fixture to follow commit, capture completion, renderer completion
and native completion. Admit one scheduling candidate only when the fixture
shows a redundant traversal/pass removed within existing work budgets and
fairness. Preserve independent source/credit/resource release, wake races,
revocation and all timer fallbacks until actual wake/deadline coverage is proven.
Re-profile after a material qualified change, then make a fresh matched niri
comparison near the target. No native repeat, protocol rewrite, Vulkan switch
or standalone window for tiny cleanup candidates is implied.

Future native packages must use the reviewed completion-status delta in
`~/.local/state/sophia-native-run-status-01`: arm number/start/pass messages,
then an explicit `TEST SUCCESS` only after final checks and receipt persistence,
or `TEST FAILED` on a propagated exception. Both say when to report back and name
the result path. Check-only is not a completed test, and completion is not proof
of a performance saving. Preserve the consumed package; integrate the delta into
the next sealed package. A hard kill or machine failure may prevent any final
marker, so its absence is not evidence that the test is still running.

| Scope | Candidate | Device-free result and next gate |
| --- | --- | --- |
| File-wire service | `93d181434`, `cpu/sft-service-batch` | Duplicate image input removed from publication flushes; progress tests pass. First native block inconclusive; no adoption or repeat. |
| Native retirement | `df0e9c1f6`, `cpu/sft-native-retirement` | Allocate callback storage only when a completion exists. Feature suite: 316 passed, one opt-in skipped. Keep as unqualified source work; native call frequency and saving unknown. |
| SDK drive | SDK `f522b0cff`, peer pin-only integration `5d2fcd4` | Eight-event automatic burst: recvfrom 11 to 4; retained burst: 27 to 12, with sendto unchanged at two. FIFO, ACK and readiness tests pass. Native block inconclusive; parked without repeat or adoption. |
| GL texture setup | `0041cc452`, `cpu/sft-texture-setup` | Immutable wrap parameters set on allocation rather than every upload. Renderer suite: 112 passed, 31 opt-in skipped. Two calls avoided per reused upload; native saving unknown. |
| Peer fill | `fcb1a70d5`, `cpu/sft-fill-vectorization` | Safe fixed-size pixel-slice fill preserves bytes, tail validation and neighbour guards. Release assembly vectorizes the actual enqueue path; eight peer tests pass. Standalone old/new helpers both vectorized and did not measure a saving. Native effect pending. |

Source evidence is in `~/.local/state/sophia-sft-service-batch-01`,
`sophia-sft-native-retirement-01`, `sophia-sdk-drive-buffered-events-01`,
`sophia-sft-texture-setup-01` and `sophia-sft-fill-vectorization-01` under the same
state directory. Each has a SHA256SUMS seal and independent review. Release
receipts for flush, SDK peer and fill are in `sophia-sft-service-native-build-01`,
`sophia-sft-drive-peer-build-01` and `sophia-sft-fill-native-build-01`.
Commands, toolchains and build environment match their controls. No binaries are
copied into evidence. The retirement target's strict local Clippy passed with
`--no-deps`; the initial dependency-inclusive run hit an unchanged renderer lint,
recorded in its receipt. No source fix was bundled to silence it.

The first source candidate is `cpu/sft-service-batch`, based on `29ee6d99f` in
an isolated worktree. Publication flushes keep the shared custody, expiry,
parked-read retry and write path, but stop re-serving image input. Ordinary input
turns still service the image channel. No wire, SDK, owner wake, timer or credit
policy changes. The measured control is untouched.

The existing image-readiness fixture now publishes during a role visit after an
observed input turn. The old implementation fails because its flush accepts a
late duplicate image connection. The candidate leaves it pending, checks the
image listener itself remains readable, and refuses it on the next input turn;
the admitted channel stays open. Existing owner-turn coverage checks same-pass
publication delivery to parked reads. This is deterministic mechanism/progress
evidence, not a count of native flushes per frame or a CPU saving.

Validation checkpoint: the full runtime run with SFT, capture and custody-dispatch
features passed (42 opt-in tests skipped), as did the focused default and feature
image/owner/readiness tests, strict runtime Clippy, changed-file formatting,
source layout and documentation-link/task-identity checks. Independent local
review passed. Evidence is retained at
`~/.local/state/sophia-sft-service-batch-01`; no native run or adoption occurred.

### Candidate qualification

For each candidate: pin a deterministic failing-before/passing-after mechanism
check, connected progress/security tests and relevant lint/build checks. Only
then prepare an isolated native comparison on the same r13 workload with all
other identities held equal. Predeclare combined CPU as primary, per-process
bounds, mechanism movement, cadence, latency, identity and cleanup gates.
Require the upper candidate-minus-control CPU bound below zero to claim saving;
otherwise record inconclusive/drop without fishing for repeats. A mechanism
proof or nonregression result alone is not a demonstrated CPU gain. Tiny source
wins may be retained as explicitly unqualified work rather than spending another
native window below the accounting resolution. Re-profile only after a material
path change or a specific ambiguity blocks the next decision.

SFT remains default-off. Native profile coverage and the reference ABBA do not
complete Lom/live-desktop acceptance or authorize adoption. A new matched
external reference is required for a claim that Sophia beats niri/Smithay.

## Connections

- [CPU critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md#immediate-critical-path): t344 owns execution and dispositions.
- [SFT architecture](../../native-frame-transactions.md): eligibility and fallback remain unchanged.
- [SFT wire contract](../../sophia-frame-transactions-wire.md): stream, credit and custody rules remain authoritative.
