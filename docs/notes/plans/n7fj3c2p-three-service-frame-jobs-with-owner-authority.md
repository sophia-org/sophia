---
id: n7fj3c2p
date: 2026-11-10
kind: plan
status: active
tags: [plan, performance, custody, sft, portability]
---
# Three-service frame jobs with owner authority

## Selection and boundary

Niltempus selected implementation of the integrated seven-part plan on November
10. First freeze the contract and obligation map, then model and review them,
then build a device-free prototype. Production changes require the bounded
authority review to pass. No native launch, SDK publication, existing contract
amendment in force, or default change follows from this research selection.

One output, one outstanding CPU-produced frame, existing two-slot source storage,
unchanged cadence and no deliberate batching delay. Three services means three
frame-dependent owner actions: dispatch, settle/submit, completion/publication.
It is not a promise about all loop iterations or all waits in a busy desktop.

## Seven design decisions

1. Keep scoped 9P setup, authentication, grants, registration and recovery. Resolve
   names at setup; no URL parser or routing broker. Host-held connection and epoch
   bind every request; peer words select resources but never authorize access.
2. Keep FrameCommit and existing journal/ACK semantics initially. Exact Rwrite
   transfers command custody; it does NOT certify pixel validity or presentation.
   Reserve every promised response before effects, including ACK application.
   The worker dispatch additionally needs bounded source, private destination,
   candidate, renderer-job and result capacity. No new unbounded queue.
3. Replace validation-return/owner-install/render-dispatch with one private
   capture -> validate -> render job. The owner may authorize bounded speculative
   work against a frozen scene description. The worker cannot install content,
   decide current grant authority, write outcomes or submit DRM. Only the owner
   can accept a result and submit it after current authority/output/scene checks.
4. Preserve private captured pixels, validation and all lifetime charges. Source
   completion is a fact, not a release notification. Return it with the combined
   result; publish source reuse at the existing safe journal boundary. Keep the
   validated backing for repaint independently of scanout-buffer lifetime.
5. Keep 9P as the carrier and reuse bounded worker queues/wakes. No shared control
   mapping, futex helper, spinner or new carrier. The closed carrier experiment
   supplies no reason to reopen transport work as part of this treatment.
6. Preserve distinct custody, validation, presentation, source and final-release
   facts; covering ACK and idle ACK; unknown outcomes and replacement-epoch
   refusal. Retain existing tracked owner-side ioctl, exact completion, uncertain
   submission and device-detach handling. No release inferred from cancellation.
7. Keep all readiness classes, caller-fresh B6 checks and unrelated per-pass
   obligations. Linux first; describe required capture/transfer/lifetime/display
   properties for future per-OS adapters, without a new generic platform layer.

## Selected worker placement, November 10

Niltempus explicitly approved keeping the separate validation worker and handing
its validated result directly to the renderer, bypassing the intermediate owner
visit. CPU per completed frame and security are the primary acceptance criteria;
validation isolation and responsiveness remain constraints. This supersedes the
initial same-worker sketch: one logical job now spans two existing threads.
The direct transfer protocol is specified in
`tools/models/combined-frame-job/TRANSFER.md`. It is a proposed internal contract,
not a production API or authority-review pass. Niltempus subsequently approved
one conventional fallback for scene-only churn with valid authority and geometry.
See FALLBACK.md for its owner checks, rollback, retained Release obligation and
accounting. No new refusal reason or physical run is selected.

## Proposed internal ownership interface

FrameReservation remains owner-local: exact connection/grant/output/candidate,
allocation and scene versions, deadline, budget charges, journal obligations and
retained predecessor. It is not transferred to the worker as mutable authority.
FrameJob is a non-cloneable source-read ticket plus immutable placement/scene
metadata, retained dependencies, work limits and cancellation signal.
PreparedFrame carries exact job/output/generation identity, validated backing,
source-read-finished fact, rendered lease or classified failure. Cancellation
signaling may stop work; it cannot grant permission or prove resource quiescence.

Service 1 validates metadata and builds a provisional composition plan, reserves
work, then dispatches. It MUST NOT manufacture an admitted resource lease for
unvalidated pixels. Service 2 validates the returned identities and current
state, settles resources and candidate state, and uses the existing tracked DRM
submission. Service 3 observes exact completion and publishes terminal/release
facts. Authority or validated-description failure retains original charges until
each last-use proof arrives. Scene-only churn uses the approved one-shot
conventional fallback with the same validated backing; it never re-enters the
combined speculative path for that attempt. No cross-epoch fallback is allowed. Unsupported cases choose the existing path before admission.

