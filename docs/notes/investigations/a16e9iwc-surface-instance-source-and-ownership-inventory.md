---
id: a16e9iwc
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, rendering, ownership]
---
# Surface instance source and ownership inventory

## Question

Which Engine and renderer paths would a generic surface instance have to
join, and where does a second presentation of one source collide with,
collapse into, or get mistaken for the source itself? This informs the
t242 contract ([plan](../plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md),
[foundation](../../rendering-foundation.md)). It is an inventory and a
proposed internal seam only. The shared passive protocol types belong to
the contract.

Read on master 3b61790f. The preview prototype was read without
changes on branch `overview` (commits cf1c33ed, 46dfc4da).

## What exists, by path

Paths are under `crates/`.

- **Retained source.** `CommittedSurfaceState` (`sophia-protocol/src/packets/surface/transaction.rs`)
  holds one committed generation and the client's own geometry. Engine keeps
  it in `SurfaceVisualStateTable`, a map keyed by `SurfaceId`
  (`sophia-engine/src/visual_state.rs`).
- **Display list.** `CompositorDisplayCommand::Surface { surface }`
  (`sophia-engine/src/compositor_graphics.rs`) is a bare id: no geometry,
  clip, opacity or node identity. The builder
  `surface_chrome_display_list_for_surfaces` rejects a repeated source
  (`DuplicateSurface`).
- **Output snapshot and head plans.** `output_scene_snapshot_from_committed_in_view`
  (`sophia-engine/src/composition_plan.rs`) gathers displayed surfaces into
  a set, takes geometry from the committed state and fixes opacity at 1000.
  `validate_snapshot` rejects a duplicate. `HeadLayerBinding` and
  `HeadCompositorCommand::Surface` are keyed by the surface id, and
  `head_output_damage_snapshot` finds a layer by the first id match.
  Direct scanout requires one layer, so an instance correctly forces
  composition.
- **Lowering.** Native: `sophia-renderer-live/src/head_composition.rs` keeps
  an emitted set and rejects a duplicate, then finds the binding and the
  source by surface. CPU: `production_cpu_scene.rs` draws the committed
  state at its committed geometry, so a destination rectangle would be
  ignored.
- **Retained sources.** `LiveProductionRetainedCompositionSourceSet`
  (`sophia-backend-live/src/production_visual_runtime/compositor_graphics.rs`)
  collects sources from `Surface` commands only, one per id. Any command
  variant it does not visit gets no source: the omission the overview's
  `missing-preview-source` control found.
  `displayed_surfaces` and `surface_outputs` in `production_visual_runtime.rs`
  are maps from `SurfaceId`, the second to a single output, which cannot
  express one source shown on two outputs.
  `LiveCpuBufferLifetimeRegistry` is keyed by surface.
  Native frame retirement and mirror groups are keyed per frame, which
  already fits; the present scheduler still matches by surface.
- **Damage.** `output_frame_damage_snapshot` and `output_frame_damage`
  (`sophia-engine/src/frame/damage.rs`) are keyed by surface and reject a
  duplicate. Source damage enters only as the source's own clip; nothing
  maps it into another destination. Chrome damage is keyed by
  `CompositorNodeId`, which surface entries do not have.
- **Presented input.** `presented_input_layer_snapshots`
  (`production_visual_runtime/projection.rs`) makes an application input
  layer from every surface in the retired frame. An instance that reached
  that list would become an application target at the thumbnail
  rectangle. This is the main hazard.
- **WM boundary.** Translation groups (`sophia-engine/src/translation.rs`)
  overwrite a surface's committed geometry in place and admit each surface
  once, so they cannot give a second placement. The projection reducer
  (`policy_projection.rs`) validates, stages and commits proposals and
  rejects a duplicate surface.

## Where a second presentation of one source breaks

