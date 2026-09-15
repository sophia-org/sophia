use sophia_engine::{
    CompositorDisplayCommand, CompositorDisplayListError, CompositorNodeId, FocusRingStyle,
    HeadlessOutput, OutputFramePresentationError, OutputFramePresentationState,
    OutputFullRepaintReason, OutputRepaintPlan, OutputRepaintPolicy, SurfaceChromeRole,
    SurfaceChromeStyle, SurfaceFrameStyle, compositor_border_bands, compositor_chrome_summary,
    compositor_display_list_damage, compositor_floating_outline, output_frame_damage_snapshot,
    plan_output_repaint, surface_chrome_display_list,
};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, OutputId, Rect, Region, Size, SurfaceId,
};

fn committed(surface: SurfaceId, geometry: Rect, generation: u64) -> CommittedSurfaceState {
    CommittedSurfaceState {
        surface,
        committed_generation: generation,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer {
                handle: u64::from(surface.index()),
            },
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
    }
}

#[test]
fn floating_outline_is_compositor_owned_and_does_not_need_new_client_pixels() {
    let surface = SurfaceId::new(7, 1);
    let geometry = Rect {
        x: 120,
        y: 90,
        width: 640,
        height: 480,
    };
    let outline = compositor_floating_outline(
        surface,
        geometry,
        2,
        sophia_engine::CompositorRgb8 {
            red: 0x70,
            green: 0xb7,
            blue: 0xff,
        },
    )
    .unwrap();

    assert_eq!(outline.inner, geometry);
    assert!(matches!(
        outline.node,
        CompositorNodeId::SurfaceChrome {
            surface: outlined,
            role: SurfaceChromeRole::FloatingOutline,
        } if outlined == surface
    ));
    assert_eq!(
        compositor_border_bands(outline)
            .into_iter()
            .filter(|band| !band.geometry.is_empty())
            .count(),
        4
    );
}

fn headless_output(output: OutputId) -> HeadlessOutput {
    HeadlessOutput {
        id: output,
        size: Size {
            width: 200,
            height: 80,
        },
        scale: 1,
    }
}

fn presentation_state(output: OutputId) -> OutputFramePresentationState {
    OutputFramePresentationState::new(headless_output(output)).unwrap()
}

#[test]
fn presentation_state_prepares_a_successor_while_submission_waits() {
    let output = OutputId::from_raw(7);
    let mut presentation = presentation_state(output);
    presentation
        .queue(frame_snapshot(
            output,
            sophia_engine::CompositorDisplayList::empty(output),
            &[],
        ))
        .unwrap();
    presentation.mark_submitted().unwrap();
    presentation
        .queue(frame_snapshot(
            output,
            sophia_engine::CompositorDisplayList::empty(output),
            &[],
        ))
        .unwrap();

    assert!(presentation.mark_rendering().is_ok());
    assert!(presentation.submitted().is_some());
    assert!(presentation.rendering().is_some());
    assert_eq!(
        presentation.promote_rendering_to_submitted(),
        Err(OutputFramePresentationError::SubmissionInFlight)
    );
}

fn frame_snapshot(
    output: OutputId,
    display_list: sophia_engine::CompositorDisplayList,
    states: &[CommittedSurfaceState],
) -> sophia_engine::OutputFrameDamageSnapshot {
    output_frame_damage_snapshot(headless_output(output), display_list, states, None).unwrap()
}

