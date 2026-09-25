//! WM surface instances (t244): a second, Engine-drawn presentation of a
//! committed source. Instance and source identities stay separate: repeated
//! sources share one sampled source but keep their own geometry and damage,
//! a preview-only source is sampled without being presented or becoming an
//! input layer, and the source's committed generation drives repaint without
//! changing the instance's interaction generation.

use sophia_engine::*;
use sophia_protocol::*;

const OUTPUT: OutputId = OutputId::from_raw(1);

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn output() -> HeadlessOutput {
    HeadlessOutput {
        id: OUTPUT,
        size: Size {
            width: 1280,
            height: 720,
        },
        scale: 1,
    }
}

fn committed(surface: SurfaceId, generation: u64, geometry: Rect) -> CommittedSurfaceState {
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    CommittedSurfaceState {
        surface,
        committed_generation: generation,
        geometry,
        content: SurfaceContentSet::singleton(
            BufferSource::CpuBuffer {
                handle: u64::from(surface.index()),
            },
            size,
        ),
        damage: Region::single(rect(0, 0, size.width, size.height)),
    }
}

fn instance(
    owner_epoch: u64,
    id: u64,
    source: SurfaceId,
    destination: Rect,
) -> CompositorSurfaceInstance {
    CompositorSurfaceInstance {
        owner_epoch,
        id,
        generation: 1,
        source,
        source_generation: 0,
        destination,
        clip: destination,
        opacity_millis: 1_000,
    }
}

fn list(
    commands: Vec<CompositorDisplayCommand>,
    committed: &[CommittedSurfaceState],
) -> CompositorDisplayList {
    let mut list = CompositorDisplayList {
        output: OUTPUT,
        commands,
    };
    resolve_surface_instance_sources(&mut list, committed).unwrap();
    list
}

fn target() -> HeadRenderTarget {
    HeadRenderTarget {
        head: RenderHeadId::from_raw(1),
        output: OUTPUT,
        target_generation: 3,
        native_size: Size {
            width: 1280,
            height: 720,
        },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    }
}

fn plan(committed: &[CommittedSurfaceState], list: CompositorDisplayList) -> HeadCompositionPlan {
    let snapshot =
        output_scene_snapshot_from_committed(output(), 9, committed, list, None).unwrap();
    build_head_composition_plan(&snapshot, target()).unwrap()
}

fn instances(plan: &HeadCompositionPlan) -> Vec<CompositorSurfaceInstance> {
    plan.compositor
        .iter()
        .filter_map(|command| match command {
            HeadCompositorCommand::SurfaceInstance(instance) => Some(*instance),
            _ => None,
        })
        .collect()
}

#[test]
fn repeated_sources_share_one_sampled_source_and_keep_distinct_instances() {
    let source = SurfaceId::new(4, 1);
    let committed = [committed(source, 5, rect(0, 0, 400, 300))];
    let list = list(
        vec![
            CompositorDisplayCommand::Surface { surface: source },
            CompositorDisplayCommand::SurfaceInstance(instance(
                7,
                1,
                source,
                rect(500, 20, 200, 150),
            )),
            CompositorDisplayCommand::SurfaceInstance(instance(
                7,
                2,
                source,
                rect(800, 20, 100, 75),
            )),
        ],
        &committed,
    );
    let plan = plan(&committed, list);
    assert_eq!(
        plan.layers.len(),
        1,
        "one sampled source for all three presentations"
    );
    let drawn = instances(&plan);
    assert_eq!(drawn.len(), 2);
    assert_eq!(drawn[0].destination, rect(500, 20, 200, 150));
    assert_eq!(drawn[1].destination, rect(800, 20, 100, 75));
    assert!(drawn.iter().all(|instance| instance.source_generation == 5));
    let frame = head_output_damage_snapshot(&plan);
    assert_eq!(
        frame.surfaces.len(),
        1,
        "only the source's own presentation is a frame surface"
    );
    assert_eq!(frame.surfaces[0].logical_geometry, rect(0, 0, 400, 300));
    assert_ne!(plan.direct_scanout, DirectScanoutVerdict::Eligible);
}

