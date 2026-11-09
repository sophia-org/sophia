---
id: ti5plluj
date: 2026-11-09
kind: investigation
status: implemented
tags: [investigation, performance, shell]
---
# Can terminal owned-ring setup return before moving state without changing servicing

## Question and scope

After [hp-01](h3gtssf5-which-functions-consume-cpu-on-the-current-lean-sft-path-without-changing-responsiveness.md),
niltempus authorized the bounded source investigation of construction inside
component servicing. Preserve every service visit, caller-fresh custody check,
phase order, deadline, publication and release rule. No caching, extra buffering,
protocol change, worker/owner transfer or native repeat is part of this step.

One concrete candidate survives: return from owned-ring setup after the disclosure
check when registration is already `Ready` or `Refused`. Source candidate
`a4f599e7a2cedbdbe1c39f6c5752877446faaa9b`, branch `cpu/owned-ring-idle`, parent
`6817e7bca`, is separate from master. It changes eight runtime lines and adds two
contract tests using existing fixtures. No runtime merge or installation yet.
Temporary allocation lists in content servicing were also read; they remain
unselected. Removing completed setup movement needs neither a new container nor
a cached snapshot.

## Evidence and attribution limit

Frozen input is hp-01's normal host `ad0c5239d`, binary `e42d0fe9`, result seal
`25c5bf5e`. Six of its 660 owner samples are memcpy leaves with caller PCs inside
`service_owned_ring_setup`. Offline stack offsets and disassembly identify four
copy call sites, including setup movement and the return value. Attribution:
`~/.local/state/sophia-owned-ring-setup-attribution-01/`, SHA256SUMS
`c9305649673bb2f701a5955eb626c6d61e5da7b90ff28197a7b5c119ff395447`.

The attempted `perf annotate -s` returned no direct samples in the setup function;
its warning and empty output are retained. Saved stack PCs plus disassembly,
not that annotation, support the call-site association. No sample records the
setup phase. Six samples are neither elapsed time nor a removable budget, and
the inclusive component-service share cannot price this guard. No new perf
recording was made.

## Why this is an identity path

At the parent source:

- `runtime/src/shell_transport/files/export/owned_ring/setup.rs` checks
  `owned_disclosed()` first and resets on failure. It then takes `Setup`, calls
  `advance`, restores it and returns the result.
- In `Setup::advance`, the deadline match leaves `Ready` and `Refused` unchanged.
  The first loop iteration restores the same phase and returns `Ok(None)`.
  Neither terminal phase owns an FD, ring or reservation. Neither path appends a
  journal record, updates published sequence, polls liveness or accesses epochs.
- `owned_ring.rs::owned_deadline` already excludes both terminal states. A refused
  registration stays registered and prevents a different registration until reset.
- `owned_copy.rs::service` observes unchanged publication sequence and `Ok(None)`
  in either version, retains its active ring, and continues the same settlement,
  release and offer servicing. This change returns from setup, not from service.

The new guard is **after** disclosure/reset and **before** `registration.take()`.
It borrows the phase; all nonterminal state transitions remain byte-for-byte
unchanged. It preserves the old panic behavior on nonterminal paths instead of
replacing their take/restore with a broader borrow rewrite. M1/M2 remain deferred;
B6 caller-fresh checks stay per pass. All four owner services remain.

## Device-free proof and review

The ready test completes the real memfd/seqpacket setup and leaves a pending
source offer with a live credit. Service before/at/after the original deadline
retains phase, identity, pending offer, journal, publication and accounting.
Subsequent disclosure loss resets registration, credits and pending state while
the external ring still owns its source charge. The refused test reaches timeout,
then checks sticky refusal, unchanged journal/accounting, EALREADY on another
registration, and reset when negotiation is lost.

The initial tests pass on the parent (24 terminal-filter tests). The candidate's
owned-ring filter passes 50 tests. A deliberately incorrect mutation moves the
guard before disclosure checking: exactly the two new tests fail at the missing
registration reset, with 22 other terminal-filter tests passing. That mutation
was restored before final gates. Tests pin behavioral equivalence; they do not
measure a performance effect.

The full isolated `cargo xtask check` passes: 809 successful test-result blocks,
zero failures, with fmt, clippy, conformance, layout, C-wire, Python and archive
checks green. Runtime rustdoc also passes. The wrong-order mutation was repeated
with the exact committed tests: both fail, and restoring the source makes both
pass; the worktree is clean. Source evidence:
`~/.local/state/sophia-owned-ring-setup-source-01/`, SHA256SUMS
`542aac183831775059e371b6beef1d225c2bccc5ee90b4dbedad4797af651ac3`.
Independent review: `~/.local/state/sophia-owned-ring-idle-review-claude-01/`,
SHA256SUMS `e35d1522c3becc23638b41f8e13795be07a7e4087b2399e2db797c0e6b3db2d5`.
Its gate status was pending when frozen; the root receipt records later success.

Release-library assembly inlines setup into `OwnedCopyOwner::service`. Both
terminal branches precede the setup-copy block and form `Ok(None)`; subsequent
service still runs. The compiler-specific excerpt and assert-based check are
retained in source evidence. This proves the selected code path bypasses those
copies, not a timing effect or a complete integrated-host code-generation result.
Source candidate accepted; worktree and pool were retained for the comparison
below. The branch remains unmerged.

## Matched effect package, November 9

