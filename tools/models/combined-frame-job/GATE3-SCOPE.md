# Gate 3 scope: the device-free prototype of the combined frame job

Revision 3. It answers Codex's pre-build review of the G3-1 source
(1ab47bc4f, package seal da774eac; HOLD of 2026-11-11). Finding 1: an
extant stream key is not an open stream, so `frame_candidate_state` needs
the exact key for a live state and the Open phase before it can answer
Unknown. Finding 2: SR1's claim that the collecting call journals the
source release was wrong, and this revision corrects it to the real
publication order below; the full-journal control stays in G3-1. Finding 3:
the post-credit `begin_isb` refusal gets its own control. The rest of
revision 2 is unchanged.

Revision 2 answered Codex's review of revision 1 (469effbaf, package seal
e2c4cf06; review of 2026-11-11). Its findings are named SR1 to SR3 here, to
keep them apart from AUTHORITY-STEP3's R1 to R5. SR1 places result intake
before reconciliation and content service. SR2 names the provenance of the
stop flag and of the validated description. SR3 binds every placement of the
copied resource. The stage order changes because of SR2: the runtime stage
now comes first.

Status: scope for review by Codex before any gate-3 source. It selects no
source by itself. It stays inside niltempus's selections of 2026-11-11 in
AUTHORITY-STEP3.md: the full scope staged, the narrow F4/R5 eligibility set,
and the failure-terminal baseline with none of the three re-entry extensions.
Base: design/combined-frame-job at 2048dc2c393138e7476e0e2baaaa7c85275b8531,
which holds the accepted 4c-4 scaffold (0f29023d8).

n7fj3c2p gate 3 asks for a test-only prototype that uses the real capture,
validation and journal seams with a deterministic renderer. It must keep
today's full-journal, disconnect and stale-result behaviour, and it must
prove that the normal path has no intermediate validation return to the
owner. Gate 4, a default-off integration into the real owner loop, is not
part of this scope.

## What "test-only" means here

Codex agreed with this reading. The composed path runs only from tests. New
source sits behind `cfg(any(test, feature = "experimental-combined-frame-job"))`,
applied per item. A crate's tests that call a dependency's combined-only API
also require the feature, because `cfg(test)` in the calling crate does not
enable it in the dependency. No production caller is added:
`physical_input_loop.rs`, `component_service.rs`, `native_service.rs` and
`ShellComponentConnections::turn_connected` are unchanged, and nothing on the
real owner turn reaches a combined entry. The device-free backend pieces the
session harness needs are exported as `session_content_fixture` is, behind
`test-support`, `doc(hidden)` and the combined feature. Feature-off builds
compile to today's code, and each stage shows that with feature-off clippy
and the unchanged control suites.

sophia-renderer-live gains its own `experimental-combined-frame-job`
feature, enabled from sophia-backend-live's feature of the same name. That
feature and its one-line propagation are the only Cargo changes in gate 3.

## One host-derived description, carried two ways (SR2)

The owned-ring peer declares only width and height in its offer.
`owned_copy_description` (runtime allocations/image.rs:6-46) derives the
`ContentResourceBegin` from them: `width_px`, `height_px`, `pixel_format` 1,
and `total_bytes = width*4*height` with a checked multiply. `reserve_copy`
reserves exactly `total_bytes`, and `OwnedCopyImageMetadata.description`
keeps it on the job row. `ValidatedCpuPixels` carries bytes only, and
`OwnedRingRead` records only a byte count, so there is no third independent
geometry. The description has one source of truth, the runtime's admission.

It reaches the renderer by two routes that never meet before the renderer:

- **with the job.** `OwnedCopyImageJobs::submit_combined` passes the row's
  admitted description to `worker.submit_ring_combined`, which stores it in
  the job's route. `ForwardedValidation` exposes it read-only as
  `description()`. The runtime takes it from its own row and never from P.
- **with the plan.** Service 1's provisional composition builds each
  placeholder from the description that `FrameAdmissionReservation` returns
  for this admission, as `project_render_bundle` builds a conventional image
  from the resident resource's description.