| Behaviour | Sites |
|---|---|
| Rejected as a duplicate | display-list builder, `validate_snapshot`, frame damage snapshot, native head composition, projection validation, translation groups |
| Collapsed or first match | snapshot's displayed set, head damage layer lookup, native binding and damage-size lookups, CPU scene lookup, `displayed_surfaces`, `surface_outputs`, CPU buffer registry, translation timeline |
| Mistaken for the source | presented input layers, the plan checksum (id and generation only) |

## Proposed internal seam

The source path stays as it is. An instance is a second, separate kind of
display entry, so no existing duplicate check has to be relaxed and no
input path can confuse the two.

1. **Display command.** `CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance)`
   with an instance id and instance generation, the source `SurfaceId`, a
   destination rectangle, a clip, and bounded opacity. Stacking is the
   command's position in the list, as for every other command. The
   instance gets a `CompositorNodeId`, so region chrome that Engine draws
   around it uses the existing node-keyed damage.
2. **Snapshot.** `OutputSceneSnapshot` gains `instances`, each carrying its
   source's committed generation. Validation requires unique instance ids
   and a source present in the committed table, whether or not that source
   is itself displayed: a preview-only source is valid.
3. **Head plans and lowering.** Bindings are keyed by a layer key that is
   either a surface or an instance. The CPU and native paths draw the
   source scaled into the destination and clipped. Native binds the source
   once and draws once per instance.
4. **Retained sources.** Collection visits `Surface` and `SurfaceInstance`
   and takes the union by source id. Repeated sources share one lease;
   each instance keeps its own geometry and damage. The source lease covers
   queued rendering and copying. Native backings stay owned through
   submission, display and each mirrored head's retirement, on the
   existing per-frame retirement owners. `surface_outputs` becomes a set of
   outputs per source.
5. **Damage.** Instance state is keyed by instance id. A change of
   destination, clip or opacity damages the old and the new rectangle. A
   change of the source's committed generation damages the destination
   within the clip, scaled from the source's damage. It leaves the instance
   generation, which is the interaction identity, unchanged.
6. **Input.** Presented input layers are built from surfaces only. An
   instance never becomes an application target. Policy action targets on
   instances are the contract's input half and are published separately.
7. **Bounds.** Instances count against the existing per-frame surface
   budget. A shared source does not make an instance free.

## Controls to keep

- The overview's production test `preview_only_updates_damage` and its
  two negative controls, `missing-preview-source` and
  `missing-source-generation`, recorded in
  `~/.local/state/hagia/development-evidence/h002-preview-20260925`.
  Each seam above gets its own red test in the same shape.
- New negative controls: an instance must never appear among presented
  input layers, and a source generation change must not change the
  instance generation.

## First source checkpoint (t244)

The contract is [WM presentation](../../wm-presentation.md). This
checkpoint joins surface instances through the Engine display list, the
output snapshot, head plans, native and CPU lowering, frame damage and
retained source ownership. Regions, ReplaceApplications suppression and
the presentation's input half are not in it.

- **Identity.** `CompositorSurfaceInstance` carries the WM's opaque id
  and generation, qualified by the admitted connection epoch as
  `CompositorNodeId::PolicyInstance`. The same id under a new epoch is a
  distinct node. `Surface` is unchanged, so every existing duplicate
  check still guards the source's own presentation.
- **Source generation.** `resolve_surface_instance_sources` sets each
  instance's source generation from the committed table at capture. A
  source without committed content refuses the whole list with
  `CompositorMissingInstanceSource`; no instance is dropped. The snapshot
  refuses a stale generation (`StaleInstanceSource`) and frame damage
  refuses an unresolved or stale one.
- **Missing and removed sources.** `set_policy_presentation` refuses a
  whole candidate if any source lacks committed content in the displayed
  or committed scene (`LivePolicyPresentationRefusal::MissingSource`), and
  the last valid presentation stays. Removing a sampled source revokes the
  whole publication and records one `LivePolicyPresentationRevocation`
  for its owner. Frame capture draws the tier whole or not at all.
- **Preview-only sources.** The snapshot samples an instance's source
  whether or not it is presented on that output. It does not add the
  source to the presentation order, draw it at its own placement, or
  make it a frame surface.