#[test]
fn a_preview_only_source_is_sampled_but_never_presented_or_an_input_layer() {
    let source = SurfaceId::new(5, 1);
    // Placed off this output, and not in its presentation order at all.
    let committed = [committed(source, 2, rect(-1000, -1000, 64, 64))];
    let list = list(
        vec![CompositorDisplayCommand::SurfaceInstance(instance(
            7,
            1,
            source,
            rect(10, 10, 32, 32),
        ))],
        &committed,
    );
    let snapshot =
        output_scene_snapshot_from_committed(output(), 9, &committed, list.clone(), None).unwrap();
    assert_eq!(
        snapshot.surfaces.len(),
        1,
        "the preview-only source is sampled"
    );
    assert!(
        !snapshot
            .display_list
            .commands
            .iter()
            .any(|command| matches!(command, CompositorDisplayCommand::Surface { .. })),
        "its own placement is not presented"
    );
    let plan = build_head_composition_plan(&snapshot, target()).unwrap();
    assert_eq!(plan.layers.len(), 1);
    assert_eq!(instances(&plan).len(), 1);
    assert!(
        head_output_damage_snapshot(&plan).surfaces.is_empty(),
        "no application input layer"
    );
    let frame = output_frame_damage_snapshot(output(), list, &committed, None).unwrap();
    assert!(
        frame.surfaces.is_empty(),
        "no application input layer on the logical path either"
    );
}

#[test]
fn moving_one_instance_damages_only_its_own_rectangles() {
    let source = SurfaceId::new(4, 1);
    let committed = [committed(source, 5, rect(-500, 0, 400, 300))];
    let first = instance(7, 1, source, rect(100, 100, 200, 150));
    let second = instance(7, 2, source, rect(600, 100, 200, 150));
    let before = list(
        vec![
            CompositorDisplayCommand::SurfaceInstance(first),
            CompositorDisplayCommand::SurfaceInstance(second),
        ],
        &committed,
    );
    let moved = CompositorSurfaceInstance {
        destination: rect(650, 120, 200, 150),
        clip: rect(650, 120, 200, 150),
        generation: 2,
        ..second
    };
    let after = list(
        vec![
            CompositorDisplayCommand::SurfaceInstance(first),
            CompositorDisplayCommand::SurfaceInstance(moved),
        ],
        &committed,
    );
    let before = output_frame_damage_snapshot(output(), before, &committed, None).unwrap();
    let after = output_frame_damage_snapshot(output(), after, &committed, None).unwrap();
    let damage = output_frame_damage(Some(&before), &after).unwrap();
    assert!(!damage.is_empty());
    for damaged in &damage.rects {
        assert!(
            *damaged == rect(600, 100, 200, 150) || *damaged == rect(650, 120, 200, 150),
            "{damaged:?} is not the moved instance's old or new rectangle"
        );
    }
}

#[test]
fn a_source_content_change_repaints_its_instances_without_changing_their_generation() {
    let source = SurfaceId::new(5, 1);
    let shown = instance(7, 1, source, rect(10, 10, 32, 32));
    let first = [committed(source, 1, rect(-1000, -1000, 64, 64))];
    let second = [committed(source, 2, rect(-1000, -1000, 64, 64))];
    let before = list(
        vec![CompositorDisplayCommand::SurfaceInstance(shown)],
        &first,
    );
    let after = list(
        vec![CompositorDisplayCommand::SurfaceInstance(shown)],
        &second,
    );
    let resolved = after.surface_instances().next().unwrap();
    assert_eq!(
        resolved.generation, shown.generation,
        "the interaction generation is the WM's"
    );
    assert_eq!(
        resolved.source_generation, 2,
        "Engine resolved the source's commit"
    );
    let before_frame = output_frame_damage_snapshot(output(), before, &first, None).unwrap();
    let after_frame = output_frame_damage_snapshot(output(), after.clone(), &second, None).unwrap();
    let damage = output_frame_damage(Some(&before_frame), &after_frame).unwrap();
    assert_eq!(damage.rects, vec![rect(10, 10, 32, 32)]);
    let snapshot = output_scene_snapshot_from_committed(output(), 9, &second, after, None).unwrap();
    assert!(
        snapshot
            .logical_damage
            .rects
            .contains(&rect(10, 10, 32, 32))
    );
    assert!(
        !snapshot
            .logical_damage
            .rects
            .iter()
            .any(|damaged| damaged.x < 0),
        "the hidden source placement is not damaged"
    );
}

#[test]
fn an_unresolved_or_stale_source_generation_is_refused() {
    let source = SurfaceId::new(5, 1);
    let committed = [committed(source, 2, rect(0, 0, 64, 64))];
    let unresolved = CompositorDisplayList {
        output: OUTPUT,
        commands: vec![CompositorDisplayCommand::SurfaceInstance(instance(
            7,
            1,
            source,
            rect(100, 10, 32, 32),
        ))],
    };
    assert_eq!(
        output_frame_damage_snapshot(output(), unresolved.clone(), &committed, None),
        Err(OutputFrameDamageError::InvalidCompositorDisplayList)
    );
    assert_eq!(
        output_scene_snapshot_from_committed(output(), 9, &committed, unresolved, None),
        Err(HeadCompositionPlanError::InvalidSnapshot)
    );
    let stale = CompositorDisplayList {
        output: OUTPUT,
        commands: vec![CompositorDisplayCommand::SurfaceInstance(
            CompositorSurfaceInstance {
                source_generation: 1,
                ..instance(7, 1, source, rect(100, 10, 32, 32))
            },
        )],
    };
    assert_eq!(
        output_scene_snapshot_from_committed(output(), 9, &committed, stale.clone(), None),
        Err(HeadCompositionPlanError::StaleInstanceSource)
    );
    assert_eq!(
        output_frame_damage_snapshot(output(), stale, &committed, None),
        Err(OutputFrameDamageError::InvalidCompositorDisplayList)
    );
}

