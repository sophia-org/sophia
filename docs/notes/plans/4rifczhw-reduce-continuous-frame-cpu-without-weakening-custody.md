---
id: 4rifczhw
date: 2026-10-28
kind: plan
tags: [plan, shell, performance, custody]
---
# Reduce continuous-frame CPU without weakening custody

## Scope and exit

Reduce Sophia's CPU per completed frame for the changing 1920×24 owned-ring
panel without weakening immutable acceptance, admission, authority freshness,
revocation or release-after-last-lease. Mason requested this plan on October 28,
2026, after the matched desktop comparison and the architectural review with
Muse and Claude.

This follows the [t331 evidence and t343 qualification plan](mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md)
and informs [t289's broader animated-workload work](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md).
It does not reopen their completed exits or replace `todo.md` as the task ledger.
The exit is a separately qualified improvement, or a documented no-change result
for each selected mechanism. A profile share is not a savings target.

## Current resumption

The first question is how much repeated control and lifecycle work can be
removed while keeping the current guarantees. There is no measured irreducible
Session floor. Muse withdrew the earlier “about 1.2 ms/frame” floor and the claim
that matched external comparisons were absent. Historical 60 Hz arms, profiled
120 Hz arms and the uninstrumented comparison must remain separate.

W2 retires a predecessor on presentation of its successor. Its device-free
codec, negotiation, custody and lifecycle proofs are on `cpu/w2-prototype` in
`~/dev/sophia-cpu-w2`; option (b) closed at signed local `7cc36ebde`. The
owner-wait proof closed at `c3abceac6`. It proves collection at the next explicit
resource-service call when leases and capacity permit, not a 25 ms wall-clock
release bound. The 61 failures in the earlier broad native-session run were
reproduced and isolated to group-writable checkout KDL inputs, as recorded below.
That closes their cause, not a new candidate's full test gate. The three focused
planner tests passed and their mutation failed.

Mason said “Implement the plan” on October 28. The lanes below are now active
in parallel, rather than waiting for W2's native result. Claude owns W2 wiring
in its isolated host, peer and runner trees. Host wiring is signed at
`88c1cda50`, peer counters and SDK repin at `b12b0a1c`; runner wiring is in
progress. The original host already emits negotiated revision, but no capability
word. W2 artifacts extend that existing receipt; the original-artifact screen
keeps its original acceptance rather than inventing missing fields.
The native-owner agent owns S1's remaining fixtures and default-off Session
wiring. W1's delayed event delivery remains shelved. Earlier custody-dispatch
and batching screens did not resolve a gain; frequency alone is not cost.

`continuous-release-overlap-01` is complete and inconclusive, with no promotion.
Its temporary build hold is lifted; its exclusive native attempt must never be
rerun. The latest successful uninstrumented comparison is `desktop-cpu-04`.
No old package, gate, result or source pin is to be rewritten.

Codex owns integration review, the test/provenance blockers and package seals.
Producer owns single-submit source/build receipts and a fresh unsealed ABBA;
transport owns an opt-in baseline attribution diagnostic. Muse owns a written
bounded frame-stream design, not a production protocol change. Coordinate
through herdr; source notes and receipts are authoritative and mem0 is an index.
No agent changes another lane's checkout or a frozen package. Mason launches
native attempts over SSH with sudo, one at a time; builds pause during captures.
Local SDK/import pins and missing C parity are permitted only as explicitly
recorded diagnostic exceptions, never as public protocol allocation or merge
approval. No storage or authority-window change is authorized by attribution
alone. The exact mechanism must receive its own custody review.

## Immediate critical path

The task ledger is `todo.md` t344. These are concurrent assignments, not a
serial backlog. A blocked lane does not stop an independent ready candidate.

| Lane | Owner | Immediate implementation and exit |
| --- | --- | --- |
| W2 | Claude | Correct the two design contradictions; implement and test host/CLI policy, peer SDK pin/counters, and runner/qualification in separate reviewable commits. Original-artifact acceptance stays separate from W2 off/on acceptance. |
| Single-submit | Producer | Review the four source trees and dependency closure; build/test in its own cargo-slot pool; stage `continuous-single-submit-01`, instrumentation none, unsealed for root review. No new GPU test is assumed. |
| S1 | Native-owner | Finish admitted-authentication, native accept/activation and send-before-notify fixtures; wire the event-read-only probe behind a default-off feature, with the original deadline and exact failure retention. |
| Cost attribution | Transport | Reuse pass-cost instrumentation to separate immutable copy, admission, settlement and retirement; count logical records per turn and authority checks by site, with disabled-path and accounting tests. Diagnostic only. |
| Frame stream | Muse | Deliver a bounded-credit state machine, compatibility and failure matrix, logical exchange prediction and falsifier; no production code or overlap with W2. |
| Integration gates | Codex | Classify the 61 native-session failures and renderer lint, review every changed boundary, pin artifacts and seal only after checks and independent review. |

Native queue priority is single-submit, baseline attribution, W2 side-effect
screen then W2 off/on effect, and S1. A not-ready package does not block another
ready one. Each has fresh inputs and one exclusive attempt; no rerun or overwrite
of historical evidence. The attribution arm is not a CPU comparator.

For the W2 side-effect screen, predeclare a maximum unresolved regression of
50 µs/frame combined and 25 µs/frame for either bound process, using the upper
candidate-minus-baseline accounting bound. Exceeding a margin is not accepted;
insufficient precision is inconclusive. This is a regression screen, not an
equivalence claim. A beneficial effect requires an upper CPU difference bound
below zero, its predicted mechanism and the existing correctness/cadence gates.
Do not reject an improvement because its interval excludes zero.

Keep initial Cargo jobs at two per lane, reuse existing pools where their source
identity is correct, and never share a target directory across worktrees. Record
signing failures without changing gpg-agent; unsigned work remains reviewable but
cannot enter a sealed source set. No unrelated cleanup, push or merge is part
of the immediate experiments.

### Implementation checkpoint, October 28

`continuous-single-submit-01` is sealed after independent root and Muse reviews:
92 input entries, 197 external pins, MANIFEST SHA-256
`67b5f665aeb33f80aa7e54e9473d7e6781c141536c911c2f1d7477dc8749cbed`.
All four check-only preflights passed; no Session or device was launched. Its
runner binary is `62df9c30` from source `2329fca7`, not the current main-pool
runner. Preserve that exact pin. The baseline two-submit test remains reported
and reconstructed, not a full retained baseline counter receipt.

S1 default-off wiring on `596ee463e` is review-ready, not yet signed. Root and
Muse found no code blocker; the fresh full native-session/capture/probe
library gate passed 1119 tests, with zero failures and 65 ignored. Its admitted-authentication fixture uses supplied authority and does
not prove Factotum or protected launch custody. Probe re-waits add waits to the
experimental counters: compare full passes and completed quiet re-waits, not
baseline wait-attribution counts as if they had identical semantics.

W2 host `88c1cda50` passed its fresh full native-session/capture/W2 library gate:
1108 passed, 65 ignored, zero failures. Its 36 tracked KDL files were changed
from 0664 to 0644 without changing bytes. Peer `b12b0a1c` records counters only
after successful sends and validated releases; the historical peer omits the
extension and keeps its old end bytes. Neither is native performance evidence.

### Test-environment disposition, October 28

The reported native-session run failed 61 tests. Using the same retained test
executable (SHA-256 `f7c8f4bdcbe2262ab762cd66eb86e633ce996f65687342eab7e26b70ac6908d6`),
root reran exactly those 61 in a device-free Bubblewrap namespace with a fresh
`/tmp`: original checkout KDL modes produced 0 passed / 61 failed; byte-identical
private read-only overlays for 36 tracked KDL files produced 61 passed / 0 failed.
The original checkout remained unchanged, including its 0664 modes. Both outcomes
and the earlier incomplete-overlay attempt are retained under
`~/.local/state/sophia/cpu-gates-20261028/`; the complete counterfactual receipt is
`tracked-kdl-mode-proof.json`. This proves the metadata cause for those failures,
not the full source provenance of that old executable or a passing new release
gate. Each new host still needs fresh tests on its exact source with owner-safe
fixture/config modes. Never weaken `ConfigIoError::UnsafeMode` to make tests pass.

The renderer `filter_map` lint is feature-dependent: the ManagedCapture arm uses
`?` only with `experimental-shell-capture`. The baseline release recipe enables
`native-session,experimental-shell-capture`. Check that actual feature closure;
do not blindly replace `filter_map` with `map`, suppress warnings globally or
call an unrelated feature configuration passing. Fresh results remain required.

### Frame-stream architecture

Muse's [standing-credit design](../investigations/tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md)
is on the critical path now. It keeps W2 retirement and image custody separate,
replaces repeated demand/permit with bounded pacing credit, and adds exact
terminal-outcome replenishment. Initial credit expiry starts at server grant;
replenished expiry starts at server commit, never client receipt. Prepared cannot
replenish; configured rate and per-generation response reservations still apply.
Expiry closes the stream and requires explicit reopen. The note is a design
deliverable, not permission to bypass those checks or implement a public wire
revision. Counter attribution and the device-free state-machine proof precede
production wiring. Record counts alone do not predict turns or CPU savings.

## Evidence, with its limits

All CPU figures below are microseconds per completed frame. CPUwin ranges are
accounting bounds, not confidence intervals. External rows cover bound server
and client thread groups, not the whole desktop or kernel workers.

| Evidence | Result | What it establishes |
| --- | --- | --- |
| `desktop-cpu-04`, Sophia, instrumentation none | Session 835–855; peer 465–485; combined 1300–1340 in both repeats | Current concrete owned-ring path at 119.88 Hz; not a lower bound on the architecture |
| Same comparison, XLibre | Server 75–95; combined 85–125 in both repeats | CPU-filled SHM strip, PageFlip off workaround, COPY at vblank; not proof of per-buffer scanout or identical producer work |
| Same comparison, niri | Combined 300–340 and 295–335; server 275–295 | Same strip geometry/cadence; CPU producer differs from Sophia's GPU render/readback |
| `continuous-pass-stage-cost-01` | About 9.05/9.02 wire-started passes per interval; 455/452 owner-body µs per interval, about 71% of owner body | Work to investigate, including useful turn work and all services in those passes; not removable overhead |
| Same diagnostic, peer main thread | Render/readback 327/311; ring upload about 17; total 497/480 | Different frame-function window, generations 121–2120; helper threads and GPU time excluded |
| `continuous-ack-coalesce-02` | Combined difference [-62.5, +22.5], midpoint −1.52% | Inconclusive; neither cheap framing nor per-record causality is proved |
| `continuous-release-overlap-01` | Combined difference [-55, +25], midpoint −15 (−1.14%); peer [-15, +25], midpoint +5; Session [-40, 0], midpoint −20 | Inconclusive; no resolved peer saving and no syscall counter proving fewer waits |

Release-overlap's latency medians increased by 39 and 46 µs in the adjacent
pairs, with counted completion gaps 8335–8348 µs and no missed interval. These
are submit-to-vblank-completion spans, not a common end-to-end latency metric.
The c2 Session CPU dip is unexplained; an unchanged binary does not exclude an
indirect scheduling effect. All four postflights and cleanup receipts passed;
120 input hashes, CPU bounds and 2120 kernel joins per arm were checked.

There is **no isolated Session destination-copy cost** in this evidence. Claude's
10–20 µs memcpy-speed estimate, repeated by Muse, was withdrawn by both as a
measurement. Peer `ring_upload`
does not price that Session copy. The proposed 1–2 µs per liveness check and
allocation-churn explanation are also unmeasured hypotheses. Flat self-time
does not establish irreducibility. Do not add inclusive profile scopes together.

## Contracts that every candidate must retain

- The client cannot change bytes after immutable acceptance. Reusing storage
  does not transfer authority or permit premature reuse of a leased version.
- Admission reserves the required bounded credits. Failure, cancellation,
  supersession, expiry and revocation settle their obligations exactly once.
- Keep exact resource generation, grant, output and candidate identities.
  A stale completion or replay cannot affect a replacement grant.
- Candidate outcome ordering and release-after-last-consumer remain intact.
  Retiring storage stays charged while a renderer or capture consumer holds it.
- Preserve the existing fresh authority checks at admission and image-service
  boundaries. A cached positive result or a wider observation window is a
  separate authority change, not an incidental optimization.
- No busy loop, renewed deadline on each wake, lost wake, starvation or skipped
  failure cleanup. Preserve the complete readiness set and retained input.

These follow the [one-core authority decision](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md),
the [shell file contract](../../sophia-shell-files.md),
[image-channel contract](../../sophia-image-channel.md) and
[launch authentication contract](../../factotum-launch-authentication.md).
This plan changes none of them.

## Investigation and candidate order

### 1. Reduce admission/control exchanges: implement the reviewed W2 design

W2 removes one explicit `ResourceRetire` exchange per successor, not a proven
number of socket calls or owner wakes. For a successful 2120-frame run there
are 2119 predecessors: the original path sends 2119 retire records; W2 sends
zero. Both receive 2119 predecessor releases. The final displayed resource
remains owned until disconnect. No permit is made implicit by this proposal.

The draft follows the existing runner request/plan/CLI plumbing: an off-by-default
build feature, an explicit host flag, an owner-local permit for the authenticated
bar attempt, and a paired peer key. Reject policy/key mismatches before launch;
initially refuse non-files, non-owned-ring and untested DMA paths. A CLI flag is
an experimental owner setting, not a new privilege boundary or proof that only
the runner can enable it. Pin the complete host, SDK, peer and runner closure.

The reviewed design closes these requirements; preserve them in implementation:

1. The current schema-7 journal has no retire/released events. Use aggregate
   end-event counters, not thousands of new per-frame lines. A versioned additive
   extension (`retire_counter_schema=1`) records `explicit_retires_sent` and
   `predecessor_releases_received`: explicit mode (2119, 2119), W2 (0, 2119).
   Count successful logical sends and exactly validated releases, not expected
   frame totals. This is not a syscall counter. New qualification requires and
   validates the extension in both W2-build modes; old readers ignoring it are
   not evidence. The original control has no such fields. Preserve old packages
   and per-frame/prepare lines; acknowledge the new end line is not byte-identical.
2. Use a peer SDK revision containing the reviewed r12 owned-ring mapping and
   exact welcome checks (`1951d597` or a reviewed successor). Peer `752ef08b`
   pointed at `8b1f7042`, which was insufficient for live r12 owned-ring;
   `b12b0a1c` now pins `1951d597`.
3. Distinguish a side-effect screen (original artifacts versus W2 artifacts with
   policy off) from the W2 effect (identical W2 artifacts, paired policy/key off
   versus on). An interval containing zero does not prove equivalence. Declare
   an acceptable regression/equivalence margin before launch; insufficient
   precision is inconclusive, and a real improvement is not a failure.
4. Specify instrumentation **none** for the CPU comparison, not merely “no
   profiler.” Work-stat diagnostics remain separately labelled. Require the
   exact negotiated revision/capabilities and attempt identity; the extended one-time
   host receipt must have tested parsing and rejection cases.

Implement only the approved wiring in isolated trees,
test feature-off/flag-absent defaults, role/source/key refusals, exact negotiation
and missing/malformed counters. Then review provenance and package one candidate
with its own ABBA. No native package is authorized by this note.

### 2. Avoid unnecessary whole-owner passes: finish S1's proof before wiring

S1 is an event-file-read-only neutral-prefix probe, not the earlier broad
`turn_connected` shortcut. ACK, flush, submit, open and other requests remain
normal-pass work. Existing scaffolding stops before consuming a nonpermitted
request and resumes it once in the normal path.

Before live wiring, close the remaining authentic-custody integration and
quietness fixtures, complete the Session gate wiring review, and prove the
send-before-notify case and keyed failure retention. Exit on mixed/error
readiness, authority work, ring, disconnect, nonquiet state or burst limit.
Keep the original finite deadline and never clear the wake in the probe.

Predict converted event-read wakes, full passes per completed interval and
unchanged outcomes. Measure conversion first. If conversion is rare, stop; the
whole wire-started body is not a defensible savings estimate. Use a separate
candidate and ABBA, without W2, to identify its effect.

### 3. Separate immutable-byte copying from resource lifecycle work

First reuse existing evidence; if it cannot distinguish the costs, propose a
small opt-in diagnostic split at actual worker stages: immutable capture/copy,
admission/reservation, completion settlement and retirement. Name the thread and
window for every clock and reconcile the measured stage totals with their own
denominator. Peer upload time cannot stand in for Session copy time.

Only then select a change: bounded allocation/storage reuse if allocation or
retirement dominates; a copy-path change if destination-copy CPU is material.
Keep the current two-slot workload and lifetime contract. Never call a writable
producer slot immutable merely because it is reused predictably. Test partial
updates, stale pixels, wrong generation, cancellation, size/layout changes,
late consumer release and device-loss failure where applicable.

The existing single-GPU-submit peer prototype targets render/readback separately.
It is not mixed into W2, S1 or a custody candidate, and its offscreen correctness
and queue-count evidence does not establish a native saving.

### 4. Price repeated authority checks before proposing a change

Count `export.poll` and liveness checks by call site, operation and owner turn;
distinguish required boundaries from repeated checks with the same inputs.
Measure their CPU contribution as well as counts. A low count does not prove a
small cost, and a high count does not prove redundant authority work.

Any diagnostic uses bounded aggregation and an opt-in default that reads no
extra clocks when disabled. Its clock/read counts and perturbation are recorded;
no per-operation log flood. If a new check-sharing design emerges, specify which
revocation observation can arrive between checks and how it is handled before
requesting authority review. Existing negative custody-dispatch results remain
part of that decision. Do not revive the same experiment without new evidence.

## Measurement and stop gates

Use existing captures to inventory logical records per turn and turns per
completed interval before adding instrumentation. Classify record kinds and
ordering constraints: multiple records per turn alone do not rule out scheduling
improvements. Earlier batching results and EBUSY-until-covering-ACK semantics
must be accounted for before selecting another batching candidate.

For each approved experiment:

1. Freeze the hypothesis, predicted counter, exact source/dependency/build
   closure, workload, CPU window and latency/cadence/cleanup limits before runs.
2. Prove the changed contract path device-free with focused failure mutations.
   Resolve relevant failing tests or document why they are outside the gate;
   do not present the unresolved broad session suite as passing.
3. If new diagnostic instrumentation is needed, propose it separately on the
   pinned baseline. Diagnostic CPU is not silently compared with uninstrumented
   CPU. A counter should identify the mechanism, not count wrapper calls alone.
4. Root reviews and seals a fresh no-overwrite package; Mason launches through
   the existing SSH/PAM/VT handoff. Old attempts and failures stay intact.
5. Use the same changing strip, two slots, 2120 frames and kernel completion
   anchors 120→2120 (2000 intervals), DP-1 at actual 119.88 Hz, VRR off. Preserve
   all arms of each isolated ABBA, process identity, joins and cleanup receipts.
6. Promote only with the declared CPU benefit resolved by the accounting bounds,
   the predicted mechanism demonstrated, and correctness/latency/cadence gates
   met. One screen is not a confidence interval. Require repeatability before
   generalizing; if bounds or counter are inconclusive, retain the result and
   stop or propose a specific next discriminator. Do not rerun until a desired
   answer appears, bundle candidates, or claim shifted work as a saving.

## Connections and exact evidence

The source baseline for the continuous Sophia diagnostics is host `d9116ff0`
with integration/peer `748c325`, unless a package explicitly pins another pair.
All local evidence below is under `~/.local/state/t331/` on crunch:

- `desktop-cpu-04/report/{report.md,report.json}`: uninstrumented matched rows,
  bound-process and pixel-producer limits; XLibre COPY-at-vblank qualification.
- `continuous-pass-stage-cost-01/analysis/sophia{1,2}.json`: diagnostic owner
  pass and peer-stage costs; the windows and threads differ.
- `continuous-ack-coalesce-02/analysis/summary.json`: inconclusive A1 ABBA.
- `continuous-release-overlap-01/{result.json,analysis/summary.json}` and its
  per-arm postflight/input hashes: complete, inconclusive, never rerun.
- `continuous-frame-cpu-02-recovery/analysis/`: profiled attribution only;
  the original failed capture remains failed.

Branch-qualified investigations are not claimed merged into this notebook:

- `~/dev/sophia-cpu-w2`, `cpu/w2-prototype`,
  `docs/notes/investigations/qyktai1j-w2-retire-the-predecessor-on-presentation.md`:
  lifecycle proof `7cc36ebde`; host wiring `88c1cda50`, runner qualification pending.
- `~/dev/sophia-cpu-turn-proof`, `cpu-turn-proof`,
  `docs/notes/investigations/4cm5nuda-turn-only-owner-pass-for-wire-wakes.md`:
  S1 boundary, default-off wiring and exact fixture/qualification limits.
- `~/dev/sophia-cpu-wake-design`, `cpu/wake-design`,
  `docs/notes/investigations/516tpvf5-wire-wakes-per-bar-frame.md`:
  W1 ordering proof and shelving decision.
- `~/.local/state/w2-prototype/lifecycle-receipts/{run3,f1-m4}/` and
  `owner-wait-m4-receipts/`: durable focused baseline/mutation receipts.

Related existing questions are [bounded independent upload concurrency](../investigations/gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md)
and [prior 9P transport cost evidence](../investigations/yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md).
Neither is an authorization to change protocol semantics. This note records
the October 28 synthesis, implementation checkpoints and remaining gates;
it does not claim a measured improvement.
