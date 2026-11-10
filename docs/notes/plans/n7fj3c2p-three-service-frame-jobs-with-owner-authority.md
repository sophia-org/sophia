---
id: n7fj3c2p
date: 2026-11-10
kind: plan
status: proposed
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
not a production API or authority-review pass. Scene-churn disposition remains
an owner decision; no new refusal reason or fallback has been selected.

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
each last-use proof arrives. Scene-only churn is a separate unselected policy:
explicit conventional fallback versus a new host-obsolete refusal. No cross-epoch
fallback is allowed. Unsupported cases choose the existing path before admission.

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
2. Scene churn after dispatch: a new explicit obsolete-scene refusal/retry contract
   versus a clearly reported fallback to the existing owner/render path. Do not
   overload existing Stale (peer identity drift) or hide a second render. Full
   scene version covers every retained composition input, not only shell content.
   Model non-shell churn, stalled work, slots and starvation before selecting.

These are prerequisites to the promised full model and prototype, not decisions
an implementer may make inside the runtime diff. No new capability, codec,
production API or native run is selected at this checkpoint. The direct worker
handoff is selected as a design, subject to the remaining proof/prototype gates.
