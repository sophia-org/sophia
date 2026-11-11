# Step 3 bounded authority review: owner integration of the combined frame job

Revision 4. R1-R5 at the end answer Codex's review of revision 2 (24fe50c62).
Revision 4 changes only the three R2 transitions Codex reviewed at e33c543aa,
and folds the superseded fallback sentences into the R4 baseline. This is a draft for review by Codex, then niltempus. It is not an
authority pass and selects no source. n7fj3c2p requires a design authority
pass before any gate-3 prototype is written; verifying the prototype is a
separate, later acceptance.

Revision 1 (fdc2b1c11) answered the five obligations from Codex's 4c-3(a)
acceptance (seal 8c85cec1). Codex reviewed it and asked for revision (seal
1ac6c496), with five blocking findings, F1 to F5, and two drafting
corrections. This revision answers each one.

Anchors are function names at head fdc2b1c11, with line numbers where they
help. Paths are under crates/. The facts were gathered read-only from the
source; nothing was built or run.

## Standing facts

- **The provisional plan cannot be today's frame type.** Lowering a
  `Mixed` frame takes each content image's storage out of its live lease
  (renderer-live head_composition.rs:313, pixel_storage.rs:37-54).
- **There is no entry point for an already-rendered lease.** The only
  constructor is `NativeGbmRendererWorker::poll` (exporter/worker.rs:729),
  and the only consumer is `export_from_worker`
  (exporter/discovery/worker_export.rs:21).
- **Response room is four reserved records**: Prepared, terminal, resource
  status and release (epoch_registry/owned_ring/frame.rs:61). Two of them
  are journal reservations, status and release
  (files/export/owned_ring/offer.rs:54-61). Codex agreed with this
  correction. TRANSFER.md takes it when this design is accepted.
- **The validator has the default plan type.** `ShellIsbImageOwner` holds
  `IsbValidationWorker` with the default plan (shell_transport/image_owner.rs:20).
  The owner, or its owned-copy part, takes a defaulted generic plan
  parameter that the session sets to the backend's `CombinedRenderPlan`.
  There is no `Box<dyn Any>`.
- **One owner turn runs in this order** (sophia-session
  live_session/owner_loop/physical_input_loop.rs):
  1. component service, which services the image owner first
     (`ShellComponentConnections::turn_connected` →
     `ShellIsbImageOwner::service`), then content submission
     (`service_content` → `submit_bundle`)
  2. `runtime.service_native`, whose reducer runs `run_tick` for any output
     whose exporter reports pending work (service.rs:353, :421)

  So one turn can settle a result, install it and reach tracked submission.
- **The host derives stride and format.** The offer request carries only
  width and height (isb_jobs/owned_copy.rs:9-28). The host derives the
  description in allocations/image.rs:6-46: row `width*4` with a checked
  multiply, pixel format 1, total `row*height`. The compositor stride is
  `width_px*4` and the format is `DRM_FORMAT_ARGB8888`
  (sophia-session content/projection.rs:96-100). Revision 1's "declared
  stride" is withdrawn; the plan freezes this host-derived stride and extent.

## Service sequence

Service 1, admission. This runs in the frame-commit turn, through existing
calls in this order:

1. **Eligibility.** The whole-composition eligibility check (F4) runs on
   metadata only. If it refuses, the frame takes today's path.
2. **Facade reservation.** `reserve_combined_frame(head)` reserves the
   facade's cell and returns the inbox.
3. **Runtime admission.** `reserve_frame_commit` runs with the combined
   admission joined (F3). It reserves the response room, the single worker
   credit, `begin_isb` and `bind_frame_copy`, and returns the
   `CombinedReservation` with the same worker job. A refusal cancels the
   facade reservation.
4. **Plan.** `compose_provisional_head_frame` builds the plan (F1, F4) and
   allocates the native frame id.
5. **Submission.** `submit_reserved_copy` runs in combined mode (R1): the
   existing operation, with its worker send replaced by `submit_ring_combined`.

Service 2, result. This runs in the turn the result wake rings:

1. **Drain.** The session drains `take_combined_frame_result(head)`. It
   checks the token against the row's reservation and the correlation
   against the row's job.
2. **Settle.** `settle_combined_frame` clears the credit and returns the
   pixels. An exported result moves into the facade's cell as `Held` (F2),
   so the facade owns the lease before the credit clears.
3. **Reconcile.** `OwnedCopyOwner::receive_forwarded(job, pixels)` (F3)
   records the result. The image owner's existing `reconcile` and
   `publish_releases` then make the resource resident and journal the
   source release, in the same service call.
4. **Bundle.** The content service runs
   `begin_content_submission → begin_submission_inner` unchanged. That
   builds the `ContentRenderBundle` with real store leases and applies the
   Stale and Budget checks.
5. **Install or fall back.** If the bundle belongs to the row's candidate
   and every check in F1 and F2 passes, `submit_rendered_bundle` installs
   the result and accepts it. Otherwise R4 classifies the decline. Only
   scene-only churn retains the held result for Release and lets today's
   `submit_bundle` render conventionally, which is FALLBACK.md's one-shot
   path. Every other decline takes its existing refusal or the R4
   failure-terminal baseline.
6. **Submit.** `service_native` in the same turn exports and submits
   through the unchanged tracked path.

Service 3, completion: page-flip retirement, presentation, terminal and
credit return, all as today.

## F1: exact scene equality

The diagnostic checksum cannot authorize acceptance. `logical_scene_checksum`
leaves out each `ContentImage`'s resource, node and pixel version
(sophia-engine composition_plan/checksum.rs:139-152). Two other resources
with the same generation and geometry hash the same. The scene check
compares exact identities instead.

At service 1 the plan records a `CombinedSceneKey` for the head. It is the
exact list of the head's lowered layers, in order, made of:

- the placement of every layer
- for every `Cpu` layer: `pixel_version`, `handle`, `generation`, `size`,
  `stride` and `format` (`LiveSharedCpuBufferSource`,
  renderer-live cpu_composition.rs:41-51)
- for every `Solid` layer: geometry and colour
- for every shell content image: the `CompositorNodeId` (grant, output,
  candidate, surface and placement), the `ContentResourceBegin` (grant
  epoch, resource id and generation, size, format) and the lease's
  immutable pixel version (`CpuPixels.version`, from
  `next_cpu_pixel_version`, resources/backing.rs:53-57; this needs a
  read-only accessor)
- the head's `target_generation`, the `content_layout_generation`, the
  presented transform, and the cursor plan

The candidate's own image appears as a placeholder that carries its
`CompositorNodeId` and host-derived description.

At consumption the owner recomposes the same key from current state. This
is metadata work: no pixels are touched and no lease is created. The key
must match exactly, except that the placeholder must now match the bundle's
real image. That image's description must equal the validated one and its
node must equal the placeholder's. The recomposition is owner work and is
counted.

Classification:

- **Candidate's own authority or geometry fails** (grant lost, interaction
  revoked, foreign retirement claim, allocation or scale changed, placement
  out of bounds, validated and declared descriptions differ): the existing
  refusal and withdrawal rules apply, through the conventional content
  service. These never become a scene-only fallback.
- **Head topology changed** (target generation, head disabled or mirrored,
  output remapped, native owner changed): the fast path is declined, and the
  frame gets the R4 failure-terminal baseline. Conventional re-entry here is
  decision 3's unapproved extension.
- **Any other layer or input differs**: this is scene-only churn and takes
  FALLBACK.md's one-shot conventional path.

Negative controls, named before validation:

- another component's resource is replaced by one with the same numeric
  generation and geometry
- another component's grant is replaced or revoked
- a surface's `pixel_version` changes with the same generation
- the transform or `content_layout_generation` changes
- the cursor moves

## F2: entering the native lifecycle as an accepted render

An accepted combined result becomes the facade's in-flight request. Storing
it beside the facade would not work: the tick returns early unless
`pending_frame()` is true (singleton_tick.rs:31-35). `pending_frame()`
reports `in_flight` (discovery.rs:586-590), and failure cleanup chooses
`rendering_content` only when the worker was in flight (singleton_tick.rs:262-293).

