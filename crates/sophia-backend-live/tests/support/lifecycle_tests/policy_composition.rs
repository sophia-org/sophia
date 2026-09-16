//! Production policy-cycle capture/builder, actual Engine lowering and queue;
//! native device completion is supplied by Target, not KMS.
use super::*;

#[test]
fn policy_cycles_preserve_both_shell_sources_without_client_redraw() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    for (index, output) in outputs.iter().enumerate() {
        let lease = upload(
            &mut store,
            grant(),
            ContentResourceId {
                id: index as u64 + 1,
                generation: 1,
            },
        );
        runtime
            .set_shell_content_on_target(
                shell_frame(*output, index as u64 + 1, lease),
                &scene,
                Some(&mut target),
            )
            .unwrap();
        target.drain();
    }
    for cycle in 10..30 {
        // The exact capture and display-list builder invoked inside the CPU
        // policy callback. No new shell candidate is supplied in this loop.
        let captured = output_composition::OutputCompositionSnapshot::capture(&runtime);
        let mut frames = Vec::new();
        for output in outputs {
            let list = captured.display_list(output.id, &[]).unwrap();
            let expected = runtime.shell_content[&output.id].frame.images[0].clone();
            let images: Vec<_> = list
                .commands
                .iter()
                .filter_map(|command| match command {
                    CompositorDisplayCommand::ContentImage(image) => Some(image),
                    _ => None,
                })
                .collect();
            assert_eq!(
                images,
                vec![&expected],
                "policy callback lost the retained bar"
            );
            let snapshot = output_scene_snapshot_from_committed_in_view(
                output.id,
                cycle,
                runtime.outputs.logical_viewport(output.id).unwrap(),
                &[],
                list,
                None,
            )
            .unwrap();
            let plans =
                build_output_head_plans(&snapshot, &target.head_targets(output.id)).unwrap();
            let heads = plans
                .iter()
                .map(|plan| LiveProductionHeadCompositionFrame {
                    head: plan.head,
                    scene_generation: plan.scene_generation,
                    target_generation: plan.target_generation,
                    mapping: plan.mapping,
                    logical_content_checksum: plan.logical_content_checksum,
                    frame: lower_cpu_head_composition_plan_with_caches(
                        plan,
                        &[],
                        &mut runtime.indicator_strip_cache.borrow_mut(),
                        &mut runtime.text_cache.borrow_mut(),
                    )
                    .unwrap(),
                })
                .collect();
            frames.push((output.id, heads));
        }
        if cycle == 10 {
            target.reject_output = Some(outputs[1].id);
            assert!(target.queue_ordinary_batch(frames).is_err());
            target.reject_output = None;
            for output in outputs {
                assert!(
                    target
                        .presented_frame(output.id)
                        .unwrap()
                        .compositor_display_list
                        .commands
                        .iter()
                        .any(|command| matches!(
                            command,
                            CompositorDisplayCommand::ContentImage(_)
                        ))
                );
            }
            continue;
        }
        target.queue_ordinary_batch(frames).unwrap();
        // Until simulated completion the preceding displayed content remains.
        for output in outputs {
            assert!(
                target
                    .presented_frame(output.id)
                    .unwrap()
                    .compositor_display_list
                    .commands
                    .iter()
                    .any(|command| matches!(command, CompositorDisplayCommand::ContentImage(_)))
            );
        }
        target.drain();
        for output in outputs {
            assert!(
                target
                    .presented_frame(output.id)
                    .unwrap()
                    .compositor_display_list
                    .commands
                    .iter()
                    .any(|command| matches!(command, CompositorDisplayCommand::ContentImage(_)))
            );
        }
    }
    target.teardown();
}

#[test]
fn policy_capture_retains_sources_and_preserves_application_shell_overlay_order() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let lease = upload(&mut store, grant(), resource);
    runtime
        .set_shell_content_on_target(shell_frame(outputs[0], 1, lease), &scene, Some(&mut target))
        .unwrap();
    target.drain();
    let surface = SurfaceId::new(1, 1);
    let overlay_surface = SurfaceId::new(2, 1);
    runtime.presentation_order = vec![surface];
    runtime.surface_outputs.insert(surface, outputs[0].id);
    let overlay = CompositorDisplayCommand::Border(
        compositor_floating_outline(
            overlay_surface,
            Rect {
                x: 2,
                y: 2,
                width: 8,
                height: 8,
            },
            1,
            runtime.surface_chrome_style.focus_ring.color,
        )
        .unwrap(),
    );
    runtime.descriptor_overlay = Some(DescriptorOverlayProjection {
        output: outputs[0].id,
        generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        },
        commands: vec![overlay.clone()],
        targets: vec![],
    });
    let captured = output_composition::OutputCompositionSnapshot::capture(&runtime);
    let committed = [CommittedSurfaceState::with_source(
        surface,
        1,
        Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        },
        BufferSource::CpuBuffer { handle: 42 },
        Size {
            width: 8,
            height: 8,
        },
        Region::empty(),
    )];
    let actual = captured.display_list(outputs[0].id, &committed).unwrap();
    assert_eq!(
        actual,
        runtime
            .display_list_for_output(outputs[0].id, Rect::default(), &committed, &[surface])
            .unwrap()
    );
    assert!(
        matches!(actual.commands[0], CompositorDisplayCommand::Surface { surface: id } if id == surface)
    );
    assert!(matches!(
        actual.commands[1],
        CompositorDisplayCommand::ContentImage(_)
    ));
    assert_eq!(actual.commands[2], overlay);
    assert!(
        captured
            .display_list(outputs[1].id, &committed)
            .unwrap()
            .commands
            .is_empty()
    );
    // The captured cycle owns actual sources independently of the live map.
    runtime.shell_content.clear();
    drop(actual);
    target.teardown();
    retire(&mut store, grant(), resource);
    store.collect();
    assert!(store.take_event().is_none());
    assert!(matches!(
        captured
            .display_list(outputs[0].id, &committed)
            .unwrap()
            .commands[1],
        CompositorDisplayCommand::ContentImage(_)
    ));
    drop(captured);
    store.collect();
    assert!(store.take_event().is_some());
    assert!(store.take_event().is_none());
}
