# Gate 3 scope: the device-free prototype of the combined frame job

Status: scope for review by Codex before any gate-3 source. It selects no
source by itself. It stays inside niltempus's selections of 2026-11-11 in
AUTHORITY-STEP3.md: the full scope staged, the narrow F4/R5 eligibility set,
and the failure-terminal baseline with none of the three re-entry extensions.
Base: design/combined-frame-job at 2048dc2c3, which holds the accepted 4c-4
scaffold (0f29023d8).

n7fj3c2p gate 3 asks for a test-only prototype that uses the real capture,
validation and journal seams with a deterministic renderer. It must keep
today's full-journal, disconnect and stale-result behaviour, and it must
prove that the normal path has no intermediate validation return to the
owner. Gate 4, a default-off integration into the real owner loop, is not
part of this scope.

## What "test-only" means here

The composed path runs only from tests. New source sits behind
`cfg(any(test, feature = "experimental-combined-frame-job"))`, as the 4c
stages did, because the seams cross four crates and a cfg(test) item in one
crate is not visible to another crate's tests. No production caller is
added: `physical_input_loop.rs`, `component_service.rs` and
`native_service.rs` are unchanged, and nothing on the real owner turn
reaches a combined entry. The device-free backend pieces the session
harness needs are exported the way `session_content_fixture` already is,
behind `test-support`, `doc(hidden)`, and here also the combined feature.
Feature-off builds compile to today's code, and each stage shows that with
feature-off clippy and the unchanged control suites.

sophia-renderer-live gains its own `experimental-combined-frame-job`
feature, enabled from sophia-backend-live's feature of the same name. That
feature and its one-line propagation are the only Cargo changes in gate 3.

If Codex reads gate 3 as cfg(test)-only in a stricter sense, the session
harness cannot be built, and the scope would stop at the per-crate stages
G3-1 to G3-4. That is the first question for review.

## Stages

Five source stages follow this document. Each has its own frozen signed
source, sealed source review before any build, Codex read, isolated
validation with exact test counts and named mutants, and acceptance. G3-1
and G3-2 do not depend on each other. G3-3 needs G3-1's placeholder.
G3-4 needs G3-3's frame. G3-5 needs all four.

### G3-1: the renderer binds validated pixels after the slot (F4, R3)

Places: sophia-renderer-live `cpu_composition/pixel_storage.rs`; backend
`exporter/worker/combined.rs` and a new `exporter/worker/combined_render.rs`.
`worker.rs` is at 997 lines and `service.rs` at 878, so neither grows beyond
a call site.

`LiveCpuPixelStorage` gains `Validated(Arc<ValidatedIsbPixels>)`, behind the
renderer-live feature. Its bytes are `ValidatedCpuPixels::bytes()`, its
`pixel_version` is `None` (the bound layer is always uploaded; the scene key
never reads it, because the key holds the placeholder), and dropping it
frees memory or unmaps a sealed mapping only.

The placeholder is not a layer. `CombinedRenderPlan` gains
`placeholder: CombinedPlaceholder { index, placement, size, stride, format,
extent }`, and the plan's `PendingRenderedFrame` holds every other layer.
Binding inserts one `Cpu` layer with `Validated` storage at `index`. I chose
this over a new layer variant because a placeholder kept outside the list
cannot reach a renderer unbound, and because a new variant of
`LiveOwnedMixedCompositionLayer` would change every exhaustive match on it.
If Codex prefers the variant, the renderer must refuse it unbound; that
changes G3-1 and G3-3 only.

`OutputRoute::serve`'s closure takes the pixels with the plan. The combined
attempt runs R3's order in `combined_render.rs`:

1. deadline and cancellation, as `serve` checks today
2. `frame_slots.try_acquire()`: `Deferred` returns `NoSlot(plan, pixels)`
   unbound, and `serve` parks both; `IncarnationExhausted` is a terminal
   failure that returns the pixels unbound
3. compare the validated size, stride and extent with the placeholder; a
   mismatch is a terminal failure (Malformed), the slot is released, and
   the pixels return unbound
4. wrap the pixels in an `Arc`, bind, and render in the slot through the
   render function the caller supplies
5. drop the frame, then `Arc::try_unwrap`; a surviving reference stops the
   lane (stopping condition 1) and is never worked around

Step 4's render function is the deterministic-renderer seam. Production
passes `render_frame_in_slot` with the real context. Tests pass a function
that reads the bound layer's bytes through the frame it was given and
returns an `Exported` outcome whose correlation carries a checksum of those
bytes. The slot table, slot release on failure, slot damage invalidation,
parking and the result route are the real ones.

Controls:

- `the_renderer_returns_the_exact_pixels_with_a_strong_count_of_one`
- `three_busy_slots_park_the_job_unbound_and_one_release_renders_it_once` (R3)
- `expiry_and_cancel_while_parked_return_plan_pixels_and_credit_without_unwrap` (R3)
- `a_description_mismatch_fails_terminally_and_returns_the_pixels_unbound`
- `an_exhausted_incarnation_returns_the_pixels_unbound`
- `a_failed_render_releases_and_invalidates_its_slot`
- `the_deterministic_renderer_saw_the_bound_bytes` (checksum equality)
- `dropping_a_plan_on_the_renderer_thread_frees_memory_only` (F4 destruction)

Mutants: bind before acquire (the busy-slot control must fail at its
"unbound" assertion); skip the description check; unwrap before dropping the
frame (must fail at the strong-count assertion, not panic outside it).

### G3-2: one admission, one reconciliation, in the runtime (F3, R1, rev 4)

Places: `shell_content/epoch_registry/owned_ring.rs` `reserve_copy` and
`owned_ring/commit.rs` `reserve_frame_commit`;
`shell_transport/owned_copy/admission.rs` `submit_reserved_copy`;
`shell_content/isb_jobs/owned_copy.rs`; `shell_transport/owned_copy.rs`;
the candidate store's read-only `frame_candidate_state`;
`shell_transport/image_owner.rs`.

- `reserve_copy` takes a reservation mode. Ordinary calls `worker.reserve`;
  combined calls `worker.reserve_combined`, and the returned job becomes the
  `worker_job`. `begin_isb` failure releases it as today.
  `FrameAdmissionReservation` carries the `CombinedReservation` out.
- `submit_reserved_copy` keeps its four steps and order. Combined mode
  changes only the send: `OwnedCopyImageJobs::submit_combined` makes
  `submit`'s binding check, then `worker.submit_ring_combined`. The plan is
  `Option<P>`; `None` (plan build failed) takes the refused-send branch with
  `Work::Working` present, so `reconcile`'s `expect` holds.
- `receive_forwarded(job, pixels) -> Result<(), ValidatedIsbPixels>` matches
  `worker_job == job && submitted && result.is_none()`, with no condition on
  `stopped` (rev 4). `Err` keeps the pixels with the caller.
- `frame_candidate_state(binding, job)` returns Assembling, Pending,
  Submitted, Ended or Unknown exactly as rev 4 defines them, read-only.
- `ShellIsbImageOwner<P = DefaultPlan>` and its owned-copy part take the
  defaulted generic plan, so existing callers compile unchanged.

Controls, all on the real owned ring, real validator thread and real journal:

- `a_combined_admission_spends_one_credit_and_binds_the_same_job` (F3)
- `a_combined_submission_reaches_receive_forwarded_reconcile_and_source_release` (R1)
- `a_foreign_ticket_is_refused_and_returned` (R1)
- `a_refused_send_leaves_no_orphan_row_and_publishes_the_source_release` (R1)
- `a_plan_build_failure_takes_the_refused_send_branch` (R1)
- `malformed_validation_and_inbox_refusal_return_the_plan_once` (F3 table)
- `begin_isb_failure_releases_the_combined_reservation` (F3 table)
- `a_withdrawn_job_keeps_its_charges_until_complete_isb_consumes_late_pixels` (rev 4)
- `an_unknown_or_mismatched_receive_keeps_the_charges_and_the_pixels` (rev 4)
- `a_full_journal_keeps_the_releasing_entry_until_retry` (F3, gate 3 full-journal)
- `candidate_state_reports_assembling_while_the_copy_gate_is_open` (rev 4)
- `candidate_state_reports_ended_only_from_a_later_generation_or_closed_stream` (rev 4)
- `a_duplicate_result_and_a_replayed_job_create_no_candidate_obligation` (F5)

Mutants: combined mode also calls `worker.reserve` (two credits; the F3
control fails); `receive_forwarded` rejects stopped jobs (the withdrawal
control fails); `frame_candidate_state` returns Ended on absence from
pending and submitted (the assembling control fails).

### G3-3: provisional composition and the scene key (F1, F4, R5)

Places: the backend visual runtime, `production_visual_runtime/` beside
`compositor_graphics/shell_content.rs`, in a new `combined_scene.rs`.

- `compose_provisional_head_frame(head, candidate) -> Result<ProvisionalFrame,
  Ineligible>` lowers the head's current composition with the candidate's
  image as the placeholder. It reads installed state and writes none of it.
  This is the check for stopping condition 2: the test snapshots every
  installed field it reads and compares after.
- `CombinedSceneKey` is F1's exact list. It needs one read-only accessor for
  the lease's `CpuPixels.version`.