At bind the renderer requires, for every placeholder, that its width,
height, stride (`width*4`) and format equal the job's `description()`, and
that `pixels.bytes().len() == description().total_bytes`. The first check
ties the plan to this job: a plan built for one admission and a job admitted
for another disagree even when their byte totals are equal, for example a
4x2 placeholder against a 2x4 job. The second ties the validated bytes to the
admission. A mismatch is a terminal renderer outcome that returns the pixels
unbound. The owner then publishes Malformed through the existing failure
reconcile, as TRANSFER.md requires: `receive_forwarded(job,
Err(ContentStoreError::Malformed))`, with the pixels dropped on the owner
thread and the candidate ending in the failure terminal.

## The stop flag at dequeue and retry (SR2)

The validation job already has a one-way `cancelled: Arc<AtomicBool>`,
which `IsbValidationWorker::cancel` sets and the validator reads
(isb_worker.rs:51, :276-283, :340). `combined_job` already clones it into
the `Job` (combined.rs:250). The route keeps a second clone, and
`ForwardedValidation::stopped()` reads it with Acquire. The flag is the
runtime's: the renderer reads it and never writes it.

`OutputRoute::serve` checks `stopped()` beside the deadline, when it takes a
job from the inbox and again each time it retries a parked job. A stopped job
returns as `CombinedOutcome::Stopped` with its pixels unbound, and no slot is
acquired. A cancel that races the check can still let one bounded render
run. Its result comes home, and the owner's walk releases it as R2 already
specifies for a withdrawn job.

## Stages

Five source stages follow this document. Each has its own frozen signed
source, sealed source review before any build, Codex read, isolated
validation with exact test counts and named mutants, and acceptance. G3-2
needs G3-1's envelope accessors. G3-3 needs G3-2's placeholders. G3-4 needs
G3-3's frame. G3-5 needs all four.

### G3-1: the runtime's joined admission, intake and reconciliation (F3, R1, rev 4, SR1, SR2)

Places: `shell_content/epoch_registry/owned_ring.rs` `reserve_copy` and
`owned_ring/commit.rs` `reserve_frame_commit`;
`shell_transport/owned_copy/admission.rs` `submit_reserved_copy` and
`owned_copy/frame.rs` `propose_frame_commit`;
`shell_content/isb_jobs/owned_copy.rs`; `shell_transport/owned_copy.rs`;
`shell_content/isb_worker/combined.rs`; the candidate store's read-only
`frame_candidate_state`; `shell_transport/image_owner.rs`.

- `reserve_copy` takes a reservation mode. Ordinary calls `worker.reserve`;
  combined calls `worker.reserve_combined`, and the returned job becomes the
  `worker_job`. `begin_isb` failure releases it as today.
  `FrameAdmissionReservation` carries the `CombinedReservation` and the
  admitted description out.
- The runtime reaches the session and backend through one trait it defines,
  `CombinedFrameHooks<P>`, so P stays opaque (stopping condition 4). Its
  methods run in AUTHORITY-STEP3's service-1 order inside
  `propose_frame_commit`: `reserve` (eligibility and the facade
  reservation) before `reserve_frame_commit`; `plan(admission) -> Option<P>`
  after it; `cancel` on any refusal. Its `collect` method is the service-2
  intake below. Without hooks, `propose_frame_commit` is today's code.
- `submit_reserved_copy` keeps its four steps and order. Combined mode
  changes only the send: `OwnedCopyImageJobs::submit_combined` makes
  `submit`'s binding check, then calls `worker.submit_ring_combined` with the
  row's description. `None` from `plan` takes the refused-send branch with
  `Work::Working` present, so `reconcile`'s `expect` holds.
- The envelope gains `description()` and `stopped()`, as above.
- `receive_forwarded(job, Result<ValidatedIsbPixels, ContentStoreError>)
  -> Result<(), ..>` matches `worker_job == job && submitted && result.is_none()`,
  with no condition on `stopped` (rev 4). `Err` hands the input back.
- `frame_candidate_state(binding, job)` returns Assembling, Pending,
  Submitted, Ended or Unknown, read-only, as rev 4 defines them.