#[test]
fn focused_ring_is_outside_content_and_inserted_before_its_surface() {
    let output = OutputId::from_raw(1);
    let first = SurfaceId::new(1, 1);
    let focused = SurfaceId::new(2, 1);
    let geometry = Rect {
        x: 100,
        y: 40,
        width: 300,
        height: 200,
    };
    let list = surface_chrome_display_list(
        output,
        &[first, focused],
        &[
            committed(first, Rect { x: 0, ..geometry }, 3),
            committed(focused, geometry, 7),
        ],
        Some(focused),
        SurfaceChromeStyle::default(),
    )
    .unwrap();

    assert_eq!(list.output, output);
    assert_eq!(list.commands.len(), 3);
    assert_eq!(
        list.commands[0],
        CompositorDisplayCommand::Surface { surface: first }
    );
    assert_eq!(
        list.commands[2],
        CompositorDisplayCommand::Surface { surface: focused }
    );
    let borders = list.borders().collect::<Vec<_>>();
    assert_eq!(borders.len(), 1);
    let rects = compositor_border_bands(borders[0]).to_vec();
    assert_eq!(rects.len(), 4);
    assert_ne!(borders[0].generation, 0);
    assert_eq!(
        borders[0].node,
        CompositorNodeId::SurfaceChrome {
            surface: focused,
            role: SurfaceChromeRole::FocusRing,
        }
    );
    assert_eq!(
        rects.iter().map(|rect| rect.geometry).collect::<Vec<_>>(),
        [
            Rect {
                x: 98,
                y: 38,
                width: 304,
                height: 2,
            },
            Rect {
                x: 98,
                y: 240,
                width: 304,
                height: 2,
            },
            Rect {
                x: 98,
                y: 40,
                width: 2,
                height: 200,
            },
            Rect {
                x: 400,
                y: 40,
                width: 2,
                height: 200,
            },
        ]
    );
}

#[test]
fn chrome_summary_reduces_frame_and_ring_roles_without_client_identity() {
    let output = OutputId::from_raw(1);
    let first = SurfaceId::new(1, 1);
    let second = SurfaceId::new(2, 1);
    let geometry = Rect {
        x: 8,
        y: 8,
        width: 80,
        height: 60,
    };
    let style = SurfaceChromeStyle {
        focus_ring: FocusRingStyle {
            width: 2,
            ..FocusRingStyle::default()
        },
        frame: SurfaceFrameStyle {
            width: 6,
            ..SurfaceFrameStyle::default()
        },
    };
    let list = surface_chrome_display_list(
        output,
        &[first, second],
        &[
            committed(first, geometry, 1),
            committed(second, Rect { x: 108, ..geometry }, 1),
        ],
        Some(second),
        style,
    )
    .unwrap();

    let summary = compositor_chrome_summary(&list, Some(second));
    assert_eq!(summary.frames, 2);
    assert_eq!(summary.focused_frames, 1);
    assert_eq!(summary.unfocused_frames, 1);
    assert_eq!(summary.focus_rings, 1);
    assert_eq!(summary.primitives, 12);
    assert_eq!(summary.clearance, 6);
    assert_ne!(summary.generation, 0);
}