- Eligibility is F4 plus R5, checked on metadata before any reservation: Cpu
  or Solid layers only, empty `image_reads`, Cpu storage Shared, Content,
  Bytes or Lock, a plane-only cursor, one head, an idle worker, no capture,
  and a known `pixel_version` on every Cpu layer. A text or indicator layer
  whose raster key cannot be read without rasterizing is ineligible.
- `recompose_scene_key(head)` at consumption is metadata only and returns
  Match, CandidateFailure, TopologyChange or SceneChurn (F1, R4).
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
- `each_disallowed_layer_class_and_image_reads_are_ineligible` (F4)
- `an_unversioned_cpu_layer_is_ineligible` (R5)
- `an_unreadable_raster_key_is_ineligible_without_rasterizing` (R5)
- `provisional_composition_writes_no_installed_state`
- `the_install_preflight_refuses_without_changing_state`

Mutants: the key omits the resource id (the same-generation control fails);
two `None` versions compare equal (the R5 control fails); provisional
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

These need a device-free head and exporter that reach the tracked submission
statuses. `native_prepared_cancellation.rs` and
`native_renderer_deferral.rs` drive device-free heads today; the source
review pins which fixture reaches `SubmittedWaitingForPageFlip`. If none
does without a device, G3-4 reports that gap and covers the statuses it can
reach; it does not fabricate a submission.

Mutants: commit before the damage-queue preflight (the refused-preflight
control fails with partial state); `poll()` reads the ordinary channel for a
combined request (the success control fails).

### G3-5: the owner row and the end-to-end prototype (R2, rev 4, rev 5)

Places: sophia-session `live_session/metadata_shell/content/combined/`
(new), under the feature, with no call from the owner loop; the session
harness under `tests/support/combined_frame/`, built on
`component_files::Harness` (a real connected component, real ring capture
and real journal), the real `ShellIsbImageOwner` with its validator thread,
and a backend `combined_frame_fixture` exported like
`session_content_fixture`, carrying G3-1's deterministic renderer on a real
renderer worker thread.

The owner row holds the runtime reservation, the `CombinedHead`, the owner
plan receiver, the admission deadline and the Release debt. One test-only
driver runs the service sequence of AUTHORITY-STEP3 in the order a real
owner turn would: component service with the image owner first, then the
content service, then the row walk, then native service. Gate 4 replaces
that driver with the real owner loop; the row and its methods do not change.

Controls:

- `the_normal_path_has_no_intermediate_validation_return`. Between the
  submission and the renderer's result wake, the validator's readiness
  descriptor stays quiet, no owner turn takes a validation completion, and
  the owner runs exactly the service-1 and service-2 turns. This uses the
  stage-4a checkpoint pattern (WAKE-SEAM.md) on the real validator.
- `an_unchanged_scene_is_accepted_and_reaches_tracked_submission`
- `scene_churn_retains_the_result_and_renders_conventionally_once` (FALLBACK.md)
- `a_renderer_failure_or_expiry_ends_in_the_failure_terminal` (R4 baseline)
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
a successor is admitted early); the driver takes the validation completion
before the result (the no-intermediate-return control fails).

## The models against the implemented ordering

Each stage reruns the TLC configs its seams touch with the pinned jar
(README.md "Run"), with every config's expected exit (0, or 12 naming its
property): Transfer (credit, cancellation, duplicate ownership) for G3-1 and
G3-2; SourceRelease (source release and the journal record) for G3-2 and
G3-5; Fallback (the scene decision) for G3-3 and G3-5; OwnerTurn (returned
results, Retain and Release backpressure) for G3-4 and G3-5. The models are unchanged; this checks that the
accepted controls still hold, not that the Rust refines them.

G3-5 adds `TRACE-MAP.md`, a table from each model action to the Rust
function that implements it. The harness records the order of those
functions in each scenario, and each scenario asserts that order is a path
the model permits. These are recorded orders of real calls, not fabricated
traces. A Trace.tla refinement check is not in this scope; it stays the open
gate README.md names.

## Stopping conditions

The five in AUTHORITY-STEP3 hold, each with the stage that tests it:

| Condition | Stage |
| --- | --- |
| the renderer cannot return the pixels unshared | G3-1 |
| the key cannot be built without writing installed state | G3-3 |
| acceptance cannot be made infallible | G3-4 |
| the plan cannot stay opaque in sophia-runtime | G3-2 |
| an admitted job is postponed without bound | G3-1, G3-5 |

If one is reached, I record the exact blocking obligation and stop. None is
worked around with worker-side DRM, owner-side pixel work, cached liveness,
speculative unbounded memory or a new ring.

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
