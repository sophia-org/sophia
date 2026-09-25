//! t244: WM policy targets on scaled and cropped heads: never rounded out of a
//! retired frame, never listed where no pixel is drawn, and refused before
//! commit when some head would not draw them.
use super::presentation_instances::{
    commit_cpu_surface, output_list, presentation_output, published, rect, region, shown_instance,
};
use super::*;

/// A non-empty policy target stays a drawn target on every head, however the
/// head scales: here a one-pixel preview at an even offset on a half-scale
/// head, from the production output list through the head plan to the
/// damage snapshot lowering retires. Before outward projection it rounded
/// to nothing, left the retired target list, and so failed the session's
/// completeness check on every republication.
#[test]
fn a_one_pixel_preview_survives_a_half_scale_head_in_the_retired_snapshot() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    let tiny = shown_instance(output, 2, 1, source, rect(2, 2, 1, 1));
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(output, PolicyPresentationMode::Overlay)],
                vec![tiny],
                vec![
                    region(
                        output,
                        3,
                        0,
                        PolicyPresentationRegionRole::Backdrop,
                        rect(6, 2, 1, 1),
                        rect(6, 2, 1, 1),
                    ),
                    region(
                        output,
                        4,
                        2,
                        PolicyPresentationRegionRole::Frame,
                        rect(10, 2, 3, 3),
                        rect(10, 2, 3, 3),
                    ),
                    region(
                        output,
                        5,
                        3,
                        PolicyPresentationRegionRole::Emphasis,
                        rect(20, 4, 5, 5),
                        rect(20, 4, 5, 5),
                    ),
                ],
            )),
            &scene,
            None,
        )
        .unwrap();
    let list = output_list(&runtime, output);
    let snapshot = sophia_engine::output_scene_snapshot_from_committed(
        outputs[0],
        1,
        runtime.committed_surfaces(),
        list,
        None,
    )
    .unwrap();
    let head = sophia_engine::HeadRenderTarget {
        head: sophia_engine::RenderHeadId::from_raw(1),
        output,
        target_generation: 1,
        native_size: Size {
            width: 32,
            height: 16,
        },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    };
    let plan = sophia_engine::build_head_composition_plan(&snapshot, head).unwrap();
    let retired = sophia_engine::head_output_damage_snapshot(&plan);
    let presented = LivePresentedPolicyPublication::from_presented_frame(&retired).unwrap();
    assert_eq!(
        presented.instances,
        vec![(2, 3)],
        "the preview is drawn and listed"
    );
    let drawn = retired
        .compositor_display_list
        .surface_instances()
        .next()
        .unwrap();
    assert!(!drawn.visible().is_empty());
    assert!(drawn.visible().x >= 0 && drawn.visible().x + drawn.visible().width <= 32);
    // Every listed region is listed because it draws at least one pixel.
    let mut listed = presented
        .regions
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    listed.sort_unstable();
    assert_eq!(listed, vec![3, 4, 5]);
    for command in &retired.compositor_display_list.commands {
        match command {
            CompositorDisplayCommand::Rect(rect)
                if matches!(rect.node, CompositorNodeId::PolicyRegion { .. }) =>
            {
                assert!(!rect.geometry.is_empty(), "{rect:?} draws nothing");
            }
            CompositorDisplayCommand::Border(border)
                if matches!(border.node, CompositorNodeId::PolicyRegion { .. }) =>
            {
                assert!(
                    sophia_engine::compositor_border_bands(*border)
                        .iter()
                        .any(|band| !band.geometry.is_empty()),
                    "{border:?} draws nothing"
                );
            }
            _ => {}
        }
    }
}

/// A tiny Overlay coverage still encloses what its tier draws on a scaled
/// head: the stamp's coverage rounds outward with the targets, so the
/// coverage damage a publication change produces contains every drawn pixel.
#[test]
fn a_tiny_overlay_coverage_encloses_its_outward_drawn_targets() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    let mut record = presentation_output(output, PolicyPresentationMode::Overlay);
    record.coverage = rect(2, 2, 1, 1);
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![record],
                vec![shown_instance(output, 2, 1, source, rect(2, 2, 1, 1))],
                vec![],
            )),
            &scene,
            None,
        )
        .unwrap();
    let snapshot = sophia_engine::output_scene_snapshot_from_committed(
        outputs[0],
        1,
        runtime.committed_surfaces(),
        output_list(&runtime, output),
        None,
    )
    .unwrap();
    let head = sophia_engine::HeadRenderTarget {
        head: sophia_engine::RenderHeadId::from_raw(1),
        output,
        target_generation: 1,
        native_size: Size {
            width: 32,
            height: 16,
        },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    };
    let plan = sophia_engine::build_head_composition_plan(&snapshot, head).unwrap();
    let retired = sophia_engine::head_output_damage_snapshot(&plan);
    let coverage = retired
        .compositor_display_list
        .presentation_stamp()
        .unwrap()
        .coverage;
    let drawn = retired
        .compositor_display_list
        .surface_instances()
        .next()
        .unwrap()
        .visible();
    assert!(!coverage.is_empty(), "the coverage keeps a pixel");
    assert!(
        coverage.x <= drawn.x
            && coverage.y <= drawn.y
            && coverage.x + coverage.width >= drawn.x + drawn.width
            && coverage.y + coverage.height >= drawn.y + drawn.height,
        "{coverage:?} does not enclose {drawn:?}"
    );
}