- `ShellIsbImageOwner<P = DefaultPlan>` and its owned-copy part take the
  defaulted generic plan, so existing callers compile unchanged.

**Service-2 order (SR1).** `ShellIsbImageOwner::service_combined(transport,
epochs, now, instant, hooks)` first runs `hooks.collect(&mut worker,
deliver)`. The session drains each row's result, settles it against this
validator, and `deliver` calls `receive_forwarded`. Only then does the
existing body run, unchanged: `service_owned` (stops, expiry, `reconcile`,
`publish_releases`, and service 1 for the next frame), the completion drain,
and the rest. So a forwarded result is reconciled in the same service call
that collected it: the copy gate closes, the candidate becomes ready and
the terminal ResourceStatus waits in the resource store. `service` stays as
it is and has no hooks.

Publication keeps today's order (revision 3). The status can only reach the
journal through the outbox, and SlotReleased is refused (EAGAIN) until the
status is journalled. In the same owner turn the host's existing
content-resource service moves the status to the outbox and the file turn
journals it. SlotReleased is then published by the next image-owner call,
which in the three-service design is service 3, the presentation turn, as
n7fj3c2p already places source publication at the existing journal
boundary. Today's path has the same shape: the completion turn journals the
status and the following turn releases the slot. Neither acceptance nor
tracked submission waits for SlotReleased, so the one-turn result gate of
G3-5 is unchanged. The client gets its ring slot back one turn after the
result turn, as today it does one turn after the completion turn. Adding a
journal drain inside the image owner to release the slot in service 2 was
considered and not taken: it would reorder the host's outbox drain for one
path and buy one turn of slot latency that no gate measures.

Controls, all on the real owned ring, real validator thread and real journal:

- `a_combined_admission_spends_one_credit_and_binds_the_same_job` (F3)
- `the_envelope_carries_the_rows_description_and_not_the_plans` (SR2)
- `cancel_after_forward_is_visible_on_the_envelope` (SR2)
- `a_combined_submission_reaches_receive_forwarded_reconcile_and_source_release` (R1)
- `intake_runs_before_reconcile_in_the_same_service_call` (SR1: one call
  collects and reconciles; the host's turn journals the status first, and
  the next call publishes exactly one SlotReleased and confirms the
  originating slot, for success, a delivered mismatch and a refused send)
- `a_forwarded_malformed_result_publishes_malformed_once` (SR2)
- `a_foreign_ticket_is_refused_and_returned`
- `a_refused_send_leaves_no_orphan_row_and_publishes_the_source_release`
- `a_plan_build_failure_takes_the_refused_send_branch`
- `malformed_validation_and_inbox_refusal_return_the_plan_once` (F3 table)
- `begin_isb_failure_releases_the_combined_reservation_and_cancels_the_hook` (F3
  table; revision 3: open transfers block only `begin_isb`, after the
  combined credit; one cancel, no plan, no surviving job or work entry, the
  credit restored and used by the next admission)
- `a_withdrawn_job_keeps_its_charges_until_complete_isb_consumes_late_pixels` (rev 4)
- `an_unknown_or_mismatched_receive_keeps_the_charges_and_the_pixels` (rev 4)
- `a_full_journal_keeps_the_releasing_entry_until_retry` (gate 3 full-journal;
  revision 3: reserved terminals cannot be refused by the journal itself, so
  the refusal is an ordinary record ahead of the status in FIFO order; the
  status, the Releasing entry and custody survive repeated calls, and after
  the ACK the status and one SlotReleased publish once)
- `candidate_state_reports_assembling_while_the_copy_gate_is_open` (rev 4)
- `candidate_state_reports_ended_only_from_a_later_generation_or_closed_stream` (rev 4;
  revision 3: a closing stream keeps its key, so Ended needs the Open phase
  check, and a live state needs the exact key)
- `a_duplicate_result_and_a_replayed_job_create_no_candidate_obligation` (F5)

