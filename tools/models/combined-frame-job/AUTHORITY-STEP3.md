# Step 3 bounded authority review: owner integration of the combined frame job

Draft for review by Codex, then niltempus. It is not an authority pass, and it
selects no source. It answers the five obligations Codex attached to its
4c-3(a) acceptance (BRIDGE.md, "4c-3 (a) accepted as an inert scaffold";
review seal 8c85cec1), against n7fj3c2p gates 1 to 4, TRANSFER.md revision 5
and FALLBACK.md. Code anchors are function names at head 53ce24697, with line
numbers where they help. Paths are under crates/.

This review was written with Codex's acceptance in hand. The facts it relies on
were gathered read-only from the source and are restated here so a reviewer can
check each one. Nothing was built or run.

## What the source shows that the contract did not

The provisional plan cannot be today's frame type. The renderer worker
renders a `PendingRenderedFrame` (sophia-backend-live
scanout/rendered_scanout/exporter/discovery.rs:28). For shell content, the
frame is `Mixed(LiveOwnedMixedCompositionFrame, ..)` (sophia-renderer-live
native_scanout.rs:273). Lowering takes each content image's pixel storage
out of its live store lease with `LiveCpuPixelStorage::try_from(lease)`
(renderer-live head_composition.rs:290-366, pixel_storage.rs:37-47). The
display list that feeds it is built by the visual runtime
(`retained_output_head_composition_frames`, compositor_graphics.rs:561) from
`shell_content`, and `shell_content` is written only by
`set_shell_component_content_on_target`, which needs those leases
(shell_content.rs:52-68). At service 1 the pixels are not captured yet. A
plan that renders without them needs a layer that names a validated image
which does not exist yet. That layer is bound on the renderer thread
(TRANSFER.md "Plan transport"), and composing it needs a provisional
`shell_content` entry that is never installed. Both are new contracts. They
touch sophia-renderer-live (a placeholder layer), the visual runtime
(composition without installation) and the backend worker (binding at
dequeue).

Submission has no entry point for an already-rendered lease. The only
constructor of `NativeGbmRendererWorkerScanoutLease` is
`NativeGbmRendererWorker::poll` (worker.rs:729). Its fields are private.
Its only consumer is `export_from_worker` (exporter/discovery/worker_export.rs:21),
reached from `export_rendered_scanout_buffer`, which `prepare.rs:241`
calls on the tick path. The native scanout's head state machine also has to
see the frame: `pending_content`, `rendering_content`, `submitted_content`
and `presented_content`, the `deferred_mirror_generations` queue, native
frame ids from `next_frame_id`, and retirement protection
(`output_retirement_protected`, renderer_images.rs:471-488). The combined
result must enter at the point where `poll()` would have yielded
`Exported`, for the same head, frame id and correlation. No current method
does that.

Four related facts correct standing text:

- Response room, not journal reservations. The owned-ring frame check is
  `response_room >= 4` control records: Prepared, terminal, resource status
  and the eventual release (epoch_registry/owned_ring/frame.rs:61,
  commit.rs:79). The file journal itself holds two reservations, status and
  release (files/export/owned_ring/offer.rs:54-61). TRANSFER.md's "four
  journal obligations" should read "four reserved response records, two of
  them journal reservations".
- The shell image owner's validator is not the combined one.
  `ShellIsbImageOwner` holds `IsbValidationWorker` with the default plan
  type (shell_transport/image_owner.rs:20,32), one owner per connection
  (sophia-session shell_component_connections.rs:80). The combined path
  needs `IsbValidationWorker<CombinedRenderPlan>`, whose plan type is a
  backend type that sophia-runtime cannot name. The owner, or its owned-copy
  part, must become generic over the plan with the session choosing the
  type, or the plan stays opaque behind a runtime-defined box. TRANSFER.md
  already forbids `Box<dyn Any>`.
- `submit_bundle` discards the runtime's identical-frame `Ok(false)` and
  still emits Prepared and a pending presentation (sophia-session
  metadata_shell/content/submission.rs:31-59). It has no rollback after the
  runtime install. The setter itself rolls back `shell_content` and the
  retirement claim if `queue_retained_projection` fails (shell_content.rs:128-146).