The facade's cell has four states:

- `Free`
- `Reserved`: set at admission
- `Held(WorkerResult)`: set at settlement, for an exported result that is
  not yet accepted
- `Retained(DiscardedWorkerLease)`: a refused result, awaiting Release

Settlement moves an exported result into `Held`. The facade owns it, so the
credit can clear (the 4c-3 order). A failed or expired result frees the
cell. In 4c-4, 4c-3's `settle_combined_frame` stops retaining an exported
result for Release; it takes it as `Held` instead. Refusal converts `Held`
to `Retained` on the facade that issued it. The `WorkerResult` never leaves
the facade, and no lease is created for a refused result.

Acceptance has two phases. Every check runs before anything changes:

1. `preflight_rendered_install(frame)` in the visual runtime runs the
   setter's checks in its order (shell_content.rs:53-117). It writes
   nothing.
2. `preflight_combined_accept(head)` in the native scanout requires all of:
   - the facade has `Held` with the row's job
   - ordinary `in_flight` and `pending_frame` are both `None`
   - the head's `pending_content` and `rendering_content` are both `None`
   - no protected retirement (renderer_images.rs:280, :471-488)
   - matching `target_generation` and `NativeFrameOwner`

   If an ordinary frame was queued for the head during validation, the
   scene changed, so it falls back.

If both preflights pass, the commit step cannot fail. It writes the
following, in this order:

1. runtime `shell_content` and `retained_projection_retirements`, with the
   retirement claim bound to the plan's native frame id, as
   `bind_shell_retirement_claims` would (projection/retirement.rs:129)
2. head `rendering_content` and `last_checksum` (as
   composition_installation.rs:193 does)
3. `output_frames` pending then rendering, with the plan's damage snapshot
4. facade `in_flight = InFlight { request: COMBINED_REQUEST, correlation, ready }`,
   with the `Held` result moved into `ready` and the cell `Free`

Revision 1 relied on the setter's two-field rollback; with no fallible step
after the first write, nothing needs rolling back.

In the tick that follows, `worker_was_in_flight` is true. `poll()` sees the
`ready` result in place of the ordinary result channel and runs its existing
checks on it: output, request id, correlation, capture and descriptor
(worker.rs:671-697). It then yields `Exported` through the one constructor.
The existing statuses then behave correctly:

- `SubmittedWaitingForPageFlip` selects `rendering_content`
  (singleton_tick.rs:136-149) and promotes rendering to submitted
- a preparation or definitive submission failure takes `rendering_content`
  and discards rendering (:262-293)
- an uncertain submission stays on the existing custody path
- `ScanoutExportPending` cannot occur, because `ready` is already terminal

Ordinary `in_flight` still means one outstanding request: the facade has
one at a time, and a combined request is tagged `COMBINED_REQUEST`.
`worker_in_flight()` keeps its meaning for topology installation
(installation.rs:318-328). An accepted combined render blocks a topology
install exactly as an ordinary render does.

Cancellation before export: once accepted, the request is in flight like
any other. Revocation follows the existing in-flight rules; it does not
need a new one.

Test: a device-free fixture drives a real head and exporter through success
to `SubmittedWaitingForPageFlip`, through preparation failure, and through
definitive submission failure. It checks head contents, output frames and
the cell after each.

## F3: joining owned-ring admission and reconciliation

There is one admission and one allocator. `reserve_copy`
(epoch_registry/owned_ring.rs:121-168) gains a worker-reservation mode:

- **ordinary:** `worker.reserve`
- **combined:** `worker.reserve_combined`

In combined mode the returned reservation's `job()` becomes the
`worker_job`. `begin_isb` failure releases that row with `release_reserved`,
as today. `reserve_frame_commit` passes the mode through and binds the same
job to the candidate's `FrameCopyState` (commit.rs:104-117) and to the
resource. `FrameAdmissionReservation` carries the `CombinedReservation`
out. No second credit is spent, and the candidate's job equals the
validator's job.

