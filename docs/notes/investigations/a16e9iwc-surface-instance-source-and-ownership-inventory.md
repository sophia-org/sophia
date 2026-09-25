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