Mutants: combined mode also calls `worker.reserve` (two credits); the
envelope takes its description from the plan argument (the row-description
control fails); intake runs after `service_owned` (the same-call control
fails); `receive_forwarded` rejects stopped jobs (the withdrawal control
fails); `frame_candidate_state` returns Ended on absence from pending and
submitted (the assembling control fails).

### G3-2: the renderer binds validated pixels after the slot (F4, R3, SR2, SR3)

Places: sophia-renderer-live `cpu_composition/pixel_storage.rs`; backend
`exporter/worker/combined.rs` and a new `exporter/worker/combined_render.rs`.
`worker.rs` is at 997 lines and `service.rs` at 878, so neither grows beyond
a call site.

`LiveCpuPixelStorage` gains `Validated(Arc<ValidatedIsbPixels>)`, behind the
renderer-live feature. Its bytes are `ValidatedCpuPixels::bytes()`, its
`pixel_version` is `None` (the bound layer is always uploaded, and the scene
key holds the placeholders, never this storage), and dropping it frees
memory or unmaps a sealed mapping only.

**Every placement is bound (SR3).** Frame validation allows several placements of
the copied resource; it requires only that one names it
(candidates/credits/commit.rs:48-88), and projection makes a node for each
(content/projection.rs:24-104). The plan therefore carries
`placeholders: Box<[CombinedPlaceholder]>`, one per lowered occurrence, at
most `max_candidate_placements`. Each holds its index in the final layer
list, its `CompositorNodeId`, the placement and clip that lowering would
give it, and the width, height, stride and format. Order follows the
candidate's placements, as projection orders them. An occurrence that
lowering would drop, such as one clipped to nothing, has no placeholder,
because the provisional composition lowers the same way. All occurrences
share one `Arc` during the render, and every clone is dropped before
`try_unwrap`.

The placeholders stay outside the layer enum (Codex agreed). The plan's
partial frame is private to `combined_render.rs`. The only way to a
renderable frame is `bind(plan, Arc<pixels>) -> Result<BoundFrame, Unbound>`,
which fills every placeholder or returns everything unbound. The render
callback takes a `BoundFrame`. No `PendingRenderedFrame` with a missing
occurrence leaves that module or reaches either render function.

`OutputRoute::serve`'s closure takes the pixels with the plan. The combined
attempt runs this order:

1. deadline and `stopped()`, at dequeue and at every retry of a parked job
2. `frame_slots.try_acquire()`: `Deferred` returns `NoSlot(plan, pixels)`
   unbound, and `serve` parks both; `IncarnationExhausted` is terminal and
   returns the pixels unbound
3. the description checks above; a mismatch is terminal, the slot is
   released, and the pixels return unbound
4. `bind`, then render in the slot through the render function the caller
   supplies
5. drop the frame, then `Arc::try_unwrap`; a surviving reference stops the
   lane (stopping condition 1) and is never worked around

Step 4's render function is the deterministic-renderer seam. Production
passes `render_frame_in_slot` with the real context. Tests pass a function
that reads every bound occurrence's bytes through the `BoundFrame` and
returns an `Exported` outcome whose correlation carries a checksum of each
occurrence. The slot table, slot release and damage invalidation on
failure, parking and the result route are the real ones.

Controls:

- `the_renderer_returns_the_exact_pixels_with_a_strong_count_of_one`
- `three_busy_slots_park_the_job_unbound_and_one_release_renders_it_once` (R3)
- `expiry_while_parked_returns_plan_pixels_and_credit_without_unwrap` (R3)
- `stop_after_forward_before_the_first_dequeue_renders_nothing` (SR2)
- `stop_while_parked_is_seen_at_the_retry_and_renders_nothing` (SR2)
- `an_equal_length_different_description_fails_terminally_unbound` (SR2, 4x2 against 2x4)
- `a_byte_extent_different_from_the_admitted_total_fails_terminally_unbound` (SR2)
- `an_exhausted_incarnation_returns_the_pixels_unbound`
- `a_failed_render_releases_and_invalidates_its_slot`
- `a_repeated_resource_binds_every_occurrence_in_order` (SR3)
- `the_bound_frame_matches_the_conventional_lowering_layer_for_layer` (kind,
  order, placement, clip and description against the same scene lowered
  from a resident resource)