- The conventional path compares facts, interaction and candidate
  generations (sophia-runtime shell_content/candidates.rs:785-793, 643-646)
  and the stored `content_layout_generation`. No scene or composition
  generation exists there. The backend has `scene_generation` (the maximum
  committed generation; compositor_graphics.rs:516-520), `target_generation`
  per head, and `logical_content_checksum` per head frame
  (renderer_images.rs:7-14).

## Obligation 1: one owner row with exact identity

The row is `CombinedPending`, owned by the session beside the connection's
image owner. It binds the runtime half and the backend half and is the only
place either half is settled.

The runtime half holds the connection and content grant epochs from the
owned-ring `Identity` (owned_ring_protocol.rs:18-33), the
`FrameCopyBinding` (candidates/credits/copy.rs:5-8), the worker job, the
`CombinedReservation`, the deadline and the four reserved responses.

The backend half needs an identity that still finds the issuing facade after
disable, mirror, remap, replacement, revocation or disconnect. None of
`OutputId`, the head vector index or `LiveRendererWorkerOutputKey` does that
alone. The key is `(group << 32) | head` (renderer_initialization.rs:69-72),
so it repeats when a worker re-attaches. The facade's real identity is the
triple of its core's `image_store` (from the `NEXT_IMAGE_STORE` counter,
worker.rs:231), its output key, and the `Arc<OutputClaim>` made at each
`attach` (worker.rs:380). Topology installation never drops, replaces or
reorders exporters, and never changes `head.head` or `head.group`
(topology/installation.rs:147-378). Shell capture already finds its exporter
by `image_store_identity()` (persistent_native_scanout/shell_capture.rs:197).
The row therefore records:

    CombinedHead {
        native_owner: NativeFrameOwner,   // process-unique, native_identity.rs:10-25
        head: RenderHeadId,               // located by head_index_for_head
        image_store: u64,                 // the issuing facade's core
        claim: Arc<OutputClaim>,          // pointer identity of the issuing facade
        target_generation: u64,           // at admission
        output: OutputId,                 // for authority checks only, never routing
    }

Collection, cancellation and settlement find the exporter by `image_store`
and `head`, then require `Arc::ptr_eq` on the facade's claim. A missing
exporter, a different `NativeFrameOwner` or a different claim is a
classified state, never "no renderer". The 4c-3 forwarders change to this
lookup (section "Backend correction before any caller").

Admission reserves both halves or neither. The order is: validator
`reserve_combined`, facade `reserve_combined_frame`, response room. A
failure rolls back in reverse with the existing calls (`release_reserved`,
`cancel_combined_frame`, `cancel_owned_offer`). A result is accepted only if
its `CombinedToken` matches the row's reservation (`Arc::ptr_eq`,
combined.rs:287) and its correlation matches the row's job. Unknown,
duplicate or stale results change no row.

A retention refusal is never discarded. `settle_combined_frame` must return
the unretained `WorkerResult` intact. A result for a different facade must
not cancel that facade's cell: the 4c-3 code does both, and both change.

Teardown keeps requested detach apart from proof. Facade Drop detaches the
claim and sends Deregister without acknowledgement (worker.rs:799-811). The
service drops the output state when the claim is detached
(service.rs:98,155), so its buffers return with it. That proves the worker
gave up the output state, not that a device operation finished. A row whose
facade was dropped may settle its credit only with TRANSFER.md's qualified
proof: `CompletedNativeOwner` for the matching `NativeFrameOwner`, after
`poll_shutdown`, disposition and owner destruction. Until then it keeps its
charges and blocks replacement over unresolved rows, as RetirementFailure
already does.

## Obligation 2: provisional geometry and resource binding

`project_render_bundle` (sophia-session metadata_shell/content/projection.rs:7)
mixes metadata checks and lease reads inside one loop. The lease's
description supplies width and height before the bounds check (:37-45).
The split is:

- `preflight_render_plan(bundle_metadata, declared, output, allocations)`
  runs every metadata check in today's order: output identity, candidate
  generation, output size, surface and allocation existence, same output,
  placement bounds with checked add, node uniqueness, visible placements,
  targets with `scale_generation`, popouts. It uses the declared width,
  height and stride from the offer's `OwnedCopyImageRequest`. It returns a
  `ProvisionalContentFrame` that holds no lease and cannot convert into
  `LiveShellContentFrame`.
