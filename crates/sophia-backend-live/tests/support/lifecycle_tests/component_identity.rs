use super::*;

#[test]
fn foreign_node_or_resource_grant_refuses_before_real_queue_admission() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    for change_node in [true, false] {
        let mut frame = shell_frame(outputs[0], 1, lease.clone());
        if change_node {
            let CompositorNodeId::ShellContent { grant, .. } = &mut frame.images[0].node else {
                panic!("fixture must carry a shell node");
            };
            grant.connection_epoch += 1;
        } else {
            // Node/frame agree, but the actual pixel owner belongs elsewhere.
            frame.grant.connection_epoch += 1;
            let CompositorNodeId::ShellContent { grant, .. } = &mut frame.images[0].node else {
                panic!("fixture must carry a shell node");
            };
            *grant = frame.grant;
        }
        let next = target.next;
        assert!(
            runtime
                .set_shell_content_on_target(frame, &scene, Some(&mut target))
                .is_err()
        );
        assert_eq!(target.next, next);
        assert!(!target.queue.pending(outputs[0].id));
        assert!(runtime.shell_content.is_empty());
        assert!(runtime.retained_projection_retirements.is_empty());
        assert_eq!(lease.bytes().len(), 32);
    }
    // Exact custody is still usable after both refusals.
    runtime
        .set_shell_content_on_target(shell_frame(outputs[0], 1, lease), &scene, Some(&mut target))
        .unwrap();
    assert!(target.queue.pending(outputs[0].id));
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, grant(), 1)
            .is_some()
    );
}

#[test]
fn presentation_lookup_requires_exact_connection_and_content_grant() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(shell_frame(outputs[0], 1, lease), &scene, Some(&mut target))
        .unwrap();
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    let epoch = runtime.shell_content_presentation_epoch(outputs[0].id, grant(), 1);
    assert!(epoch.is_some());
    for wrong in [
        ContentGrant {
            connection_epoch: grant().connection_epoch + 1,
            ..grant()
        },
        ContentGrant {
            content_grant_epoch: grant().content_grant_epoch + 1,
            ..grant()
        },
    ] {
        assert_eq!(
            runtime.shell_content_presentation_epoch(outputs[0].id, wrong, 1),
            None
        );
        assert_eq!(
            runtime.shell_content_presentation_epoch(outputs[0].id, grant(), 1),
            epoch
        );
    }
}

#[test]
fn separate_component_layers_keep_both_real_sources_on_one_output() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let panel_grant = ContentGrant {
        connection_epoch: 100,
        content_grant_epoch: 100,
    };
    let launcher_grant = ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 2,
    };
    let mut panel_store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(panel_grant)).unwrap();
    let mut launcher_store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(launcher_grant))
            .unwrap();
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let panel = shell_frame(
        outputs[0],
        1,
        upload(&mut panel_store, panel_grant, resource),
    );
    let launcher = shell_frame(
        outputs[0],
        1,
        upload(&mut launcher_store, launcher_grant, resource),
    );
    let duplicate_grant = shell_frame(outputs[0], 2, panel.images[0].resource.clone());
    runtime
        .set_shell_component_content_on_target(
            panel,
            LiveShellContentLayer::Shell,
            &scene,
            Some(&mut target),
        )
        .unwrap();
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .set_shell_component_content_on_target(
                duplicate_grant,
                LiveShellContentLayer::Launcher,
                &scene,
                Some(&mut target)
            )
            .is_err()
    );
    assert_eq!(runtime.shell_content.len(), 1);
    runtime
        .set_shell_component_content_on_target(
            launcher,
            LiveShellContentLayer::Launcher,
            &scene,
            Some(&mut target),
        )
        .unwrap();
    assert_eq!(runtime.shell_content.len(), 2);
    // A prepared launcher has no input authority before its actual queued
    // generation is supplied as presented by this simulated completion edge.
    runtime.publish_presented_input_layers(&target);
    let before = &runtime.input_projections[0].content;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].grant, panel_grant);
    let panel_epoch = before[0].presentation_epoch;
    let panel_continuity = before[0].targets[0].continuity;
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    let images = runtime.tab_frames[&outputs[0].id]
        .content_images()
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].resource.grant, panel_grant);
    assert_eq!(images[1].resource.grant, launcher_grant);
    let bindings = &runtime.input_projections[0].content;
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].grant, panel_grant);
    assert_eq!(bindings[1].grant, launcher_grant);
    assert_eq!(bindings[0].presentation_epoch, panel_epoch);
    assert_eq!(bindings[0].targets[0].continuity, panel_continuity);
    assert!(bindings.iter().all(|binding| binding.authority_current));
    let mut capture = sophia_engine::ContentCaptureState::default();
    for pressed in [true, false] {
        let result = sophia_engine::resolve_content_pointer_stack(
            &mut capture,
            sophia_protocol::SeatId::from_raw(1),
            sophia_protocol::DeviceId::from_raw(1),
            sophia_protocol::InputEventKind::PointerButton {
                button: 0x110,
                pressed,
            },
            Some(sophia_protocol::Point { x: 1.0, y: 1.0 }),
            bindings,
            false,
        );
        if pressed {
            assert_eq!(result, sophia_engine::ContentPointerDisposition::Captured);
        } else {
            let sophia_engine::ContentPointerDisposition::Activated(target) = result else {
                panic!("launcher not activated")
            };
            assert_eq!(target.grant, launcher_grant);
        }
    }
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, panel_grant, 1)
            .is_some()
    );
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, launcher_grant, 1)
            .is_some()
    );
    let old_panel = runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Shell)].clone();
    let old_launcher =
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Launcher)].clone();
    let replacement = shell_frame(
        outputs[0],
        2,
        upload(
            &mut panel_store,
            panel_grant,
            ContentResourceId {
                id: 2,
                generation: 1,
            },
        ),
    );
    target.reject_output = Some(outputs[0].id);
    assert!(
        runtime
            .set_shell_component_content_on_target(
                replacement,
                LiveShellContentLayer::Shell,
                &scene,
                Some(&mut target)
            )
            .is_err()
    );
    assert_eq!(
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Shell)],
        old_panel
    );
    assert_eq!(
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Launcher)],
        old_launcher
    );
    assert!(runtime.retained_projection_retirements.is_empty());
}
