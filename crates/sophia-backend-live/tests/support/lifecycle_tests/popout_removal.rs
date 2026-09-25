//! Real composition owners with supplied device completion, not physical KMS.
use super::*;

#[test]
fn popout_withdrawal_preserves_parent_and_retains_old_pixels_until_replacement() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let grant = grant();
    let mut resources =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    let bar = upload(
        &mut resources,
        grant,
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    let popup = upload(
        &mut resources,
        grant,
        ContentResourceId {
            id: 2,
            generation: 1,
        },
    );
    let mut frame = shell_frame(outputs[0], 1, bar);
    let output = frame.content_output;
    let parent = frame.targets[0].allocation;
    let allocation = sophia_protocol::ContentAllocationId {
        id: 20,
        generation: 1,
    };
    let mut image = frame.images[0].clone();
    image.node = CompositorNodeId::ShellContent {
        grant,
        output: outputs[0].id,
        candidate: 1,
        surface: 1,
        placement: 1,
    };
    image.geometry_px.y = 3;
    image.resource = popup.clone();
    frame.images.push(image);
    let mut popup_target = frame.targets[0].clone();
    popup_target.allocation = allocation;
    popup_target.allocation_logical.y = 3;
    popup_target.allocation_pixel.y = 3;
    frame.allocations.push((
        allocation,
        popup_target.allocation_logical,
        popup_target.allocation_pixel,
    ));
    frame.targets.push(popup_target);
    frame.popouts.push(sophia_engine::PresentedContentPopout {
        allocation,
        parent,
        surface_index: 1,
    });
    resources
        .retire(
            TransactionId::from_raw(4),
            &sophia_protocol::ContentResourceRetire {
                grant,
                resource: ContentResourceId {
                    id: 2,
                    generation: 1,
                },
            },
        )
        .unwrap();
    runtime
        .set_shell_content_on_target(frame, &scene, Some(&mut target))
        .unwrap();
    assert!(
        !runtime
            .withdraw_shell_popout_on_target(grant, output, allocation, &scene, Some(&mut target))
            .unwrap()
    );
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(!runtime.input_projections[0].content[0].authority_current);
    let original_epoch = runtime.input_projections[0].content[0].presentation_epoch;

    target.reject_output = Some(outputs[0].id);
    assert!(
        runtime
            .withdraw_shell_popout_on_target(grant, output, allocation, &scene, Some(&mut target))
            .is_err()
    );
    let key = (outputs[0].id, LiveShellContentLayer::Shell);
    assert_eq!(runtime.shell_content[&key].frame.images.len(), 2);
    assert_eq!(popup.bytes().len(), 32);
    target.reject_output = None;
    assert!(
        runtime
            .withdraw_shell_popout_on_target(grant, output, allocation, &scene, Some(&mut target))
            .unwrap()
    );
    assert_eq!(runtime.shell_content[&key].frame.images.len(), 1);
    assert_eq!(runtime.shell_content[&key].frame.targets.len(), 1);
    assert_eq!(
        runtime.shell_content[&key].frame.targets[0].allocation,
        parent
    );
    assert!(!runtime.input_projections[0].content[0].authority_current);
    assert_eq!(runtime.input_projections[0].content[0].popouts.len(), 1);
    runtime.publish_presented_input_layers(&target);
    assert_eq!(
        runtime.input_projections[0].content[0].popouts.len(),
        1,
        "enqueue is not presentation"
    );
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    let binding = &runtime.input_projections[0].content[0];
    assert!(binding.authority_current);
    assert_eq!(
        binding.presentation_epoch, original_epoch,
        "unchanged parent actions still name the candidate's original Presented receipt"
    );
    assert!(binding.popouts.is_empty());
    assert_eq!(binding.targets.len(), 1);
    assert_eq!(
        popup.bytes().len(),
        32,
        "an independent lease still owns the pixels"
    );
    resources.collect();
    assert_eq!(resources.usage().retiring, 32);
    drop(popup);
    resources.collect();
    assert_eq!(
        resources.usage().retiring,
        0,
        "the final independent lease releases its resource credit"
    );
    let mut wrong_grant = grant;
    wrong_grant.connection_epoch += 1;
    assert!(
        runtime
            .withdraw_shell_popout_on_target(wrong_grant, output, parent, &scene, Some(&mut target))
            .unwrap()
    );
    assert_eq!(
        runtime.shell_content[&key].frame.targets[0].allocation,
        parent
    );
}