- `bind_validated(provisional, lease)` runs today's lease checks against
  the validated description and rejects any mismatch as Malformed. It
  happens once at owner consumption. The renderer runs the same size,
  stride and extent equality at dequeue for its own placeholder (TRANSFER.md)
  and reports a mismatch as a classified failure, never as Malformed.

A provisional composition is the visual runtime's existing display list for
that output, built as if the provisional content were installed. It cannot
write `shell_content`, retirement claims or `retained_projection_pending`.
The proposed operation is
`compose_provisional_head_frame(output, provisional, scene) -> CombinedRenderPlan`
in compositor_graphics, using `retained_composition_source_set`,
`display_list_for_output` and `compose_native_head_frames_from_sources`.
The provisional image becomes a new `LiveOwnedMixedCompositionLayer`
variant in sophia-renderer-live. It carries the declared description and a
binding slot and nothing else. Every other layer keeps its existing lease or
storage. Those are the "already-validated predecessor CPU content leases"
TRANSFER.md permits in the plan. The plan also records the
`scene_generation`, the head's `target_generation`, the
`content_layout_generation` and the transform it was composed against.

The authority rechecks at consumption are the setter's checks in its order
(shell_content.rs:53-117): grant per layer, interaction revocation,
quiescence and frame service, foreign retirement claims, viewport and layout
generation. Before them come allocation rows and scale generation, output
extent, current grant and epoch, and native availability. Negative cases:
declared and validated size differ; stride overflow; extent overflow; a
placement out of bounds after an allocation shrinks; grant lost between
admission and result; interaction revoked; foreign retirement claim
appears; native suspended. Each is a refusal or withdrawal on today's rules,
never a fallback.

## Obligation 3: owner-only installation of the rendered result

Successful installation is a lease transfer, separate from the
discard-and-Release settlement 4c-3 implements. It has two halves, done in
one owner turn and rolled back together on refusal.

The visual runtime half is `install_rendered_shell_content(frame, layer,
plan_identity)`, beside `set_shell_component_content_on_target`. It runs
the same checks, writes `shell_content` and the retirement claim the same
way, and does not call `queue_retained_projection`. It binds the retirement
claim to the combined frame's native frame id, which `queue_retained_projection`
would otherwise do through `bind_shell_retirement_claims`. Its rollback is
the setter's. Identical frames: if the frame equals the installed one, the
owner settles the result as Expired-equivalent (retain and Release the
lease) and returns `Ok(false)`. The session caller must then skip Prepared
and pending presentation. This is the honest form of today's discarded
`Ok(false)`, and it changes nothing on the conventional path.

The native scanout half is `accept_combined_render(head: CombinedHead,
result)`. It finds the facade by identity, checks that the head's
`target_generation` and `NativeFrameOwner` still match, and checks that the
head has no pending, rendering or protected-retirement content
(renderer_images.rs:280, 471-488). It then stores the exported result so the
next `export_rendered_scanout_buffer` for that head returns it, exactly as
`poll()` returning `Exported` would. It records the head's
`rendering_content` with the frame id allocated at admission. The existing
tracked path (`prepare.rs:201`, `tracked_submit.rs:10`,
`custody.accept_submission`) then submits it unchanged. Uncertain
submission, page-flip retirement and device detach stay where they are.
Prepared is emitted only after both halves succeed. Presentation follows
the exact tracked completion.

Multi-output effects: admission is single-output and single-head, so one
result touches one head. An output that has become mirrored or remapped
since admission fails the native-half check, and its result settles by
Release. That is a classified failure, never a silent drop.

## Obligation 4: bounded accounting and failure ownership

Charges that stay on the row until its settlement:

- the validator credit, held through rendering and cleared only by
  `settle_combined_token`
- the backing headroom (`owned_offer_available`, files/export/owned_ring.rs:109-136)
- the source slot, busy until the read ends and its release is journaled
- the private frame binding
- the four reserved responses
- the facade's Release cell

Rwrite keeps its custody meaning: a lean commit is settled by its Rwrite
alone (files/export/transaction.rs:176). The source release is published
by today's `publish_releases` at service 2, after consumption or exact
failure, never from the validator.

