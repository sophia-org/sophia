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
facts. Failure or stale scene at service 2 refuses/discards with original charges
retained until each last-use proof arrives; no automatic rerender or cross-epoch
fallback. Unsupported cases choose the existing path before admission.

## Source obligation map (base 22d8cbca7)

Paths below are under crates/. Function names, rather than drifting line numbers,
are the primary anchors; source-pins.json in the modeling directory binds bytes.

| Current seam | Obligation | Proposed placement / unresolved proof |
| --- | --- | --- |
| runtime shell_transport/owned_copy/frame.rs propose_frame_commit | current grant/ring identity, source and response reservation | service 1; preserve custody and write-response meaning |
| runtime shell_content/epoch_registry/owned_ring/frame.rs reserve_owned_frame_copy | frame/copy/resource accounting joined before consume | service 1; add renderer/result reservation without weakening rollback |
| runtime shell_content/owned_ring.rs OwnedRingRead::copy | noncloneable retained source, generation checks, one private payload read | combined worker; same failure rules, no client mapping |
| runtime shell_content/isb_worker.rs | bounded validation, deadlines/cancel, result on worker exit | combined worker; preserve validation chunk checks and failure notification |
| runtime shell_transport/image_owner.rs service | current grant reconciliation, withdrawn jobs, settlements, source publication | service 2 or withdrawal handling; no worker authority |
| session live_session/metadata_shell/content/projection.rs project_render_bundle | placement bounds and resource lease retention | split metadata preflight at service 1 from validated-resource binding at service 2; new provisional plan must not impersonate ContentRenderBundle |
| session live_session/metadata_shell/content/submission.rs submit_bundle | projection, runtime install, Prepared, pending presentation | service 2; must avoid triggering a second render through the current installation path |
| backend-live production_visual_runtime/compositor_graphics/shell_content.rs set_shell_content_on_target | grant/layer ownership, quiescence, retirement, scene transform; queue retained projection | metadata checks at service 1 and recheck service 2; separate install of an already-rendered result requires proof, not an existing API |
| backend-live scanout/rendered_scanout/exporter/worker/service.rs render_frame | actual frame-slot acquisition, private rendering, failure invalidation | combined worker; slot acquisition currently happens here, not at owner dispatch |
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
