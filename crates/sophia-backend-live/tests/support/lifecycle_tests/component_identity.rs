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
            .shell_content_presentation_epoch(outputs[0].id, 1)
            .is_some()
    );
}