## Source obligation map (base 22d8cbca7)

Paths below are under crates/. Function names, rather than drifting line numbers,
are the primary anchors; source-pins.json in the modeling directory binds bytes.

| Current seam | Obligation | Proposed placement / unresolved proof |
| --- | --- | --- |
| runtime shell_transport/owned_copy/frame.rs propose_frame_commit | current grant/ring identity, source and response reservation | service 1; preserve custody and write-response meaning |
| runtime shell_content/epoch_registry/owned_ring/frame.rs reserve_owned_frame_copy | frame/copy/resource accounting joined before consume | service 1; add renderer/result reservation without weakening rollback |
| runtime shell_content/owned_ring.rs OwnedRingRead::copy | noncloneable retained source, generation checks, one private payload read | validation worker; same failure rules, no client mapping |
| runtime shell_content/isb_worker.rs | bounded validation, deadlines/cancel, result on worker exit | validation worker; direct transfer on success, preserve failure notification |
| runtime shell_transport/image_owner.rs service | current grant reconciliation, withdrawn jobs, settlements, source publication | service 2 or withdrawal handling; no worker authority |
| session live_session/metadata_shell/content/projection.rs project_render_bundle | placement bounds and resource lease retention | split metadata preflight at service 1 from validated-resource binding at service 2; new provisional plan must not impersonate ContentRenderBundle |
| session live_session/metadata_shell/content/submission.rs submit_bundle | projection, runtime install, Prepared, pending presentation | service 2; must avoid triggering a second render through the current installation path |
| backend-live production_visual_runtime/compositor_graphics/shell_content.rs set_shell_content_on_target | grant/layer ownership, quiescence, retirement, scene transform; queue retained projection | metadata checks at service 1 and recheck service 2; separate install of an already-rendered result requires proof, not an existing API |
| backend-live scanout/rendered_scanout/exporter/worker/service.rs render_frame | actual frame-slot acquisition, private rendering, failure invalidation | renderer worker; slot acquisition remains here, not at owner dispatch |
| backend-live scanout/rendered_scanout/exporter/worker.rs | correlation, worker lifetime, result wake | reuse, one completion notification for combined success; no transfer of DRM owner |

Two concrete implementation gaps: current projection requires a validated
resource lease, and content installation queues rendering. Therefore simply
connecting the two workers cannot implement this design. A provisional plan and
an owner-only install-already-rendered operation would be new internal contracts.
The actual renderer slot is worker-owned; the reservation design must distinguish
owner job/result capacity from actual slot acquisition and handle Deferred without
silently creating an extra success-path owner exchange.

## Gates and stopping rules

1. Freeze this map, a concurrency modeling brief and source pins. Authority review
   must challenge provisional geometry, resource binding, Deferred, rollback,
   stale scene, and post-revocation private work before runtime changes.
2. Check finite ownership/progress projections and negative controls. Two successive
   frames must coexist with a retained predecessor. A model of the proposed design
   is not a refinement proof of existing Rust. Explicitly identify omitted ioctl,
   memory ordering, journal and platform details; no fabricated implementation traces.
3. After review, a test-only prototype uses real capture/validation/journal seams
   and a deterministic renderer. Preserve full-journal, disconnect and stale result
   behavior; prove no intermediate validation return on the normal path.
4. Only after those gates, isolate a default-off real-worker integration and run
   feature-off/on checks and the established device-hidden full gate. Do not pick
   a numeric capability bit or change the public SDK without the wire-contract gate.
5. First mechanism capture proves actual correlated service counts. A later matched
   physical comparison holds host/peer inputs except this treatment and measures
   combined CPU/frame, latency, cadence and memory. Niltempus launches. No automatic
   repeat, source adoption, three-wake claim or two-service substitution.

If provisional composition cannot preserve authority, lifetime or responsiveness,
record the exact blocking obligation and stop. Do not fall back to worker-side DRM,
owner-side pixel work, cached liveness, speculative unbounded memory or a new ring.

## Connections