| Failure | Where it is seen | Who owns the charges | How it settles |
| --- | --- | --- | --- |
| Validation fails before forwarding | `take_completed` with `owner_plan` | row | ordinary completion; plan dropped on owner |
| Inbox refuses the move | same | row | same; `cancel_combined_frame` |
| Forwarded, flag not yet set | none | row | wait for the result or exit; no release |
| Renderer expires the job | `CombinedOutcome::Expired` | row | settle; cell Free; source release published |
| Render fails | `Rendered(Failed)` | row | settle; cell Free |
| Exported, owner refuses | `Rendered(Exported)` | row, then cell | retain, settle; Release on next poll |
| Validator thread dies | NotifyOnExit, BrokenPipe | row | Io as today; credit kept |
| Renderer route disconnects | `NotConnected` | row | kept until `CompletedNativeOwner` proof |
| Facade dropped (topology, revocation) | identity lookup fails | row | kept until qualified proof |
| Journal full at publication | `EAGAIN` | row's release | retried next turn, as today |
| Grant lost or deadline passes | `withdraw_epoch`, `expire` | row | cancel flag only; result still settles |

A successor may be admitted while its predecessor's lease is still retained
in the cell only if the cell is free. The cell is per facade, so the
successor is refused and takes the conventional path. That refusal is
counted.

## Obligation 5: scene fallback and progress

The scene comparison is the plan's recorded `scene_generation`,
`target_generation`, `content_layout_generation` and transform against the
current values at consumption, plus a recomputed `logical_content_checksum`
for the output's other layers. `scene_generation` alone is a maximum over
committed generations and could miss a lower-generation change. Any
difference is scene churn. Churn takes FALLBACK.md's one-way,
once-per-candidate conventional path: retain the speculative lease, clear
the credit, latch the candidate, and let `begin_submission_inner` and the
existing content service submit it. A change in authority or geometry is
never treated as churn.

Prototype checks name their negative controls before validation:

- full and disconnected Release
- an occupied three-slot pool
- ordinary and combined contention for one output
- deadline and cancel at each phase
- capture on and off
- disable, mirror and remap during each phase
- no second validation or render on the stable success path

The mechanism capture counts fallback work, retained age and added
notifications. Three services and any CPU saving stay claims for the
later ABBA.

## Eligibility for the first prototype

Admission takes the combined path only when all of these hold; otherwise
the frame stays on today's path, chosen before any reservation:

- one enabled head serves the output, and it is not mirrored
- the head's worker is attached, idle and not quarantined
- the combined cell is free
- the content is CPU storage, not dmabuf or snapshot
- capture is not active on the head
- the frame comes from the owned-ring lean lane

## Backend correction before any caller (4c-4)

The 4c-3 forwarders change before a production caller exists:

1. Key by `CombinedHead`, not `OutputId`. Find the exporter by
   `image_store` and head, and require claim pointer equality.
2. Return refused results instead of dropping them:
   `Result<ValidatedIsbPixels, CombinedSettleRefused>` gains the unretained
   `WorkerResult`.
3. Make "renderer not found" a distinct error, not a settlement.
4. Cancel only the facade that issued the reservation.

Tests cover remap and disable between reserve and settle, using the existing
fake-worker fixture. These are backend-only, behind the feature.

## Stopping conditions

Stop and record the blocking obligation if any of these turns out to be
true:

- the placeholder layer cannot be bound on the renderer thread without
  owner-side pixel work
- provisional composition needs to write `shell_content`
- `accept_combined_render` cannot keep the head state machine's retirement
  protection
- the validator's plan type cannot stay opaque in sophia-runtime without
  `Box<dyn Any>`

Per n7fj3c2p, none of these may be worked around with worker-side DRM,
cached liveness or a new ring.

## Decisions for niltempus

1. Scope: step 3 touches four crates. sophia-renderer-live gets a
   placeholder layer. The backend visual runtime gets provisional
   composition and `install_rendered_shell_content`. The backend native
   scanout gets `accept_combined_render`. The session and runtime get the
   owner row and the generic plan. Accept this scope, or stop at the
   scaffold.
2. Eligibility: the narrow first set above.
3. Order:
   - 4c-4 backend correction
   - gate-3 prototype on the test-only seams: the renderer-live placeholder
     layer, provisional composition and accept with a deterministic renderer
   - the authority pass
   - gate 4, default-off real integration
