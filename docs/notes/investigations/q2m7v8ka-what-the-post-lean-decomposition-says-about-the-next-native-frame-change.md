---
id: q2m7v8ka
date: 2026-11-08
kind: investigation
status: investigating
tags: [investigation, session, performance, frame-transaction, custody]
---
# What the post-lean decomposition says about the next native frame change

## Result and scope

`dcp-01/changing` completed one diagnostic native arm at source `9d1708744`.
The root and independent recounts pass with zero field disagreements. Source remains on its
diagnostic branch, not master or an installed release. No second arm or repeat
is selected. The capture supports investigation, not a saving, regression,
removable-cost estimate or numeric security premium.

Frozen result: `~/.local/state/sophia-decomp-post-lean-result-01/`, SHA256SUMS
`7a0677ef83845072794eb34925c0bc7fe231c369da1d9216b53e80f41caf8a5c`.
Input package MANIFEST:
`7e300ac74308034ac4f5e7c9092ecfaffd5f77c7718880eb9631be3a7da658cf`.
The sealed root reader imports no package reducer. It reproduces raw owner
rows, process-clock brackets, role census summaries, worker snapshots and
service counters; it verifies all 358 input pins and 43 analysis input hashes.
Full raw evidence remains under `~/.local/state/t331/dcp-01/changing/`.
Independent audit: `~/.local/state/sophia-decomp-post-lean-result-audit-claude-01/`,
SHA256SUMS `4d159153ba590616e43686d0dd7b039a1adde9d38906ebd0c05e024b9269e3d1`.
Acceptance: `~/.local/state/sophia-decomp-post-lean-audit-acceptance-01/`.
Both readers use the same raw capture and window rules; agreement confirms the
reduction, not representativeness. Frozen result files are unchanged; the sibling
acceptance supersedes their pending-audit status. The diagnostic worktree is
released for ordinary cleanup, retaining branch history and all evidence.

The workload is the held lean r13 owned-ring 1920×24 CPU strip at 60 Hz,
2,120 frames, two slots, terminal-only, capabilities `0x1c8183`, W2 off.
Common peer, SDK, runner and helpers are unchanged from liv-01. Treatment is
`diagnostic-owner-cost` plus per-role `strace -c`, not an uninstrumented arm.

## Measured owner phases

Native anchors 120/2120 cover 2,000 intervals. There are 7,999 complete owner
rows and two boundary rows carrying 228,589 ns CPU. The phases partition the
owner thread; each phase upper bound includes all boundary CPU, so upper
bounds are not additive.

| Phase | Instrumented us per completed frame | Meaning |
|---|---:|---|
| Components | 461.579–461.693 | The `component_service::service_components` closure |
| Other | 226.868–226.982 | Owner work outside explicitly scoped phases |
| Native | 226.680–226.794 | Explicitly scoped native operations |
| Authority | 101.996–102.110 | Authority/wait-control CPU, excluding nested native scope |
| Whole owner | 1017.123–1017.237 | Owner thread only |

Source boundaries are in the diagnostic tree's
`live_session/owner_loop/physical_input_loop.rs` and `owner_cost/enabled.rs`.
Other is a real remainder of the disjoint owner scopes, not a GL remainder
or a bucket of unnecessary work. Authority is not a security-premium number:
it includes wait/control work, while security checks also execute in other
phases. Sleeping wall time is not CPU time.

The whole rows carry four single-wait classes: native 2,000, wire 2,001,
fd 1,999, ring+Service 1,999. No whole row has zero or multiple waits. These
counts do not reconstruct every boundary pass. Separately, seven periodic
receipt pairs cover generations 265..2064 (1,800 completions): 7,201 owner
passes/waits, 7,201 supplied observed component turns and zero blind turns.
There are 1,800 Service waits, all ending on the ring, with no Service expiry.
All three rendering-in-flight reason marginals are 1,800; they overlap.

The four-pass structure remains. Neither a wait class nor a component visit
is proven unnecessary. B3's limit remains: supplied `Some` precedes server
validity, so zero blind turns does not establish zero fallback syscalls.

## What the syscall census adds

Before/after identities match for 41 TIDs partitioned across owner, validation
worker, other Session threads and peer. Every tracer detached with SIGINT and
no forced kill. Denominator bounds differ by role: owner 1,940; validation
1,939–1,940; other Session 1,938–1,939; peer 1,938. These intervals lie inside
the native anchors but are not the full owner window.