- **Repeated sources.** Head plans keep one binding per source. Native
  lowering draws that binding once per instance at the instance's
  destination, clip, opacity and sampling. Retained source collection
  takes the union of `Surface` and `SurfaceInstance` sources by id, and
  CPU variant layers use the presentation order plus instance sources.
- **Damage.** Instance damage is keyed by node. A changed placement,
  opacity, generation or resolved source generation damages the visible
  rectangle it had and has. A change of order among instances damages
  every instance involved. A source commit also damages each instance's
  visible rectangle on that output, not the hidden source placement.
- **Input.** Head damage snapshots carry instances as display commands,
  never as frame surfaces, so presented input layers cannot include one.
- **CPU opacity.** The CPU instance path follows the native composition
  shader. Premultiplied colour is clamped to alpha, colour and alpha are
  scaled once by the opacity, and the result is composed over the frame.
  At full opacity an unscaled instance matches an ordinary layer byte
  for byte.
- **Tier.** The runtime holds an admitted `LivePolicyPresentation`, whose
  admission belongs to t243. The output list places its instances in z
  order after the floating outline and before shell content.

Guards: `crates/sophia-engine/tests/surface_instances.rs`,
`crates/sophia-renderer-live/tests/cpu_instance_opacity.rs`, and the
production test
`preview_only_instances_share_a_source_until_copy_and_backings_until_retirement`
and `a_missing_source_refuses_the_presentation_whole_and_removal_revokes_it_whole`.
The production test ports the overview prototype's
`preview_only_updates_damage` onto the generic presentation, with two
instances of one preview-only source.

Negative controls, run on this checkpoint with evidence in the
worktree's `.artifacts/t244-controls/controls.json`. They correspond to
the archived prototype's controls (overview 914858fa):

| Control | Change | Result |
|---|---|---|
| missing-instance-source | retained collection ignores instance sources | fails: `MissingCpuSource(55)` |
| missing-source-generation | display list skips source resolution | fails: instance source generation stays 0 |
| admits-missing-source | admission skips the missing-source refusal | fails: the refused candidate is installed |
| removal-not-revoked | removal skips revocation | fails: the presentation outlives its source |

## Second checkpoint: regions, replacement and the presented stamp

- **Regions.** Regions are `CompositorNodeId::PolicyRegion { owner_epoch,
  id }`, drawn with the existing commands inside the region's clipped
  allocation. Backdrop is a `Rect` in the frame colour at full opacity.
  Frame is a `Border` with the frame stroke. Emphasis is a `Border` with
  the focus-ring stroke. Regions and instances on an output are merged in
  one z order.
- **ReplaceApplications.** On an output in that mode, the tier replaces
  that output's application surfaces, their chrome, tab bars and the
  floating outline. They leave the frame's surfaces, so they are not hit
  targets there. Clients keep their allocations and content, a preview
  still samples them, and withdrawal restores them. The mode applies
  only while the tier is drawn.
- **Stamp.** Each output list with an output record starts its tier with
  a pixel-less `PresentationStamp`. It carries the owner epoch,
  publication generation, output, output generation and coverage, and it
  flows through the snapshot, the head plan (coverage projected) and the
  head damage snapshot. The frame that retires therefore names the
  publication it presents.
  `LivePresentedPolicyPublication::from_presented_frame` reads that
  publication, with each drawn instance's and region's `(id, generation)`,
  from a completed frame only. It is separate from the requested
  `policy_presentation()`. A change of stamp damages the old and new
  coverage. A binding-only publication with identical pixels therefore
  still presents and retires, and a withdrawal presents a frame without a
  stamp. A source repaint changes no presented identity.

Tests: `presentation_instances.rs` holds the two earlier production tests
and three new ones:
`regions_and_instances_share_one_z_order_under_one_stamp`,
`replace_applications_substitutes_the_tier_for_that_outputs_applications_only`
and
`a_retired_frame_names_the_publication_it_presents_and_a_repaint_keeps_its_identities`.