Niltempus selected one host-only ABBA of exact parent `6817e7bca` against
`a4f599e7a`. Package `~/.local/state/t331/ring-01/changing` is sealed at MANIFEST
`42e7091efd0201035c5d27da6e190d5ec0dd10eb432ec4e8664de5fea01aa870`.
Order is control, candidate, candidate, control. The four arms retain hp-01's
peer, SDK, runner, helpers, configuration and lean changing-strip workload, with
cpu-strip-03's process-clock accounting and cadence/identity/cleanup gates.
Normal features are enforced on both hosts; no native perf, strace or owner-cost
instrumentation. There is no automatic repeat, adoption, runtime merge or install.

The hosts were built from clean signed worktrees in distinct cargo-slot pools
`sophia@sophia-ringctl` and `sophia@sophia-ringidle`. Control binary
`e42d0fe9b73a545ffdc3c22c3120c050579212c9c085d2928f17a251e810020d`
reproduces hp-01's normal host; candidate binary is
`5948a8303737f656e2109938ae06462f2c116f21d3e17532baae6a2af5b835ee`.
Both match the held release recipe, toolchain, profile and feature rows. Build
seal `~/.local/state/sophia-owned-ring-idle-build-01/SHA256SUMS`:
`6acd40c087dafbc5c5ee3a1b22973950fd264bc6f742e6e23d16663fa218ddfe`.
Worktrees and pools were pinned through result sealing and the independent
recount. Both clean worktrees were then removed with ordinary `git worktree
remove`; candidate refs remain locally and on origin. Pools are left to nightly
hygiene, with no binary copies or broad cleanup. Frozen evidence is untouched.

All 18 package suites pass. The bounded package review accepts the arm isolation,
build parity, unchanged gates and new descriptive latency join:
`~/.local/state/sophia-ring-01-package-review-claude-01/SHA256SUMS`,
`cafb46d14883148d0a58058d2b71665a4b2cb79177d3698dee771a86afbfa56a`.
Readiness `~/.local/state/sophia-owned-ring-idle-readiness-01/SHA256SUMS`
is `95ccb4fb726c4ae8594bec6af9e6ea8fd4b04b06fc157d79c6c875112af4545a`:
check-only passes, zero GPU/input device opens and connector status reads,
zero native launches at readiness. Niltempus subsequently launched it once.

Report CPU per completed frame, pair deltas, within-variant spread, native
submission-to-completion and peer offer-to-observed-Presented distributions.
Peer latency joins unique connection/grant/output/generation stamps, uses
generations 121..2120, and includes observation delay; it excludes earlier
fill/input age and is not input-to-photon latency. Latency is descriptive, not
a new threshold. CPU nonregression alone cannot justify adoption if the latency
or cadence evidence is concerning.

Prior between-arm variability does not establish a detection floor. A CPU delta
interval containing zero is inconclusive, not proof of no effect or an effect
below resolution. Neither a null result nor source proof automatically decides
keep/drop. No saving magnitude was predicted. The single owner launch and root
audit are complete; the source disposition remains a separate decision.

## Matched result, November 9

All four arms completed and qualified. Root result:
`~/.local/state/sophia-owned-ring-idle-result-01/SHA256SUMS`,
`70546268ae8830108a772123ec1059e5e5f0b3e5477119d9387f62247649e565`.
The raw-record recount reproduces CPU brackets, per-process and tick bounds,
latencies and ABBA reduction without importing the package reducer. All 128
analysis input hashes, the original package manifest and 508 pins verify.

| Combined CPU, µs/completed frame | Lower | Upper |
| --- | ---: | ---: |
| Control mean | 699.488006 | 700.003314 |
| Candidate mean | 699.342410 | 699.820884 |
| Candidate minus control | -0.660903 | +0.332879 |

Nonregression passes the predeclared 5% rule; **saving is inconclusive**.
The midpoint difference is -0.164012 µs/frame (-0.023439%). Pair midpoint
differences have opposite signs: +3.352754 and -3.680779 µs/frame. These
accounting intervals are not confidence intervals; neither the midpoint nor
the source-level elimination proves an effect.

The 2000 counted completion gaps span 16662..16673 µs in b1, 16663..16671 in
c1, and 16666..16668 in c2/b2. All pass [0.5,1.5] refresh periods, with zero late
slots. Native latency median pair differences are -1/-2 µs; peer-observed
latency median differences +1/0, p99 +8/-4 and maxima +16/+11 µs. These are
descriptive observations under the stated timestamp limits, not a control/input
responsiveness proof. Per-arm distributions and CPU splits are in the report.

All postflights and cleanup pass: four normal lifecycle handoffs, PAM cleanup,
child status zero, no residual PIDs, tty restored, empty outer cleanup, seat
unchanged, regular persisted logs and no remaining per-arm tmpfs sources/live
links. tty4's agetty is restored. `TEST SUCCESS` means qualification, not saving.
The window is closed; independent recount matches the sealed data:
`~/.local/state/sophia-ring-01-recount-claude-01/SHA256SUMS`,
`91c585adadef00f314414c77aa350edda0525ffe3e63639daf8cda5399460c55`.
It caught a prose error in the root REPORT: 16666..16668 was incorrectly stated
for every arm. Per-arm JSON and both recounts already have the ranges above;
the frozen report remains untouched, with this correction carried separately.
Acceptance and correction:
`~/.local/state/sophia-owned-ring-idle-result-acceptance-01/SHA256SUMS`,
`a696c310bf97a9c6a696f9f23128e5b86c8d1f92181fadabe6814b4773bbeb90`.
No native repeat, runtime merge, installation or automatic adoption. The source
proof stands independently; keep/drop requires a distinct decision and must not
be presented as a measured CPU improvement.

The owning [CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and `todo.md` t344 carry execution order. The previous architectural deferrals
and the user's responsiveness constraint remain unchanged.