| Role | Selected aggregate calls per completion |
|---|---|
| Owner | ppoll 44; clock_gettime 36; read 23.025; wait4 12; write 10.022; recvmsg 7; accept4 3; sendto 2; recvfrom 1 |
| Peer | recvfrom 17, of which 15 errors; write 7; ppoll 2; sendto 1 |

All owner recvmsg and accept4 calls are recorded as errors. Their errno is
unknown. Counts are not CPU prices, per-visit emptiness or socket attribution;
the FD inventory is descriptive only. Diagnostic clock reads contribute to
clock_gettime traffic. Do not subtract earlier liv-01 custody counts from
this census to assign the remaining calls to a source.

One peer sendto per completion weakens the case for another generic send
batching change. It does not prove all per-frame requests are in that call,
identify the socket, or prove a shared control ring useless. Those require
request/record and authority contracts, not this aggregate summary.

## Worker and process limits

Between named owner snapshots fully inside the common identity-checked census
interval, completed render-command scope increases by 288,307,279 ns and
completed image-validation scope by 78,744,132 ns. Snapshot row stamps are
37384540208710..37416825903602 ns; counter sampling precedes those stamps.
These are completed scopes, not worker thread lifetimes or exact-anchor CPU
bounds. In-flight charges may arrive later. Regressions refuse; stable thread
sets and zero recovery/generation replacement counters support the lane but
do not prove absence of every hidden core replacement.

Process-clock context for this instrumented arm is Session 1356.224–1356.673,
peer 208.757–208.845, combined 1564.981–1565.518 us/frame. Do not compare it with
untraced SFT or historical Niri figures. Do not subtract worker counters from
Session CPU to invent a GL remainder or combine unlike windows. This capture
neither measures an uninstrumented security premium nor reaches a performance
target. The instrumentation itself changes the workload.

## Proposed next questions, not selected changes

1. **Owner coordination and component work first.** Components are the largest
   instrumented owner phase, and four owner passes persist. Reuse the
   [component-walk map](3u93ti2w-which-component-walk-visits-do-useful-work-per-ready-source.md)
   and [obligations-on-triggers map](rug93yha-which-per-pass-obligations-can-move-to-their-own-trigger-without-weakening-custody.md)
   to build a dependency table for the four waits: which action each makes
   ready, which action must execute on the owner, and which fresh authority or
   immutable-publication boundary separates it from the next action. A proposed
   consolidation must name an actual round trip or repeated operation it removes
   while preserving failure and release ordering. Four passes alone do not prove
   that any pair can be combined. The prior turn-only regression stands.
2. **Control ring only with a specific remaining round trip.** The peer already
   has one aggregate sendto per completion. A ring proposal needs to show which
   event read, reply, wake or owner transition disappears beyond that batching,
   and how it preserves mapping custody, sequence validation, revocation,
   backpressure and immutable acceptance. Preserve 9P's control namespace.
   A model and device-free interleaving proof must precede implementation;
   these counts do not justify selecting a protocol replacement.
3. **Renderer work needs its own correctly scoped question.** Retain the
   completed command/job deltas as evidence, without converting them to a
   process remainder or treating them as proof that rendering is cheap. Name
   a copy, allocation, upload or draw operation and establish its active path
   before proposing SIMD, a backend change or another native comparison.

The first proposal is an investigation priority, not a code or native-run
selection. Within it B8 remains unpriced, M3 needs lifecycle coverage, and M1/M2
remain separate authority decisions. The 12 wait4 calls/frame and repeated
polls can motivate source questions; they do not authorize dropping reaping,
fresh custody checks, deadline work or guarded delivery. No estimated saving
is assigned to these proposals.

## Qualification

All 2,000 counted native gaps are 16666–16668 us. Largest counted sampler step
is 5.042391 ms; late_slots zero; all_exited. Postflight and cleanup pass,
including VT/PAM restoration, empty residuals, agetty back on tty4, unchanged
seat, and canonical regular session.log with its named tmpfs staging removed.
No native rerun, build, merge or installation follows from this result.

Task selection and execution order remain in [t344's plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and `todo.md`. Existing closed comparisons and authority decisions are not
reopened by this diagnostic capture.