fn head_target(
    output: OutputId,
    head: u64,
    width: i32,
    height: i32,
    mapping: OutputHeadMapping,
) -> sophia_engine::HeadRenderTarget {
    sophia_engine::HeadRenderTarget {
        head: sophia_engine::RenderHeadId::from_raw(head),
        output,
        target_generation: 1,
        native_size: Size { width, height },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping,
    }
}

/// A publication with targets on both sides of a Cover head's crop: a
/// 64x32 output on a 32x32 head keeps only the middle half.
fn cropped_publication() -> (
    LiveProductionVisualRuntime,
    OutputId,
    sophia_engine::OutputSceneSnapshot,
) {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(output, PolicyPresentationMode::Overlay)],
                vec![
                    shown_instance(output, 2, 10, source, rect(30, 10, 4, 4)),
                    shown_instance(output, 3, 11, source, rect(2, 20, 2, 2)),
                ],
                vec![
                    region(
                        output,
                        4,
                        1,
                        PolicyPresentationRegionRole::Backdrop,
                        rect(2, 2, 1, 1),
                        rect(2, 2, 1, 1),
                    ),
                    region(
                        output,
                        5,
                        2,
                        PolicyPresentationRegionRole::Frame,
                        rect(4, 2, 3, 3),
                        rect(4, 2, 3, 3),
                    ),
                    region(
                        output,
                        6,
                        3,
                        PolicyPresentationRegionRole::Emphasis,
                        rect(58, 20, 4, 4),
                        rect(58, 20, 4, 4),
                    ),
                    region(
                        output,
                        7,
                        4,
                        PolicyPresentationRegionRole::Backdrop,
                        rect(28, 4, 4, 4),
                        rect(28, 4, 4, 4),
                    ),
                    region(
                        output,
                        8,
                        5,
                        PolicyPresentationRegionRole::Frame,
                        rect(26, 18, 6, 6),
                        rect(26, 18, 6, 6),
                    ),
                ],
            )),
            &scene,
            None,
        )
        .unwrap();
    let snapshot = sophia_engine::output_scene_snapshot_from_committed(
        outputs[0],
        1,
        runtime.committed_surfaces(),
        output_list(&runtime, output),
        None,
    )
    .unwrap();
    (runtime, output, snapshot)
}

/// A Cover head that crops a target away entirely neither draws nor lists
/// it; every target it lists draws inside the head.
#[test]
fn a_cover_head_lists_only_the_policy_targets_it_draws() {
    let (_runtime, output, snapshot) = cropped_publication();
    let plan = sophia_engine::build_head_composition_plan(
        &snapshot,
        head_target(output, 1, 32, 32, OutputHeadMapping::Cover),
    )
    .unwrap();
    let retired = sophia_engine::head_output_damage_snapshot(&plan);
    let presented = LivePresentedPolicyPublication::from_presented_frame(&retired).unwrap();
    assert_eq!(
        presented.instances,
        vec![(2, 3)],
        "only the on-screen preview"
    );
    let mut regions = presented
        .regions
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    regions.sort_unstable();
    assert_eq!(regions, vec![7, 8], "only the on-screen regions");
    let screen = rect(0, 0, 32, 32);
    let inside = |r: Rect| {
        !r.is_empty()
            && r.x < screen.width
            && r.y < screen.height
            && r.x + r.width > 0
            && r.y + r.height > 0
    };
    for command in &retired.compositor_display_list.commands {
        match command {
            CompositorDisplayCommand::Rect(rect)
                if matches!(rect.node, CompositorNodeId::PolicyRegion { .. }) =>
            {
                assert!(inside(rect.geometry), "{rect:?}");
            }
            CompositorDisplayCommand::Border(border)
                if matches!(border.node, CompositorNodeId::PolicyRegion { .. }) =>
            {
                assert!(
                    sophia_engine::compositor_border_bands(*border)
                        .iter()
                        .any(|band| inside(band.geometry)),
                    "{border:?}"
                );
            }
            CompositorDisplayCommand::SurfaceInstance(instance) => {
                assert!(inside(instance.visible()), "{instance:?}");
            }
            _ => {}
        }
    }
}