- [Current CPU plan](4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
- [Four-wake obligations](../investigations/edc0te2v-which-action-each-of-the-four-lean-owner-wakes-owes-and-which-transition-a-consolidation-would-remove.md)
- [Portable mechanisms](../concepts/fp9o6pdb-portable-plan-9-interfaces-with-native-frame-mechanisms.md)

No Rust source, native run or performance evidence belongs to this contract draft.

## Authority challenge: prototype gate held

Review `sophia-framejob-authority-review-claude-01` (seal `2e0c759f`, report
`2d1bea35`) finds the direction coherent enough to model after the following
requirements and decisions; it is NOT authority approval. Model ee4b95725 is
only the preliminary ownership projection (evidence `4c811ba9`).

Required contract refinements, accepted for the next revision:

- Plan geometry is untrusted declared metadata, never a fabricated validated lease.
  Validated width/height/stride/extent must equal the planned description; mismatch
  is Malformed, with charges retained until proper settlement. Recheck allocation
  rows/scale generation, output size, viewport/content-layout generation, foreign
  retirement claims, interaction revocation and native service availability.
- PreparedFrame must carry composition records and transform. Owner installation
  must atomically update content entry, retirement claim, display list and input
  projection with rollback. Specify the identical-frame Ok(false) handling; do not
  claim the existing content install already accepts rendered results.
- Preserve service-1 Rwrite and its command-custody meaning; retain the existing
  four reserved journal responses. Prepared is owner-produced after the service-2
  checks, never implied by a worker completion.
- Pin successor gating, validation credit held through render, exact pending-table
  ownership and worker-death transfers. Withdrawn render leases use the existing
  Release command; backing follows retired-epoch reconciliation. Quarantine and
  disconnect prove neither last use nor safe release.
- Renderer has three slots per output and acquires them on its thread. Deferred
  is a real busy-output case. Specify bounded retry or an explicit non-success
  result retaining backing; do not assert it impossible. Gate dispatch against
  current in-flight/stalled/quarantined state.

Review history (thread placement is now selected above; scene choice remains):

1. Execution placement: resolved by niltempus's approval of separate validation
   and renderer threads with direct transfer. TRANSFER.md revision 4 records the
   queues, opaque plan, ownership witness, credit proof and cancellation contract.
   These are design obligations, not an implemented worker handoff.
2. Scene churn after dispatch: resolved by niltempus's approval of one conventional
   fallback with still-valid authority and geometry. Keep existing Stale meaning
   (peer identity drift), and count discarded and ordinary render work. Full scene
   version covers every retained composition input, not only shell content.
   Non-shell churn, stalled work, slots and starvation remain proof obligations.

These are prerequisites to the promised full model and prototype, not decisions
an implementer may make inside the runtime diff. No new capability, codec,
production API or native run is selected at this checkpoint. The direct worker
handoff is selected as a design, subject to the remaining proof/prototype gates.

<a id="gate-dispositions-and-default-off-merge-november-19"></a>

## Gate dispositions and default-off merge, November 19

On November 19 niltempus directed a default-off merge of the lane, with no
adoption and no further matched block. Codex accepted that direction and
performs the merge. The merge commit is named here once it exists; this
section does not predict it. The combined frame job and its diagnostic
witness stay opt-in: `experimental-combined-frame-job` and
`diagnostic-combined-witness` are in no default feature list or
`native-session` closure, and the witness does not enable the combined
feature. No installed binary, service command, desktop profile or default
selection changes.

Every acceptance below is scoped to its own evidence. Sealed packages live
under `~/.local/state/sophia-framejob-*`. They are not rewritten; corrections
are carried in their reviews and here.

| Gate | Disposition | Head and evidence |
|---|---|---|
| 1, 2 | map, models and authority review completed before gate 3 | `tools/models/combined-frame-job/` |
| 3 | device-free prototype accepted | `0fd5ac842`; stages in `G35D-LEDGER.md` |
| 4 | default-off integration accepted, scoped, for this merge | C1 `d64a69595`, C2 `e452c0f58`, C2b `cdeb48652`, C3a `2393a7070`, C3b `4349542f4`, binding repair `87c9b98d5`, worker accounting `62fdd0d1e` (validation-01, seal `b33175d8`) |
| 4, D2 | master parity closed QUALIFIED | below |
| 5 | mechanism supported; still and moving blocks complete | fj-mech-05, assessment-04 seal `328f2cde`; fj-02, fj-03 below |

Gate 4 is accepted for a default-off merge, not as production acceptance.

### D2, feature-off parity with master

The final comparison
(`sophia-framejob-gate4-d2-parity-final-01`, seal `25e53df2`) compares the
lane at `62fdd0d1e` (tree `7f633eb8`) with master `d7f77e54d` (tree
`c4d0dee0`). It uses master's D2 baseline-01 (seal `965a8f1b`) and the lane's
worker-accounting validation-01 logs, with the proposal-02 comparator
unchanged. All six feature-off rows are collection complete. No shared test
name is removed, no FAILED outcome or single-result difference is observed,
and all 3,088 shared row entries resolve to source. The lane adds 40 tests,
all in combined-frame-job modules. Two repeated-differ observation sets and
one unknown-master result stay unresolved and are not promoted to equal
outcomes:

- session-ns-nft `protected_descriptor_peer`: lane [ok, ignored, ok, ok],
  master [ok, ignored, ok];
- session-ns-nft `authenticated_wm_child`: lane [ignored, ok], master
  [ignored];
- runtime-nft
  `shell_session_nonblocking_negotiation_exposes_idle_authentication_deadline`:
  lane ok, master has no per-test line.

The four observation limits of the earlier comparison at `2393a7070`
(baseline-01, addendum-01 seal `dafaea1d`) are retained separately. Master's
two known clippy failures, its failed gate on
`only_a_genuinely_stale_group_socket_is_cleared` and the accepted lane
policy difference `a84d6643` for that test also stand. Equal names and
totals do not prove equal test bodies.

The commits from `2393a7070` to `62fdd0d1e` leave ordinary production
builds without either feature unchanged. That is not true of every
feature-off test build: under `cfg(test)` the runtime serial refactor
compiles. It moves the validator serial onto the worker and replaces a
relaxed `fetch_add` with a checked `fetch_update` and an exhaustion
sentinel. The sealed E1 and validation runs exercise that code.

### Native results

Both blocks measure session plus peer CPU on the process clock, inside the
2,000-frame counted window, at `62fdd0d1e`. Control A lacks the combined
frame job; candidate B has it, including its accounting and emission paths.
Bounds are kernel-accounted CPU boundary intervals, not confidence
intervals. Injector, observer, sampler and runner are excluded, so this is
not total-machine CPU. The two blocks ran in separate windows under
different workload-validity rules and are never pooled. Their difference is
not a measured cost of pointer motion.

**fj-01** failed before any arm: the nightly pool sweep had removed the peer
binary (capture `ce6ed50a`).

**fj-02** failed overall at the first moving arm, because the frozen M7
required `physical_pointer_routed` above zero and ma1 recorded zero. That
counter counts accepted routed Deliver requests, not owner-side motion, so
package proposal 05 changed the workload-validity contract for the next
block. Its still block completed and is accepted separately (capture `731f9560`,
package manifest `61561b2b`):

| still | µs/frame |
|---|---|
| control (sa1, sa2) | [646.6646655, 647.09651175] |
| candidate (sb1, sb2) | [591.89153125, 592.29084875] |
| candidate − control | [−55.2049805, −54.37381675] |

The midpoint change is −8.4698%. Pairs: sa1/sb1 −51.910302
[−52.3202525, −51.5003515]; sa2/sb2 −57.66849525 [−58.0897085, −57.247282].
The midpoint timing imbalance is 0.3411695 s. The excluded fj-02 ma1 arm
stays excluded.

**fj-03** ran the four moving arms under M7 as amended by package proposal
05. That M7 requires the session's pointer-event count to lie between the
events offered in the window and all events offered, plus exactly one
`motion_observed` record; routed delivery is reported, not required (capture
`267f30d6`, manifest `70633b01`):

| moving | µs/frame |
|---|---|
| control (ma1, ma2) | [991.47281975, 991.966418] |
| candidate (mb1, mb2) | [949.1324915, 949.66313575] |
| candidate − control | [−42.8339265, −41.809684] |

The midpoint change is −4.2675%, worst case −4.2169%. Pairs: ma1/mb1
−65.960393 [−66.49721, −65.423576]; ma2/mb2 −18.6832175
[−19.170643, −18.195792]. Both favour the candidate, but by very different
amounts, and no pair-agreement gate was frozen, so neither is discarded.
The aggregate does not establish a stable effect size. The two control arms
differ by 36.24660125 µs/frame (ma1 1009.8429195, ma2 973.59631825). The
midpoint timing imbalance is −0.12707575 s. ABBA order alone does not
cancel drift.

Latency is mixed, not uniformly better. Moving, candidate minus control:
submission-to-completion medians +13 and +3 µs, p99 +5 and +7 µs;
peer-offer-to-presented medians −4 and −9 µs, maxima +30 and +5 µs.

Both blocks' frozen decisions are nonregression_pass true,
demonstrated_saving true and production_adoption false.

### Open after the merge

- Adoption is a separate decision. It needs evidence beyond one panel
  workload, one host and one block per pointer condition.
- Native PM1–PM4 hook-order and mutation gaps, the inherited
  killed-not-detected and surviving mutants, and the worker aggregation and
  record-detector limits are carried limits.
- The pointer evidence is whole-session. It does not prove per-event
  consumption, routing or presentation inside the window.