| Control | Change | Result |
|---|---|---|
| replacement-not-suppressed | the mode is ignored | fails: the application is drawn on the replaced output |
| stamp-missing | the tier omits its stamp | fails: the retired frame names no publication |
| stamp-change-undamaged | a stamp change damages nothing | fails: a binding-only change has no damage |

Further coverage on the same checkpoint series:
`an_instance_samples_a_source_that_another_output_presents` covers a
source that another output owns. It keeps its placement and
`surface_outputs` entry, the second output draws it, the first samples
it, and both lower one layer from one lease.
`an_instance_forces_composition_and_a_stamp_alone_does_not` covers the
direct-scanout fallback.
`an_instance_is_projected_through_a_fractional_head_and_its_repaint_widened`
covers a 1.5 scale head. Mirrored heads settle ownership in the
preview-only production test, which flips the sibling head before the
primary.

## Scaled sampling: aligned with the native contract

The native renderer's sampling contract is `sharp_reconstruction.frag`
and `composition.frag`, selected by source-to-target size (exact nearest
at identity, sharp reconstruction when scaled). Its headless reference
model now lives in
`crates/sophia-renderer-native-egl/tests/support/reference_sampling.rs`.
It was moved unchanged out of that crate's `tests/sampling.rs`, which
still runs its shader-text contracts against it, and it remains test
code.

The CPU instance path (`cpu_composition/scaled.rs`) now follows that
contract:
- **Selection.** It uses the Engine's `head_sampling_class`, the same
  classification the head plan uses. Identity takes exact texels, and at
  full opacity XRGB texels are copied verbatim.
- **Reconstruction.** At any other scale it runs a CPU port of
  `sharp_reconstruction.frag`: a 4x4 Catmull-Rom over pixel centres with
  clamp-to-edge taps, gamma-2 light conversion (unpremultiplied for
  premultiplied sources), alpha clamped before the encode, premultiplied
  colour clamped to alpha, then opacity on colour and alpha.
- **Why a port.** No production Rust implementation existed to reuse; the
  GLSL was the only one.

`crates/sophia-renderer-live/tests/cpu_instance_sampling.rs` includes the
native reference model as test code. It compares every frame pixel
(inside the clipped destination, untouched black outside) to within one
byte step for upscale, downscale, fractional, mixed, a hard luminance
edge, a clipped offset destination, translucent premultiplied at partial
opacity, and opaque at partial opacity. It also checks identity-scale
exact texels, and a hard alpha edge over a grey backdrop within 1.5
steps.
`identity_scale_opacity_matches_the_native_reference_model` keeps the
opacity cross-check.

| Control on the CPU port | Result |
|---|---|
| sampling-in-gamma: taps filtered without light conversion | fails 7 of 9 (worst 83 steps) |
| sampling-nearest: nearest texel instead of reconstruction | fails 8 of 9 (worst 124 steps) |
| opacity-twice: opacity applied twice | fails 2 of 9 (worst 57 steps) |
| no-encode-clamp: alpha not clamped before the encode | passes: equivalent at byte precision, since the final byte encode clamps to [0, 255]; kept to mirror the shader |

Physical limit: these controls compare the CPU port with the reference
model, and the native tests compare the model with the shader text.
Neither establishes what a particular GPU and driver produce. No physical
run is claimed, and none was made for this work.

A source commit damages each instance's whole visible rectangle rather
than the source's damage scaled into the destination. That is the same
whole-placement policy ordinary surfaces follow in the snapshot. With
reconstruction, one changed texel affects up to two texels around it, so
whole-rectangle damage stays conservative.

## Boundary review: stamps, mirrors, revocation, withdrawal

A focused review of the joined contract and renderer boundary found the
following.