- `a_placement_clipped_to_nothing_has_no_placeholder_and_no_layer`
- `bind_returns_everything_unbound_when_any_occurrence_cannot_be_filled`
- `the_deterministic_renderer_saw_every_bound_occurrence` (checksums)
- `dropping_a_plan_on_the_renderer_thread_frees_memory_only` (F4)
- `dropping_the_validator_queue_or_unwinding_with_a_plan_frees_memory_only`
  (F4, with the real validator and the backend plan type)

Mutants: bind before acquire (the busy-slot control fails at its "unbound"
assertion); bind only the first placeholder (the repeated-resource control
fails); compare only total bytes (the 4x2/2x4 control fails); skip
`stopped()` at retry (the parked-stop control fails); unwrap before dropping
the frame (fails at the strong-count assertion).

**G3-2 source decisions (Codex, 2026-11-11, before source).**

- D1. The layer-for-layer comparison with conventional lowering, and the
  clipped-to-nothing comparison, move to G3-3, which must compare
  independently obtained conventional and provisional/bound frames from the
  same real scene. G3-2 asserts kinds, order, placement, clip, descriptor
  fields and bytes of bound layers directly, for interleaved layers and
  repeated occurrences, and keeps the bind-only-first mutant. Lowering is
  not changed: ordinary Content and Lock layers keep `bytes.pixel_version()`,
  handle, sampling, placement and clipping. Only the Validated variant has
  `pixel_version` None.
- D2. The deterministic renderer records one checksum per bound occurrence,
  in order and with identity, into a captured list while it reads each
  layer. The result route and the exact pixel return are asserted
  separately. These checksums are test evidence only, never a scene key or
  submission identity.
- D3. A job with no description (a sealed ISB job) is accepted only with an
  empty placeholder list. One or more placeholders with no description is a
  Mismatch, returned unbound without rendering. With a description,
  `bytes().len() == total_bytes` is checked outside the placeholder loop,
  also with no placeholders; width, height, checked `width*4` stride and the
  explicit protocol-to-renderer format mapping are checked per placeholder.
  This does not widen G3-3's whole-plan eligibility, and a plan-derived
  description never replaces the row's.
- D4. If `Arc::try_unwrap` finds a surviving reference after the bound frame
  dropped, the renderer worker panics. That ends the shared renderer worker
  for every output it serves, not one lane. It is a prototype stopping
  condition, never a terminal outcome, a pass or a reconciliation: no copy,
  settlement, credit release or fabricated pixels hide it. Its occurrence in
  any normal control holds G3-2. `WorkerThread::poll_join` keeps the panic as
  an error; disconnection or a panic never proves that the runtime resource
  obligation was reconciled.

**Bind is all-or-nothing.** Before inserting anything, `bind` checks every
placeholder: strictly ascending and unique final-layer indices, each within
the final layer count (checked arithmetic), and a Mixed frame. A bad later
occurrence, such as indices [0, 3] against one retained layer, returns the
input frame unchanged with no retained `Arc` clone. Only a successful bind
makes a `BoundFrame`.

**The terminal disposition reaches the owner.** `settle_combined_frame`
consumes the outcome, so `CombinedRenderResult::disposition()` is a
read-only `Rendered | Expired | Stopped | Mismatch` the owner records before
settling, and a refused settlement returns the result with it intact. A
Mismatch cancels only its own admission's Release reservation, and settling
the credit never implies that the pixels validated or rendered. Publishing
Malformed from it is G3-5's owner work.

**Slots.** Only a failed attempted render invalidates slot damage; a
pre-bind mismatch releases the untouched slot. A deferred attempt keeps the
unbound plan and the pixels, and checks stop and deadline again before it
acquires a slot on retry.

**Fixture.** Description controls use the existing registry reservation
with `OwnedRing::admit` and `offer`, and take the description through the
admitted row where its provenance is under test; a raw
`submit_ring_combined` fixture states its narrower scope.