#[test]
fn a_missing_source_refuses_the_whole_list_instead_of_dropping_one_instance() {
    let present = SurfaceId::new(4, 1);
    let missing = SurfaceId::new(6, 1);
    let committed = [committed(present, 3, rect(0, 0, 64, 64))];
    let mut list: CompositorDisplayList = CompositorDisplayList {
        output: OUTPUT,
        commands: vec![
            CompositorDisplayCommand::SurfaceInstance(instance(
                7,
                2,
                present,
                rect(200, 10, 32, 32),
            )),
            CompositorDisplayCommand::SurfaceInstance(instance(
                7,
                1,
                missing,
                rect(100, 10, 32, 32),
            )),
        ],
    };
    let before = list.clone();
    assert_eq!(
        resolve_surface_instance_sources(&mut list, &committed),
        Err(CompositorMissingInstanceSource { source: missing })
    );
    assert_eq!(list, before, "nothing resolved, nothing dropped");
}

#[test]
fn malformed_instances_and_aliased_identities_are_refused() {
    let source = SurfaceId::new(4, 1);
    let committed = [committed(source, 3, rect(0, 0, 64, 64))];
    let good = instance(7, 1, source, rect(100, 10, 32, 32));
    let refused = |commands: Vec<CompositorSurfaceInstance>| {
        let list = list(
            commands
                .into_iter()
                .map(CompositorDisplayCommand::SurfaceInstance)
                .collect(),
            &committed,
        );
        output_frame_damage_snapshot(output(), list, &committed, None).is_err()
    };
    assert!(!refused(vec![good]));
    assert!(refused(vec![CompositorSurfaceInstance {
        opacity_millis: 0,
        ..good
    }]));
    assert!(refused(vec![CompositorSurfaceInstance {
        opacity_millis: 1_001,
        ..good
    }]));
    assert!(refused(vec![CompositorSurfaceInstance { id: 0, ..good }]));
    assert!(refused(vec![CompositorSurfaceInstance {
        owner_epoch: 0,
        ..good
    }]));
    assert!(refused(vec![CompositorSurfaceInstance {
        generation: 0,
        ..good
    }]));
    assert!(refused(vec![CompositorSurfaceInstance {
        clip: rect(0, 0, 0, 0),
        ..good
    }]));
    assert!(refused(vec![CompositorSurfaceInstance {
        clip: rect(400, 400, 10, 10),
        ..good
    }]));
    assert!(
        refused(vec![
            good,
            CompositorSurfaceInstance {
                destination: rect(300, 10, 32, 32),
                clip: rect(300, 10, 32, 32),
                ..good
            }
        ]),
        "one node per epoch and id"
    );
    assert!(
        !refused(vec![
            good,
            CompositorSurfaceInstance {
                owner_epoch: 8,
                ..good
            }
        ]),
        "the same opaque id under a new connection epoch is a distinct node"
    );
}

#[test]
fn restacking_instances_damages_every_instance_involved() {
    let source = SurfaceId::new(4, 1);
    let committed = [committed(source, 3, rect(-500, 0, 64, 64))];
    let lower = instance(7, 1, source, rect(100, 100, 64, 64));
    let upper = instance(7, 2, source, rect(132, 132, 64, 64));
    let before = list(
        vec![
            CompositorDisplayCommand::SurfaceInstance(lower),
            CompositorDisplayCommand::SurfaceInstance(upper),
        ],
        &committed,
    );
    let after = list(
        vec![
            CompositorDisplayCommand::SurfaceInstance(upper),
            CompositorDisplayCommand::SurfaceInstance(lower),
        ],
        &committed,
    );
    let before = output_frame_damage_snapshot(output(), before, &committed, None).unwrap();
    let after = output_frame_damage_snapshot(output(), after, &committed, None).unwrap();
    let damage = output_frame_damage(Some(&before), &after).unwrap();
    assert!(damage.rects.contains(&lower.destination));
    assert!(damage.rects.contains(&upper.destination));
}