- **Mirrored completion (fixed).** The native target reported only the
  primary head's retired frame, so a publication read as presented while
  a mirror head still showed the previous one. `presented_head_frames`
  now reports each head's retired frame, primary first
  (`presented_output_head_frames` in the native target).
  `LivePresentedPolicyPublication::from_presented_heads` yields the
  primary's publication only once every head has retired a frame with the
  same stamp identity (owner epoch, publication generation, output,
  output generation). Guard:
  `a_mirrored_output_presents_a_publication_only_once_every_head_has`.
  Control primary-head-only fails.
- **Mirror damage (fixed).** The mirror-copy damage projection rescaled
  surfaces and borders only. It now also rescales instance destinations
  and clips, rects (region backdrops) and stamp coverage. A rect or
  instance that projects to nothing on a head is dropped from that head's
  damage record, since the damage ledger would refuse it; borders keep
  their established rounding. Guard:
  `mirror_damage_projection_places_instances_regions_and_stamp_coverage`.
- **Source revocation (fixed).** The batch paths revoked a presentation
  whose source was removed, but the public prepare paths
  (`prepare_authority_transactions`, `prepare_authority_groups`, and
  `run_authority_transactions` through them) committed removals without
  revoking. Both now revoke. Guard:
  `a_prepared_removal_of_a_sampled_source_revokes_the_presentation`.
- **ReplaceApplications withdrawal (verified).** Withdrawing damages the
  stamp's coverage and the restored application's placement, and the
  application is a frame surface, and so a hit target, again. This is
  asserted in
  `replace_applications_substitutes_the_tier_for_that_outputs_applications_only`.
- **Absent stamps.**
  - Every native path builds its lists through the output composition, so
    it carries the stamp. That covers retained, ordinary, Present,
    translation and configuration repaints.
  - A head's frame record is queued from the lowered frame's snapshot,
    which direct scanout does not replace. This is established by code
    reading and not separately tested.
  - An output without an output record, or a tier withheld for a missing
    source, has no stamp, which reads as not presented.
  - Open, for a decision: the software-only composition path
    (`production_cpu_cycle.rs`, used without native scanout) draws
    application surfaces and chrome only. It never draws the tier or a
    stamp, and the committed-fallback input projection still lists
    application layers under ReplaceApplications. The failure is safe, since
    no stamp means no receipt, but a presentation there never completes.
    Refusing the capability without native scanout, or drawing the tier on
    that path, is the director's choice.

## Incident: the real-card smoke ran during a suite

On 2026-09-25, two `cargo test --offline -p sophia-backend-live
--all-features` runs from this worktree executed
`atomic_scanout_hardware_smoke`, the real primary-card atomic scanout
smoke, while the operator's live session on `:77` held the card.

- **Trigger.** The live session's launch command exports
  `SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1` (its parent process runs
  `env ... SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1 ... sophia session run
  --display=:77 --native-scanout ...`). The agent's shell inherits it, and
  those two commands did not clear it. The gate chains and other suite
  commands that day did unset it.
- **Source.** Both runs built the tree committed as 934aab9c ("Draw WM
  regions, replace applications and stamp presented publications"). The
  first ran just before that commit, on the identical working tree. The
  second was a rerun on the committed tree to identify the failures. The
  test binary `libdrm_events_feature-1b36adee45dbaf8c` was built at
  09:11:17 -0400.
- **Outcome.** In both runs the child test
  `native_atomic_scanout_real_primary_card_child` failed its assertion
  that the smoke evidence status is `Passed`, with `left:
  AtomicSubmitFailed`. The parent test
  `native_atomic_scanout_smokes_real_primary_card_when_enabled` reported
  `real atomic scanout smoke child failed with status exit status: 101`.
  The atomic submit was refused because the live session holds the card,
  and the `:77` session process was still running afterwards. The full
  output of the runs was not saved; the lines quoted here are the
  captured excerpt.
- **Correction.** Removing the flag alone is not isolation. Backend and
  session suites now run inside the same device-hidden sandbox that
  `xtask native-protocol-family` uses for its stages: bwrap with a
  minimal `/dev` (no DRM nodes), an empty `/run/user`, a private `/tmp`,
  and the display, socket, config and smoke variables cleared. No further
  physical probe or live-session action is part of this work.