There is one reconciliation entry. Clearing the token yields pixels and
nothing else. The owned-copy owner gains
`receive_forwarded(job, pixels) -> Result<(), ValidatedIsbPixels>`. It does
what `receive` does (isb_jobs.rs:135): it matches
`worker_job == job && submitted && result.is_none()` and sets
`result = Some(Ok(pixels))`. It takes no `IsbValidationCompletion` and never
calls `take_completed`. A mismatch returns the pixels. The existing
`reconcile` then calls `complete_isb(grant, resource, result)`, which makes
the backing resident in the store: there is no fabricated lease, because
the store makes it. `publish_releases` → `finish_frame_copy` then matches
the same job and binding (isb_jobs/owned_copy.rs:204, credits/copy.rs:31)
and journals the source release. Retry ownership is the existing `Releasing`
entry, and `EAGAIN` keeps it. Across a disconnect the row keeps the
original grant epoch. `finish_frame_copy` refuses a stale identity, as today.

Corrections to the revision 1 failure table, with each path cleaned up
exactly once:

| Path | Credit | Facade cell | Plan | Resource and candidate |
| --- | --- | --- | --- | --- |
| refused before the facade reservation | none | none | none | today's path |
| facade reserved, runtime refuses | none | `cancel` | none | refusal as today |
| runtime admitted, plan build fails | `withdraw(Cancelled)`, row released | `cancel` | none | existing withdrawal |
| validation fails or inbox refuses | cleared by `take_completed` | `cancel` when the owner plan returns | dropped on the owner | ordinary Malformed or failure reconcile |
| forwarded, result Failed or Expired | `settle_combined_token` | `Free` | consumed | `receive_forwarded`, then R4 failure terminal (re-entry is decision 3's extension) |
| forwarded, result Exported | `settle_combined_token` | `Held` | consumed | `receive_forwarded`, then install or fallback |
| renderer route disconnects | kept | kept | in flight | kept until `CompletedNativeOwner` |
| facade dropped | kept | gone with the facade | in flight | kept until qualified proof |
| journal full | unchanged | unchanged | - | `Releasing` retry |

Rollback test at each reservation boundary: malformed validation, inbox
refusal, `begin_isb` failure, and blocked reconciliation and journal.

## F4: whole-plan eligibility and validated backing

Admission checks the entire provisional composition for the head, not only
the candidate. It is eligible only if all of these hold:

- every layer is `Cpu` or `Solid`; `DmaBuf`, `RendererImage`, `Snapshot`
  and `ManagedCapture` refuse
- `image_reads` is empty
- every `Cpu` layer's storage is `Shared`, `Content`, `Bytes` or `Lock`.
  Dropping any of these frees memory or unmaps a sealed mapping, and none
  holds a device handle (cpu_composition/pixel_storage.rs:7-15; Content's
  drop ends in `SealedMapping`'s munmap, sophia-linux-peer
  sealed_mapping.rs:88-94)
- the cursor plan is plane-only, with no layer composited from a renderer
  image
- the earlier single-head, idle-worker, CPU-content, no-capture and
  lean-lane conditions also hold

A refusal is a metadata decision made before any reservation.

Validated backing on the renderer: the placeholder layer carries only the
host-derived description. At dequeue the renderer checks that the validated
size, stride and extent equal it. Binding follows slot acquisition (R3).
The renderer then wraps the `ValidatedIsbPixels` in a new
`LiveCpuPixelStorage::Validated(Arc<ValidatedIsbPixels>)` and binds it as a
`Cpu` layer. This adds no copy and no admitted lease: in the owned-ring lane
the pixels are `ImmutableCpuPixels::Owned(Box<[u8]>)` (isb_pixels.rs:22-25),
and wrapping them in an `Arc` moves a pointer. After the render the renderer
drops the frame and recovers the pixels with `Arc::try_unwrap` into
`CombinedRenderResult.pixels`. The row's backing charge stays on the row
throughout. If the renderer kept a reference beyond the render, as damage
history or a retained snapshot could, `try_unwrap` fails. That is a
stopping condition to settle in the prototype, not a case to work around.

Tests:

- refusal for each disallowed layer class and for `image_reads`
- dropping the validator queue and unwinding with a plan inside, showing
  only memory is freed on that thread
- the renderer returns the pixels with a strong count of one

## F5: identical frames and protocol work

Revision 1's `Ok(false)` branch is removed. A new candidate cannot equal
the installed frame: `LiveShellContentFrame` equality includes
`candidate_generation` and every image node
(production_visual_runtime.rs:389-404, shell_content.rs:89-91).
Admission refuses any `candidate_generation` not above the last one
(candidates/admission.rs:33, credits.rs:267). Duplicate results are refused
earlier, by token and job identity, and create no candidate obligation. If
the install preflight still finds equality, that is an invariant failure:
the fast path is declined and today's conventional path runs, which keeps
today's Prepared and pending behaviour.

Protocol work on the success path is `submit_bundle`'s with the runtime
call replaced. The `Projection`, `Prepared`
(`transport.content_prepared(..)`, which runs `ContentCandidateStore::prepare`
and consumes a response credit) and pending-presentation steps stay the
same. The terminal and credit return stay with service 3.

Test: a duplicate result and a replayed job leave the candidate's state
unchanged and both response obligations intact, and return the credit once.

## Backend correction before any caller (4c-4)

1. **Routing:** by `CombinedHead`, which is `NativeFrameOwner`,
   `RenderHeadId`, `image_store`, claim pointer and `target_generation`.
   The exporter is found by `image_store` and head, and the claim must be
   the same pointer. Collection, cancellation and disposal reach the
   issuing facade after any target change. Only acceptance checks current
   validity.
2. **The cell** gains `Held`. Settlement holds an exported result rather
   than retaining it for Release. Refusal returns the non-cloneable result
   to the issuing facade's cell; it never exposes `WorkerResult`.
3. **Not found is distinct:** "renderer not found" is its own error, not a
   settlement.
4. **Operations are tied to the job:** an operation on one facade must name
   that facade's exact pending job.

Tests: disable, mirror and remap between reserve and settle; the same
output key with a replacement claim; unrelated pending work on another
facade; collection, cancellation and settlement each covered.

## Stopping conditions

- The renderer cannot return the validated pixels unshared after a render.
- Provisional composition cannot produce the F1 key without writing
  installed state.
- Acceptance cannot be made infallible after its preflights.
- The plan type cannot stay opaque in sophia-runtime.
- A three-slot pool or ordinary contention postpones an admitted job
  without bound.

Per n7fj3c2p, none of these may be worked around with worker-side DRM,
owner-side pixel work, cached liveness or a new ring.

## Decisions for niltempus

1. **Scope.** Step 3 touches four places:
   - sophia-renderer-live: the placeholder layer and the `Validated` storage
   - the backend visual runtime: provisional composition, the scene key and
     the two-phase install
   - the backend native scanout: the two-phase accept and the `Held` cell
   - sophia-runtime and the session: the joined admission,
     `receive_forwarded`, the generic plan and the owner row

   Accept this scope, or stop at the scaffold.
2. **Eligibility.** The F4 set is deliberately narrow.
3. **Fallback extensions, not approved; baseline in R4.** FALLBACK.md
   approved conventional re-entry only for scene-only churn. For every
   other decline the baseline is a failure terminal. Three extensions are
   offered as separate choices, each re-entering the conventional path:
   - after a renderer failure or expiry
   - after a head topology change that still passes the ordinary checks
   - when the head is busy with ordinary work
4. **Order:**
   - this design's authority pass
   - 4c-4
   - the gate-3 prototype on these seams with a deterministic renderer
   - prototype acceptance
   - gate 4, a default-off real integration

## Revision 3: the three ownership paths and clarifications

### R1: the owned-ring submission keeps its bookkeeping

The combined path does not replace `submit_reserved_copy`
(shell_transport/owned_copy/admission.rs:121). It runs that operation in a
second mode. The operation keeps every step, in its current order:

1. Open the ring read ticket with `active.ring.offer(slot, bytes)`.
2. Insert `Work::Working { identity, job, withdrawn: false,
   handed_off: source.is_ok() }` into the reserved work slot.
3. Submit through the job table.
4. If the submit is refused, drop the ticket and call
   `jobs.withdraw(.., Cancelled, ..)`.

Combined mode changes only step 3. Ordinary mode keeps
`OwnedCopyImageJobs::submit` (isb_jobs/owned_copy.rs:266).

Combined mode calls a new `OwnedCopyImageJobs::submit_combined(worker, job,
source, reservation, plan, inbox)`. It makes the same binding check as
`submit`:

- the `worker_job` matches
- the job is not submitted and not stopped
- it has no result
- `source.matches(binding, slot)`

It then calls `worker.submit_ring_combined(reservation, source, plan,
inbox)` (isb_worker/combined.rs:200), which also checks the reserved byte
count against the ticket's. The job table sets `submitted` only after a
successful send. The worker's own row was marked submitted by
`send_combined`, so both flags move together or neither does.

A refused send returns `(failure, ticket, plan)`. The ticket is dropped and
step 4 runs as today. The plan returns to the session, which drops it on the
owner thread and cancels the facade reservation.

A successful send returns the owner-plan `Receiver`, which goes to the
session row. A later validation failure or inbox refusal returns the plan
through it, and its arrival triggers the facade cancel in F3's table.

A plan-build failure goes down the refused-send branch. The session builds
the plan first and passes `Option<P>`; `None` skips the send. `Work::Working`
then exists, the ticket is dropped, and the withdrawal reaches `reconcile`
(shell_transport/owned_copy.rs:159-181) with its owner entry present, so
that `expect` holds on every path. Today's `publish_releases` then publishes
the source release from the `Releasing` entry, with the same `identity` and
`handed_off`.

Controls:

- A real owned-ring combined submission reaches `receive_forwarded`,
  resource reconciliation and source publication with the same job, ring
  and slot.
- A foreign ticket is refused, and the ticket comes back.
- A forced send refusal leaves no orphaned row, no panic and no lost source
  release. So does a plan-build failure.

### R2: the owner row disposes of a Held result

The exact owner row decides what happens to a `Held` result. It never waits
on a bundle that may not arrive. In each owner turn, after the image owner's
service and the content service, the session visits every row whose facade
is `Held` and was not accepted that turn. It calls `release_held(head, job)`,
which converts `Held` to `Retained` on the issuing facade (revision 4: the
flush that follows is in "Revision 4" below). It does this in any of these
cases:

- the content grant is gone, or the connection has closed
- the row's job was withdrawn, stopped or refused in reconciliation
- the frame-stream candidate is terminal, withdrawn, superseded or absent in
  every one of its live states (revision 4: `frame_candidate_state`, below)
- the row's admission deadline has passed

The resource, source and journal obligations stay with their own owners:
the `Releasing` entry, `publish_releases` and the `EAGAIN` retry. Only the
renderer lease is given up.

Temporary backpressure keeps the result `Held` until the deadline. That
covers reconciliation being blocked, or publication returning `EAGAIN`
before the bundle exists. The existing service runs every owner turn, and
the session's walk runs in those same turns, so no wake or timer is added.

A returned `Err(pixels)` from `receive_forwarded` is handled in "Revision 4"
below. Revision 3's rule, which dropped pixels for a withdrawn or stopped
job, is withdrawn.

The row reaches the issuing facade through `CombinedHead`, even after the
output or the connection's authority is gone. If the facade itself is gone,
the row takes F3's qualified-retirement path.

Controls:

- Hold an exported result and revoke the grant before reconciliation. The
  exact lease is enqueued for Release without any `submit_rendered_bundle`
  call.
- Repeat with the allocation lost.
- Repeat with the candidate rejected by the content service.
- Apply journal or reconciliation backpressure, then withdraw at the
  deadline.

### R3: acquire the slot, then bind

`render_frame` acquires the frame slot before it renders
(exporter/worker/service.rs:646-650). The combined render keeps that order:

1. Check the deadline and cancellation, as `OutputRoute::serve` does.
2. Call `frame_slots.try_acquire()`:
   - **`Deferred`:** return `RenderAttempt::NoSlot` with the plan still
     unbound. The envelope still owns its `ValidatedIsbPixels`, and `serve`
     parks it as it does today: one parked job, the original deadline, no
     owner visit and no second capture.
   - **`Acquired(token)`:** wrap the pixels in an `Arc`, bind the
     placeholder, and call `render_frame_in_slot` with that token. A
     failed render releases its slot, as today.
3. Drop the frame and `Arc::try_unwrap` the pixels into the result.

A `Deferred` attempt never holds a bound frame, so `try_unwrap` never runs
while a reference is alive. Each render acquires its token once.
`IncarnationExhausted` is a terminal failure that returns the unbound
pixels. A reference that survives a terminal render remains a prototype
stopping condition (F4).

Controls:

- With all three slots occupied, dequeue a combined job, defer it
  repeatedly, then release one slot. It renders once and returns the exact
  pixels.
- Expire or cancel the job while it is parked. The plan, pixels and credit
  settle without any `try_unwrap`.

### R4: classification aligned with FALLBACK.md

Revision 2's "otherwise fallback" is narrowed. Only scene-only churn takes
the approved conventional re-entry: every candidate and head check passes,
and only the `CombinedSceneKey` differs. Every other decline keeps its
existing classification:

- A failure of the candidate's own authority or geometry leads to the
  existing refusal or withdrawal.
- A head topology change, a head busy with ordinary work, or a renderer
  failure or expiry ends in a failure terminal as the baseline. It uses the
  terminal today's path emits for the same condition; the prototype pins the
  exact mapping. The lease goes from `Held` to `Retained`, and the resource
  and source settle as in R2.

Ordinary work queued for the head does not prove the scene changed. It
declines as "head busy", not as churn. Conventional re-entry for any of
these cases is decision 3's separate, unapproved extension.

### R5: clarifications

- **The scene key fails closed.** A `Cpu` layer whose `pixel_version` is
  `None` makes the plan ineligible. The source type documents `None` as
  "always uploads", so it is not an immutable version, and two `None`s
  never count as matching pixels. A text or indicator layer whose current
  raster key cannot be read without rasterizing declines the fast path;
  metadata recomposition never rasterizes. This is a testable eligibility
  rule.
- **The acceptance preflight covers more than F2 listed.** It also checks:
  - that `output_frames` permits the pending and rendering transitions with
    the plan's damage snapshot
  - that the damage queue accepts mark-rendering
  - that the facade is not quarantined or stalled
  - the frame's exact content and retirement identity

  "Commit cannot fail" then covers every fallible `output_frames`
  transition. The F2 fixture drives each refused preflight and shows that
  no state changes.
- **Pixel storage.** F4 now names the owned-ring storage as
  `Owned(Box<[u8]>)`. No copy is made either way.

## Revision 4: the R2 transitions

### Retained Release has an owner-service path

`release_held` cannot rely on the next `poll()`. A declined result creates
neither a `pending_frame` nor an ordinary or accepted `in_flight`
(discovery.rs:586-590), so `singleton_tick` returns before export
(singleton_tick.rs:31-35). On an idle or disabled head, `poll()` is never
called.

The backend gains a narrow maintenance entry,
`flush_combined_release(head: &CombinedHead) -> CombinedReleaseFlush`. It
finds the issuing facade by `CombinedHead`, then calls that facade's
existing nonblocking `flush_combined_release` (worker/release.rs:103).
That is the same operation `poll()` calls first, without the rest of
`poll()`, so it cannot consume an unrelated ordinary result. Because the
lookup is by `CombinedHead`, it works on a disabled or remapped head.

`release_held` converts `Held` to `Retained` and calls the maintenance
flush once. The row keeps a Release debt until the flush returns `Enqueued`:

| Flush result | Meaning | Next step |
| --- | --- | --- |
| `Enqueued` | ownership has moved to the command queue; the physical slot is freed later, when the worker handles the Release | the row's renderer half is closed |
| `Full` | the obligation stays in the cell | the session's walk retries |
| `Disconnected` | the worker cannot take the Release | the debt stays until qualified retirement |

Each owner turn, the session's walk visits rows that are `Held` or carry a
Release debt. That covers rows on disabled or remapped original heads. The
walk runs inside existing owner turns, whose authority wait has a 25 ms
maintenance cap, so no timer or thread is added.

Controls:

- Decline one exported result on an idle output and queue no further
  render. Its Release must reach the worker.
- Repeat with `Full` on the first attempt, then a successful retry.
- Repeat with the head disabled before the decline.

None of these cases may need a later render to finish its cleanup.

### Candidate liveness includes the assembly and its copy gate

An owned-ring candidate can be live while it is still assembling. The
assembly survives while `assembly.copy` is present (candidates.rs:620). The
copy gate completes only after resource reconciliation
(candidates/credits/copy.rs:31), and only then does validation move the
candidate to pending (candidates.rs:601-603, 716). Revision 3's predicate,
which checked only pending and submitted, would have discarded `Held` during
exactly the backpressure R2 meant to retain it.

The read-only store query becomes
`frame_candidate_state(binding: &FrameCopyBinding, job) -> FrameCandidateState`.
Its variants:

- `Assembling`: the assembly for `binding.key` has
  `begin.candidate_generation == binding.generation`, and `assembly.copy`
  names this `job` (stopped or not yet finished).
- `Pending` or `Submitted`: the same exact binding is in that table.
- `Ended`: provable from existing state, with no new record. Candidate
  generations strictly increase, and each output holds at most one
  assembly and one pending entry (credits.rs:267-269). So the binding has
  ended when that output's assembly, pending or submitted entry carries a
  later `candidate_generation` for the same stream key, or when the stream
  key or ledger is no longer open for `binding.key`. Terminal outcomes are
  emitted as records (`self.outcome(..)`, credits.rs:315-324) and are not
  kept for querying, so the query does not read them.
- `Unknown`: none of the above. The walk treats `Unknown` as live until the
  deadline. It never takes absence from two tables as proof of death.

The walk releases `Held` on `Ended`, on the separate stopped or withdrawn
check, or at the deadline. It keeps `Held` on `Assembling`, `Pending`,
`Submitted` and `Unknown` before the deadline.

Controls:

- Block resource reconciliation while the candidate is still assembling.
  The row stays `Held` until its deadline.
- Unblock it before the deadline. The candidate becomes eligible and is
  installed with no second speculative render.
- Reject the candidate, or withdraw it. Each converts `Held` to `Retained`.

### Withdrawal does not settle the charges, so late pixels go to reconciliation

`CpuImageJobs::withdraw` stops the store row and, for submitted work, only
calls `worker.cancel` (isb_jobs.rs:97-130). Stopping cannot release the
worker mapping or its charge (resources.rs:250). `complete_isb` consumes the
returned result and calls `abort_finished` for a stopped transfer
(resources.rs:325-356). Revision 3's rule that dropped the pixels of a
stopped job is therefore withdrawn.

`receive_forwarded` accepts a stopped job. Its predicate is
`worker_job == job && submitted && result.is_none()`, with no condition on
`stopped`. A late result for a withdrawn job enters that job's row, and the
existing `reconcile` → `complete_isb` path consumes it. For a stopped
transfer that path ends in `abort_finished`, and the staging and reserved
charges clear exactly once there.

The exact-row invariant leaves only two ways for `receive_forwarded` to
return `Err(pixels)`:

- **The row's `result` is already `Some`.** That would mean a second result
  for one job. `settle_combined_token` consumes the token once, so this
  cannot happen.
- **No row matches.** This is an invariant failure.

Either way the handling fails closed. The row keeps the pixels and the
charge obligation and records the failure. It drops neither until exact
reconciliation or qualified disposition: connection teardown through
`withdraw_epoch` and its stop reconciliation, or native retirement. The
renderer half (Held, Retained, Release) is independent and continues
meanwhile.

Controls:

- Withdraw a submitted combined job before its returned pixels are
  reconciled. Its staging and reserved charges must remain until
  `complete_isb` consumes the result, then clear exactly once.
- A `receive_forwarded` for an unknown or mismatched job must leave the
  charges unchanged and keep the only copy of the pixels.
