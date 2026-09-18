//! Actual intake/queue/projection with supplied device completion, not KMS.
use super::*;

#[test]
fn exact_component_removal_retains_bar_and_waits_for_actual_replacement() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let bar = grant();
    let menu = ContentGrant {
        connection_epoch: 20,
        content_grant_epoch: 21,
    };
    let mut bar_store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(bar)).unwrap();
    let mut menu_store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(menu)).unwrap();
    let id = ContentResourceId {
        id: 1,
        generation: 1,
    };
    runtime
        .set_shell_component_content_on_target(
            shell_frame(outputs[0], 1, upload(&mut bar_store, bar, id)),
            LiveShellContentLayer::Shell,
            &scene,
            Some(&mut target),
        )
        .unwrap();
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    let bar_epoch = runtime
        .shell_content_presentation_epoch(outputs[0].id, bar, 1)
        .unwrap();
    let held = upload(&mut menu_store, menu, id);
    let mut menu_frame = shell_frame(outputs[0], 1, held.clone());
    menu_frame.allocations.push((
        menu_frame.targets[0].allocation,
        menu_frame.targets[0].allocation_logical,
        menu_frame.targets[0].allocation_pixel,
    ));
    runtime
        .set_shell_component_content_on_target(
            menu_frame,
            LiveShellContentLayer::Launcher,
            &scene,
            Some(&mut target),
        )
        .unwrap();
    // Prepared is not enough to discard the original candidate obligation.
    assert_eq!(
        runtime
            .remove_shell_component_content_on_target(
                outputs[0].id,
                LiveShellContentLayer::Launcher,
                menu,
                1,
                &scene,
                Some(&mut target)
            )
            .unwrap(),
        None
    );
    assert!(
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Launcher)]
            .interaction_revoked
    );
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    let binding = runtime.input_projections[0]
        .content
        .iter()
        .find(|v| v.grant == menu)
        .unwrap();
    assert!(!binding.authority_current);
    assert!(!binding.allocations.is_empty()); // old pixels remain occluding
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
            &runtime.input_projections[0].content,
            false,
        );
        assert!(!matches!(
            result,
            sophia_engine::ContentPointerDisposition::Pass
                | sophia_engine::ContentPointerDisposition::Activated(_)
        ));
    }
    let old_menu = runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Launcher)].clone();
    let old_bar = runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Shell)].clone();
    target.reject_output = Some(outputs[0].id);
    assert!(
        runtime
            .remove_shell_component_content_on_target(
                outputs[0].id,
                LiveShellContentLayer::Launcher,
                menu,
                1,
                &scene,
                Some(&mut target)
            )
            .is_err()
    );
    assert_eq!(
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Launcher)],
        old_menu
    );
    assert_eq!(
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Shell)],
        old_bar
    );
    assert!(runtime.retained_projection_retirements.is_empty());
    assert_eq!(held.bytes().len(), 32);
    target.reject_output = None;
    assert!(
        runtime
            .set_shell_component_content_on_target(
                shell_frame(outputs[0], 2, held.clone()),
                LiveShellContentLayer::Launcher,
                &scene,
                Some(&mut target)
            )
            .is_err()
    );
    assert_eq!(
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Launcher)],
        old_menu
    );
    target.reject_output = None;
    let removal = runtime
        .remove_shell_component_content_on_target(
            outputs[0].id,
            LiveShellContentLayer::Launcher,
            menu,
            1,
            &scene,
            Some(&mut target),
        )
        .unwrap()
        .unwrap();
    assert!(!runtime.shell_component_removal_presented(removal));
    runtime.publish_presented_input_layers(&target);
    assert!(!runtime.shell_component_removal_presented(removal));
    assert_eq!(runtime.input_projections[0].content.len(), 2);
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(runtime.shell_component_removal_presented(removal));
    assert_eq!(runtime.input_projections[0].content.len(), 1);
    assert_eq!(runtime.input_projections[0].content[0].grant, bar);
    assert_eq!(
        runtime.shell_content_presentation_epoch(outputs[0].id, bar, 1),
        Some(bar_epoch)
    );
    assert_eq!(held.bytes().len(), 32); // receipt does not claim consumer release
    // A stale close cannot revoke/remove a successor, even in the same grant.
    runtime
        .set_shell_component_content_on_target(
            shell_frame(outputs[0], 2, held),
            LiveShellContentLayer::Launcher,
            &scene,
            Some(&mut target),
        )
        .unwrap();
    assert!(
        runtime
            .remove_shell_component_content_on_target(
                outputs[0].id,
                LiveShellContentLayer::Launcher,
                menu,
                1,
                &scene,
                Some(&mut target)
            )
            .is_err()
    );
    assert!(!runtime.shell_component_removal_presented(removal));
    assert!(
        !runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Launcher)]
            .interaction_revoked
    );
}