### G3-3: provisional composition and the scene key (F1, F4, R5)

Places: the backend visual runtime, `production_visual_runtime/` beside
`compositor_graphics/shell_content.rs`, in a new `combined_scene.rs`.

- `compose_provisional_head_frame(head, candidate, admission) ->
  Result<ProvisionalFrame, Ineligible>` lowers the head's current
  composition with every occurrence of the candidate's image as a
  placeholder (G3-2's descriptors). It reads installed state and writes none
  of it. This is the check for stopping condition 2: the test snapshots
  every installed field it reads and compares after.
- `CombinedSceneKey` is F1's exact list, with one placeholder entry per
  occurrence carrying its node and description. It needs one read-only
  accessor for the lease's `CpuPixels.version`.
- Eligibility is F4 plus R5, checked on metadata before any reservation: Cpu
  or Solid layers only, empty `image_reads`, Cpu storage Shared, Content,
  Bytes or Lock, a plane-only cursor, one head, an idle worker, no capture,
  and a known `pixel_version` on every Cpu layer. A text or indicator layer
  whose raster key cannot be read without rasterizing is ineligible.
- `recompose_scene_key(head)` at consumption is metadata only. Every
  placeholder must now match the bundle's real image node for node. It
  returns Match, CandidateFailure, TopologyChange or SceneChurn (F1, R4).
- `preflight_rendered_install(frame)` runs the setter's checks in their
  order and writes nothing.

Controls (F1's five negatives are named in AUTHORITY-STEP3 and kept):

- `another_resource_with_the_same_generation_and_geometry_is_churn`
- `another_grant_replaced_or_revoked_is_churn`
- `a_pixel_version_change_with_the_same_generation_is_churn`
- `a_transform_or_layout_generation_change_is_churn`
- `a_cursor_move_is_churn`
- `a_target_generation_or_native_owner_change_is_a_topology_change`
- `the_candidates_own_grant_loss_is_a_candidate_failure`
- `a_repeated_resource_keys_every_occurrence_and_a_dropped_one_is_a_candidate_failure`
- `each_disallowed_layer_class_and_image_reads_are_ineligible` (F4)
- `an_unversioned_cpu_layer_is_ineligible` (R5)
- `an_unreadable_raster_key_is_ineligible_without_rasterizing` (R5)
- `provisional_composition_writes_no_installed_state`
- `the_install_preflight_refuses_without_changing_state`

Mutants: the key omits the resource id (the same-generation control fails);
two `None` versions compare equal (the R5 control fails); the key holds only
the first occurrence (the repeated-resource control fails); provisional
composition marks the damage queue (the no-write control fails).

### G3-4: the two-phase accept on the native scanout (F2, R5)

Places: `persistent_native_scanout/combined_frame.rs` (198 lines) and a new
`combined_accept.rs` beside it; the facade's cell in `worker/release.rs`;
`poll()` takes `ready` in `worker/combined.rs` through one call site in
`worker.rs`.

- `preflight_combined_accept(head, admission)` requires F2's list plus R5's
  additions: Held with this admission, ordinary `in_flight` and
  `pending_frame` empty, head `pending_content` and `rendering_content`
  empty, no protected retirement, matching `target_generation` and
  `NativeFrameOwner`, `output_frames` permitting pending then rendering with
  the plan's damage snapshot, the damage queue accepting mark-rendering, the
  facade neither quarantined nor stalled, and the exact content and
  retirement identity.
- The commit writes F2's four steps in order and has no fallible step
  (stopping condition 3).
- `poll()` with `in_flight.request == COMBINED_REQUEST` takes `ready` and
  runs the existing output, request, correlation, capture and descriptor
  checks, then yields `Exported` through the one constructor.

Controls:

- `an_accepted_render_reaches_submitted_waiting_for_page_flip`
- `a_preparation_failure_takes_rendering_content_and_discards_rendering`
- `a_definitive_submission_failure_takes_rendering_content`
- `each_refused_preflight_changes_no_state` (one case per R5 item)
- `an_accepted_combined_render_blocks_a_topology_install`
- `an_ordinary_frame_queued_during_validation_declines_as_head_busy` (R4)

**The tracked-submission fixture holds the gate.** These controls need a
device-free head and exporter on which the real acceptance, `poll()`,
export and preparation, and tracked-state code run around one deterministic
device submission boundary. Setting the final state by hand does not count.
`native_prepared_cancellation.rs` and `native_renderer_deferral.rs` seed
head state for cancellation and deferral; they do not establish this
success path. The G3-4 source review pins the fixture. If no fixture reaches
`SubmittedWaitingForPageFlip` this way, G3-4 reports the gap, and the
success-path acceptance, G3-5's tracked-submission controls and gate 3 as a
whole stay held until one does. Reporting the gap does not waive it, and
gate 4 does not start.

Mutants: commit before the damage-queue preflight (the refused-preflight
control fails with partial state); `poll()` reads the ordinary channel for a
combined request (the success control fails).

### G3-5: the owner row and the end-to-end prototype (R2, rev 4, rev 5, SR1)

Places: sophia-session `live_session/metadata_shell/content/combined/`
(new), under the feature: the owner row, its `CombinedFrameHooks`
implementation, and a feature-gated branch in `submit_bundle` that installs
through the two preflights and the commit when the bundle's candidate
belongs to a row, in place of `set_shell_component_content`. The
`Projection`, `Prepared` and pending-presentation steps around it are
unchanged (F5). The harness lives under `tests/support/combined_frame/`, is
mounted by path inside `live_session`, and is built on
`component_files::Harness` (a real connected component, real ring capture
and real journal). It also uses the real `ShellIsbImageOwner` with its
validator thread, and a backend `combined_frame_fixture` exported like
`session_content_fixture`. That fixture carries G3-2's deterministic
renderer on a real renderer worker thread and G3-4's pinned submission
fixture.

The owner row holds the runtime reservation, the `CombinedHead`, the
admitted description, the owner-plan receiver, the admission deadline and
the Release debt.

**One driver turn, in the order a gate-4 owner turn will use (SR1).** The
harness has one function, `combined_owner_turn`, and it makes exactly these
calls, in this order, with nothing else in between:

1. `ShellIsbImageOwner::service_combined` with the rows as hooks. This
   drains and settles each result, calls `receive_forwarded`, then runs
   the existing reconciliation (the copy gate closes and the terminal
   ResourceStatus is queued), SlotReleased for an earlier result whose
   status is already journalled, and service 1 for the next frame.
2. The existing content service for that component: the content-resource
   service queues the status for the file turn, then the candidate service
   builds the bundle, and `submit_bundle` runs the two preflights, the
   commit, Prepared and the pending presentation. SlotReleased for this
   result follows at the next turn's step 1 (revision 3).
3. The row walk: Held disposal for rejected, withdrawn, ended or expired
   rows, and Release-debt maintenance (R2, rev 4, rev 5). It never collects
   results.
4. Native service through the fixture: export, preparation and tracked
   submission.

Gate 4 adds the hooks to `turn_connected` and calls the same functions from
the owner loop. The row and its methods do not change. The source review
reads `combined_owner_turn` to confirm it has no hidden drain, reconcile or
extra service call.

Controls:

- `the_normal_path_has_no_intermediate_validation_return`. Between the
  submission and the renderer's result wake, the validator's readiness
  descriptor stays quiet, and no turn takes a validation completion. The
  frame is admitted in one driver turn and accepted and submitted in the
  next, the result turn. This uses the stage-4a checkpoint pattern
  (WAKE-SEAM.md) on the real validator.
- `an_unchanged_scene_is_accepted_and_submitted_in_the_result_turn`: one
  `combined_owner_turn` collects, reconciles, installs, sends Prepared and
  reaches `SubmittedWaitingForPageFlip`
- `a_repeated_resource_frame_is_accepted_with_every_occurrence_rendered`
- `scene_churn_retains_the_result_and_renders_conventionally_once` (FALLBACK.md)
- `a_renderer_failure_or_expiry_ends_in_the_failure_terminal` (R4 baseline)
- `a_description_mismatch_publishes_malformed_and_ends_in_the_failure_terminal` (SR2)
- `a_withdrawal_after_forward_stops_the_render_and_settles_once` (SR2)
- `a_revoked_grant_before_reconciliation_releases_the_held_lease` (R2)
- `a_lost_allocation_releases_the_held_lease` (R2)
- `a_candidate_rejected_by_the_content_service_releases_the_held_lease` (R2)
- `backpressure_keeps_held_until_the_deadline_then_releases` (R2, rev 4)
- `an_assembling_candidate_unblocked_before_the_deadline_installs_without_a_second_render` (rev 4)
- `an_idle_output_releases_a_declined_result_without_a_later_render` (rev 4)
- `a_full_first_flush_retries_and_a_disabled_head_still_releases` (rev 4)
- `an_ordinary_poll_flush_closes_the_debt_as_transferred_once` (rev 5)
- `a_full_journal_on_source_release_retries_without_losing_the_row` (gate 3)
- `a_component_disconnect_after_submission_settles_every_obligation_once` (gate 3)
- `a_renderer_route_disconnect_keeps_the_row_until_qualified_retirement` (F3)
- `a_stale_result_from_a_superseded_admission_is_refused_whole` (gate 3)

Mutants: the walk skips rows on disabled heads (the disabled-head control
fails); the row closes its debt on `NoRelease` (the rev-5 control fails, or
a successor is admitted early); intake moves into the row walk (the
result-turn control fails, because installation needs another turn).

## The models against the implemented ordering

Each stage reruns the TLC configs its seams touch with the pinned jar
(README.md "Run"), with every config's expected exit (0, or 12 naming its
property): Transfer (credit, cancellation, duplicate ownership) for G3-1 and
G3-2; SourceRelease (source release and the journal record) for G3-1 and
G3-5; Fallback (the scene decision) for G3-3 and G3-5; OwnerTurn (returned
results, Retain and Release backpressure) for G3-4 and G3-5. The models are
unchanged. These runs check the design projections; they prove nothing about
the new Rust.

G3-5 adds `TRACE-MAP.md`, a table from each model action to the Rust call
that implements it. The harness records each mapped call as it happens, with
the job, the admission ticket, the grant epoch, the head's id and native
owner, and the outcome, enough to tell apart the transitions the models
guard. Each scenario asserts its record is a path the model permits. These
are scenario and order checks on real calls, not a refinement proof or
exhaustive trace validation. The trace and refinement obligation in
README.md stays open.

## Stopping conditions

The five in AUTHORITY-STEP3 hold, each with the stage that tests it:

| Condition | Stage |
| --- | --- |
| the renderer cannot return the pixels unshared | G3-2 |
| the key cannot be built without writing installed state | G3-3 |
| acceptance cannot be made infallible | G3-4 |
| the plan cannot stay opaque in sophia-runtime | G3-1 |
| an admitted job is postponed without bound | G3-2, G3-5 |

If one is reached, I record the exact blocking obligation and stop. None is
worked around with worker-side DRM, owner-side pixel work, cached liveness,
speculative unbounded memory or a new ring. A missing tracked-submission
fixture (G3-4) holds gate 3 in the same way.

## Verification for every stage

The isolated recipe holds: bwrap hiding /dev/dri and /dev/input with no
network, the SOPHIA_*, display, RUSTFLAGS and target variables removed,
stdin from /dev/null, umask 022, cargo-slot sophia, `timeout -s KILL`, and
the gate's `exit=` read from its log. Clippy runs feature-off and feature-on
for each touched crate. The new tests run feature-on with exact passed
counts. The 4c control suites run under both. Mutants run in
`~/dev/sophia-fjmut` with its own pool, each failing at its named assertion.
`cargo xtask check` and `cargo fmt --all --check` close each run. Touched src
files stay at or under 1000 lines, with tests mounted by path.

## Not in this scope

Gate 4's owner-loop integration, any native or GPU run, any CPU or
performance claim, a feature default, a wire or SDK change, a capability
bit, merge, push and install. The three re-entry extensions stay
unselected.