/// A matching stamp does not prove the same draw: a Fit primary draws every
/// target, a Cover mirror crops some away, and the whole output lists only
/// the targets both heads drew.
#[test]
fn a_whole_output_lists_only_targets_every_head_drew() {
    let (_runtime, output, snapshot) = cropped_publication();
    let fit = sophia_engine::head_output_damage_snapshot(
        &sophia_engine::build_head_composition_plan(
            &snapshot,
            head_target(output, 1, 64, 32, OutputHeadMapping::Fit),
        )
        .unwrap(),
    );
    let cover = sophia_engine::head_output_damage_snapshot(
        &sophia_engine::build_head_composition_plan(
            &snapshot,
            head_target(output, 2, 32, 32, OutputHeadMapping::Cover),
        )
        .unwrap(),
    );
    let primary = LivePresentedPolicyPublication::from_presented_frame(&fit).unwrap();
    assert_eq!(
        primary.instances.len(),
        2,
        "the Fit primary draws both previews"
    );
    assert_eq!(primary.regions.len(), 5);
    let whole =
        LivePresentedPolicyPublication::from_presented_heads(&[Some(&fit), Some(&cover)]).unwrap();
    assert_eq!(whole.instances, vec![(2, 3)]);
    let mut regions = whole.regions.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    regions.sort_unstable();
    assert_eq!(regions, vec![7, 8]);
}

/// Whole-candidate geometry admission, read-only and before commit: every
/// covered output needs a head, and every target must draw a clipped pixel
/// on every head (a Fit primary and a Cover or Exact mirror alike), by the
/// head plan's own arithmetic; a partly cropped target still draws.
#[test]
fn geometry_admission_refuses_a_target_any_head_would_not_draw() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(
        &mut runtime,
        &mut scene,
        source,
        55,
        1,
        rect(-100, -100, 8, 8),
    );
    let fit = head_target(output, 1, 64, 32, OutputHeadMapping::Fit);
    let cover = head_target(output, 2, 32, 32, OutputHeadMapping::Cover);
    let exact = head_target(output, 3, 32, 32, OutputHeadMapping::Exact);
    let overlay = || vec![presentation_output(output, PolicyPresentationMode::Overlay)];
    let candidate = |instances: Vec<PolicySurfaceInstance>,
                     regions: Vec<PolicyPresentationRegion>| {
        published(1, overlay(), instances, regions)
    };
    let visible = shown_instance(output, 2, 10, source, rect(30, 10, 4, 4));
    let accepted = candidate(
        vec![visible],
        vec![
            region(
                output,
                7,
                1,
                PolicyPresentationRegionRole::Backdrop,
                rect(28, 4, 4, 4),
                rect(28, 4, 4, 4),
            ),
            // Straddles the Cover crop: its left band is cut away, its right
            // band still draws.
            region(
                output,
                8,
                2,
                PolicyPresentationRegionRole::Frame,
                rect(14, 4, 6, 6),
                rect(14, 4, 6, 6),
            ),
        ],
    );
    assert_eq!(
        runtime.validate_policy_presentation_on_heads(&accepted, &[fit, cover]),
        Ok(())
    );
    let refused = |candidate: &LivePolicyPresentation,
                   heads: &[sophia_engine::HeadRenderTarget]| {
        runtime.validate_policy_presentation_on_heads(candidate, heads)
    };
    for (id, candidate) in [
        (
            4,
            candidate(
                vec![visible],
                vec![region(
                    output,
                    4,
                    1,
                    PolicyPresentationRegionRole::Backdrop,
                    rect(2, 2, 1, 1),
                    rect(2, 2, 1, 1),
                )],
            ),
        ),
        (
            5,
            candidate(
                vec![visible],
                vec![region(
                    output,
                    5,
                    1,
                    PolicyPresentationRegionRole::Frame,
                    rect(4, 2, 3, 3),
                    rect(4, 2, 3, 3),
                )],
            ),
        ),
        (
            6,
            candidate(
                vec![visible],
                vec![region(
                    output,
                    6,
                    1,
                    PolicyPresentationRegionRole::Emphasis,
                    rect(58, 20, 4, 4),
                    rect(58, 20, 4, 4),
                )],
            ),
        ),
        (
            3,
            candidate(
                vec![
                    visible,
                    shown_instance(output, 3, 11, source, rect(2, 20, 2, 2)),
                ],
                vec![],
            ),
        ),
    ] {
        assert_eq!(
            refused(&candidate, &[fit]),
            Ok(()),
            "the Fit primary draws target {id}"
        );
        assert_eq!(
            refused(&candidate, &[fit, cover]),
            Err(LivePolicyPresentationRefusal::UndrawnTarget { output, id }),
            "the Cover mirror crops target {id} away"
        );
    }
    let right_half = candidate(
        vec![visible],
        vec![region(
            output,
            6,
            1,
            PolicyPresentationRegionRole::Emphasis,
            rect(58, 20, 4, 4),
            rect(58, 20, 4, 4),
        )],
    );
    assert!(matches!(
        refused(&right_half, &[fit, exact]),
        Err(LivePolicyPresentationRefusal::UndrawnTarget { id: 6, .. })
    ));
    assert_eq!(
        refused(&accepted, &[]),
        Err(LivePolicyPresentationRefusal::MissingHeads { output }),
        "a covered output with no head is refused, not vacuously accepted"
    );
    assert_eq!(
        runtime.policy_presentation(),
        None,
        "admission is read-only"
    );
}