#[test]
fn hidden_or_uncommitted_focus_produces_no_border() {
    let output = OutputId::from_raw(1);
    let visible = SurfaceId::new(1, 1);
    let hidden = SurfaceId::new(2, 1);
    let states = [
        committed(
            visible,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            1,
        ),
        committed(
            hidden,
            Rect {
                x: 100,
                y: 0,
                width: 100,
                height: 100,
            },
            1,
        ),
    ];

    let hidden_list = surface_chrome_display_list(
        output,
        &[visible],
        &states,
        Some(hidden),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    assert_eq!(hidden_list.borders().count(), 0);

    let missing = SurfaceId::new(3, 1);
    let missing_list = surface_chrome_display_list(
        output,
        &[visible, missing],
        &states,
        Some(missing),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    assert_eq!(missing_list.borders().count(), 0);
}

#[test]
fn display_list_damage_covers_old_and_new_focus_extents_once_per_edge() {
    let output = OutputId::from_raw(1);
    let first = SurfaceId::new(1, 1);
    let second = SurfaceId::new(2, 1);
    let states = [
        committed(
            first,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
        committed(
            second,
            Rect {
                x: 100,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
    ];
    let first_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(first),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let stable_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(first),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let pixel_generation_only = surface_chrome_display_list(
        output,
        &[first, second],
        &[
            committed(first, states[0].geometry, 99),
            committed(second, states[1].geometry, 100),
        ],
        Some(first),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let second_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(second),
        SurfaceChromeStyle::default(),
    )
    .unwrap();

    assert!(compositor_display_list_damage(&first_list, &stable_list).is_empty());
    assert!(
        compositor_display_list_damage(&first_list, &pixel_generation_only).is_empty(),
        "client pixel generations must not damage stable compositor chrome"
    );
    let damage = compositor_display_list_damage(&first_list, &second_list);
    assert_eq!(damage.rects.len(), 8);
    assert!(damage.rects.iter().any(|rect| rect.x == -2));
    assert!(damage.rects.iter().any(|rect| rect.x == 98));
}

#[test]
fn display_list_rejects_invalid_duplicate_and_over_capacity_surface_streams() {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(1, 1);
    assert_eq!(
        surface_chrome_display_list(
            output,
            &[surface, surface],
            &[],
            None,
            SurfaceChromeStyle::default(),
        ),
        Err(CompositorDisplayListError::DuplicateSurface)
    );
    assert_eq!(
        surface_chrome_display_list(
            output,
            &[SurfaceId::INVALID],
            &[],
            None,
            SurfaceChromeStyle::default(),
        ),
        Err(CompositorDisplayListError::InvalidSurface)
    );

    let surfaces = (0..=sophia_engine::MAX_COMPOSITOR_DISPLAY_COMMANDS)
        .map(|index| SurfaceId::new(u32::try_from(index).unwrap(), 1))
        .collect::<Vec<_>>();
    assert_eq!(
        surface_chrome_display_list(output, &surfaces, &[], None, SurfaceChromeStyle::default(),),
        Err(CompositorDisplayListError::CapacityExceeded)
    );
}

#[test]
fn presentation_state_advances_only_after_accepted_submit_and_page_flip() {
    let output = OutputId::from_raw(1);
    let first = SurfaceId::new(1, 1);
    let second = SurfaceId::new(2, 1);
    let states = [
        committed(
            first,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
        committed(
            second,
            Rect {
                x: 100,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
    ];
    let first_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(first),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let second_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(second),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let mut presentation = presentation_state(output);

    assert_eq!(
        presentation
            .queue(frame_snapshot(output, first_list.clone(), &states))
            .unwrap()
            .compositor_damage
            .rects
            .len(),
        4
    );
    assert!(presentation.presented().is_none());
    presentation.mark_submitted().unwrap();
    assert!(presentation.presented().is_none());
    let first_presented = presentation.mark_presented().unwrap();
    assert_eq!(
        first_presented.snapshot.compositor_display_list,
        first_list.clone().into()
    );
    assert_eq!(
        presentation
            .presented()
            .map(|snapshot| &snapshot.compositor_display_list),
        Some(&sophia_engine::CompositorDamageList::from(
            first_list.clone()
        ))
    );

    assert_eq!(
        presentation
            .queue(frame_snapshot(output, second_list.clone(), &states))
            .unwrap()
            .damage
            .rects
            .len(),
        8
    );
    presentation.mark_submitted().unwrap();
    let second_presented = presentation.mark_presented().unwrap();
    assert_eq!(
        second_presented.snapshot.compositor_display_list,
        sophia_engine::CompositorDamageList::from(second_list.clone())
    );
    assert_eq!(second_presented.damage.rects.len(), 8);
}

#[test]
fn asynchronous_rendering_keeps_its_snapshot_when_newer_work_is_queued() {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(1, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 80,
    };
    let list = sophia_engine::CompositorDisplayList {
        output,
        commands: vec![CompositorDisplayCommand::Surface { surface }],
    };
    let snapshot = |generation| {
        frame_snapshot(
            output,
            list.clone(),
            &[committed(surface, geometry, generation)],
        )
    };
    let mut presentation = presentation_state(output);
    presentation.queue(snapshot(1)).unwrap();
    presentation.mark_initial_presented().unwrap();

    presentation.queue(snapshot(2)).unwrap();
    presentation.mark_rendering().unwrap();
    presentation.queue(snapshot(3)).unwrap();

    assert_eq!(
        presentation.rendering().unwrap().snapshot.surfaces[0].committed_generation,
        2
    );
    assert_eq!(
        presentation.pending().unwrap().snapshot.surfaces[0].committed_generation,
        3
    );
    assert_eq!(
        presentation.mark_submitted(),
        Err(OutputFramePresentationError::RenderingInFlight)
    );

    presentation.promote_rendering_to_submitted().unwrap();
    let retired = presentation.mark_presented().unwrap();
    assert_eq!(retired.snapshot.surfaces[0].committed_generation, 2);

    presentation.mark_submitted().unwrap();
    let retired = presentation.mark_presented().unwrap();
    assert_eq!(retired.snapshot.surfaces[0].committed_generation, 3);
}

#[test]
fn failed_and_superseded_pending_lists_do_not_advance_or_corrupt_damage_baseline() {
    let output = OutputId::from_raw(1);
    let first = SurfaceId::new(1, 1);
    let second = SurfaceId::new(2, 1);
    let states = [
        committed(
            first,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
        committed(
            second,
            Rect {
                x: 100,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
    ];
    let first_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(first),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let second_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(second),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let mut presentation = presentation_state(output);
    presentation
        .queue(frame_snapshot(output, first_list.clone(), &states))
        .unwrap();
    presentation.mark_initial_presented().unwrap();

    presentation
        .queue(frame_snapshot(output, second_list.clone(), &states))
        .unwrap();
    assert_eq!(
        presentation.discard_pending().unwrap().damage.rects.len(),
        8
    );
    assert_eq!(
        presentation
            .presented()
            .map(|snapshot| &snapshot.compositor_display_list),
        Some(&sophia_engine::CompositorDamageList::from(
            first_list.clone()
        ))
    );

    presentation
        .queue(frame_snapshot(output, second_list.clone(), &states))
        .unwrap();
    presentation
        .queue(frame_snapshot(output, first_list.clone(), &states))
        .unwrap();
    assert!(
        presentation.pending().unwrap().damage.is_empty(),
        "superseded work must compare with the still-presented list"
    );
    presentation.mark_submitted().unwrap();
    assert_eq!(
        presentation.mark_submitted(),
        Err(OutputFramePresentationError::SubmissionInFlight)
    );
    presentation.mark_presented().unwrap();
    assert_eq!(
        presentation
            .presented()
            .map(|snapshot| &snapshot.compositor_display_list),
        Some(&sophia_engine::CompositorDamageList::from(
            first_list.clone()
        ))
    );

    let wrong_output = surface_chrome_display_list(
        OutputId::from_raw(2),
        &[first],
        &states,
        Some(first),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let wrong_snapshot = output_frame_damage_snapshot(
        headless_output(OutputId::from_raw(2)),
        wrong_output,
        &states,
        None,
    )
    .unwrap();
    assert_eq!(
        presentation.queue(wrong_snapshot),
        Err(OutputFramePresentationError::OutputMismatch)
    );
}

#[test]
fn pending_list_uses_the_in_flight_submission_as_its_damage_baseline() {
    let output = OutputId::from_raw(1);
    let first = SurfaceId::new(1, 1);
    let second = SurfaceId::new(2, 1);
    let states = [
        committed(
            first,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
        committed(
            second,
            Rect {
                x: 100,
                y: 0,
                width: 100,
                height: 80,
            },
            1,
        ),
    ];
    let first_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(first),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let second_list = surface_chrome_display_list(
        output,
        &[first, second],
        &states,
        Some(second),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let mut presentation = presentation_state(output);
    presentation
        .queue(frame_snapshot(output, first_list.clone(), &states))
        .unwrap();
    presentation.mark_initial_presented().unwrap();
    presentation
        .queue(frame_snapshot(output, second_list.clone(), &states))
        .unwrap();
    presentation.mark_submitted().unwrap();

    let queued = presentation
        .queue(frame_snapshot(output, first_list.clone(), &states))
        .unwrap();
    assert_eq!(
        queued.damage.rects.len(),
        8,
        "the next frame follows the submitted list, not the older presented list"
    );
    presentation.mark_presented().unwrap();
    assert_eq!(
        presentation
            .presented()
            .map(|snapshot| &snapshot.compositor_display_list),
        Some(&sophia_engine::CompositorDamageList::from(
            second_list.clone()
        ))
    );
    presentation.mark_submitted().unwrap();
    presentation.mark_presented().unwrap();
    assert_eq!(
        presentation
            .presented()
            .map(|snapshot| &snapshot.compositor_display_list),
        Some(&sophia_engine::CompositorDamageList::from(
            first_list.clone()
        ))
    );
}

#[test]
fn repaint_plan_clips_and_coalesces_only_rectangular_output_damage() {
    let output_size = Size {
        width: 100,
        height: 100,
    };
    let damage = Region {
        rects: vec![
            Rect {
                x: -5,
                y: 10,
                width: 10,
                height: 10,
            },
            Rect {
                x: 5,
                y: 10,
                width: 5,
                height: 10,
            },
            Rect {
                x: 20,
                y: 20,
                width: 10,
                height: 2,
            },
            Rect {
                x: 20,
                y: 22,
                width: 2,
                height: 8,
            },
            Rect {
                x: 150,
                y: 150,
                width: 10,
                height: 10,
            },
        ],
    };

    let OutputRepaintPlan::Partial {
        damage,
        damaged_pixels,
    } = plan_output_repaint(output_size, &damage, OutputRepaintPolicy::default()).unwrap()
    else {
        panic!("small clipped damage should remain partial");
    };

    assert_eq!(
        damage.rects,
        [
            Rect {
                x: 0,
                y: 10,
                width: 10,
                height: 10,
            },
            Rect {
                x: 20,
                y: 20,
                width: 10,
                height: 2,
            },
            Rect {
                x: 20,
                y: 22,
                width: 2,
                height: 8,
            },
        ]
    );
    assert_eq!(damaged_pixels, 136);
}

#[test]
fn repaint_plan_falls_back_to_full_output_for_coverage_or_complexity() {
    let output_size = Size {
        width: 100,
        height: 100,
    };
    let coverage = Region::single(Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 80,
    });
    let OutputRepaintPlan::Full {
        damage,
        damaged_pixels,
        reason,
    } = plan_output_repaint(output_size, &coverage, OutputRepaintPolicy::default()).unwrap()
    else {
        panic!("large coverage should use a full repaint");
    };
    assert_eq!(reason, OutputFullRepaintReason::CoverageThresholdReached);
    assert_eq!(
        damage,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        })
    );
    assert_eq!(damaged_pixels, 10_000);

    let fragmented = Region {
        rects: (0..33)
            .map(|index| Rect {
                x: index * 2,
                y: 0,
                width: 1,
                height: 1,
            })
            .collect(),
    };
    let OutputRepaintPlan::Full { reason, .. } =
        plan_output_repaint(output_size, &fragmented, OutputRepaintPolicy::default()).unwrap()
    else {
        panic!("fragmented damage should use a full repaint");
    };
    assert_eq!(reason, OutputFullRepaintReason::PartialRectLimitExceeded);

    let over_capacity = Region {
        rects: vec![
            Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            };
            sophia_engine::MAX_OUTPUT_DAMAGE_RECTS + 1
        ],
    };
    let OutputRepaintPlan::Full { reason, .. } =
        plan_output_repaint(output_size, &over_capacity, OutputRepaintPolicy::default()).unwrap()
    else {
        panic!("over-capacity damage should fail safe to full repaint");
    };
    assert_eq!(reason, OutputFullRepaintReason::DamageCapacityExceeded);
}

#[test]
fn repaint_plan_rejects_invalid_policy_and_initial_presentation_fails_safe_to_full() {
    let invalid = OutputRepaintPolicy {
        max_partial_rects: 0,
        ..OutputRepaintPolicy::default()
    };
    assert!(
        plan_output_repaint(
            Size {
                width: 1,
                height: 1
            },
            &Region::empty(),
            invalid
        )
        .is_err()
    );
    assert_eq!(
        OutputFramePresentationState::new(HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 0,
                height: 1,
            },
            scale: 1,
        }),
        Err(OutputFramePresentationError::InvalidOutputSize)
    );

    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(1, 1);
    let states = [committed(
        surface,
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 80,
        },
        1,
    )];
    let list = surface_chrome_display_list(
        output,
        &[surface],
        &states,
        Some(surface),
        SurfaceChromeStyle::default(),
    )
    .unwrap();
    let mut presentation = presentation_state(output);
    let queued = presentation
        .queue(frame_snapshot(output, list, &states))
        .unwrap();

    assert_eq!(queued.compositor_damage.rects.len(), 4);
    assert!(matches!(
        queued.repaint,
        OutputRepaintPlan::Full {
            damaged_pixels: 16_000,
            ..
        }
    ));
    assert_eq!(queued.repaint.damage().unwrap().rects.len(), 1);
}
