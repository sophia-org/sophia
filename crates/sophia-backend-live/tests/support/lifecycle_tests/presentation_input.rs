use super::*;

#[test]
fn input_publication_comes_from_all_completed_heads_not_the_requested_getter() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let publication = |generation| {
        published(
            generation,
            vec![presentation_output(
                output,
                PolicyPresentationMode::ReplaceApplications,
            )],
            vec![],
            vec![region(
                output,
                1,
                0,
                PolicyPresentationRegionRole::Backdrop,
                rect(0, 0, 64, 32),
                rect(0, 0, 64, 32),
            )],
        )
    };
    runtime
        .set_policy_presentation(Some(publication(1)), &scene, None)
        .unwrap();
    let queue = |runtime: &LiveProductionVisualRuntime, target: &mut MirroredTarget| {
        let frames = runtime
            .retained_output_head_composition_frames(&scene, &*target)
            .unwrap();
        target
            .queue_retained_batch(frames, &BTreeSet::new())
            .unwrap();
        target.install(output).unwrap();
        target.prepare(output);
    };
    queue(&runtime, &mut target);
    runtime
        .set_policy_presentation(Some(publication(2)), &scene, None)
        .unwrap();
    runtime.publish_presented_input_layers(&target);
    assert!(runtime.input_projections()[0].policy_publication.is_none());
    target.flip(output, 1);
    runtime.publish_presented_input_layers(&target);
    assert!(runtime.input_projections()[0].policy_publication.is_none());
    target.flip(output, 0);
    runtime.publish_presented_input_layers(&target);
    let first = runtime.input_projections()[0].clone();
    assert_eq!(first.policy_publication.as_ref().unwrap().generation, 1);
    assert_eq!(
        runtime
            .policy_presentation()
            .unwrap()
            .presentation
            .generation,
        2
    );
    runtime.publish_presented_input_layers(&target);
    assert_eq!(runtime.input_projections()[0], first);
    queue(&runtime, &mut target);
    target.flip(output, 0);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime.input_projections()[0].policy_visible,
        "old mirror pixels still shield input"
    );
    assert!(!runtime.input_projections()[0].frame_completed);
    assert!(runtime.input_projections()[0].policy_publication.is_none());
    target.flip(output, 1);
    runtime.publish_presented_input_layers(&target);
    let second = runtime.input_projections()[0].clone();
    assert_eq!(second.policy_publication.as_ref().unwrap().generation, 2);
    assert!(second.epoch > first.epoch);
    runtime.set_policy_presentation(None, &scene, None).unwrap();
    runtime.publish_presented_input_layers(&target);
    assert_eq!(runtime.input_projections()[0], second);
    queue(&runtime, &mut target);
    target.flip(output, 0);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime.input_projections()[0].policy_visible,
        "primary withdrawal cannot uncover old mirror pixels"
    );
    assert!(!runtime.input_projections()[0].frame_completed);
    assert!(runtime.input_projections()[0].policy_publication.is_none());
    target.flip(output, 1);
    runtime.publish_presented_input_layers(&target);
    assert!(runtime.input_projections()[0].policy_publication.is_none());
    assert!(!runtime.input_projections()[0].policy_visible);
    assert!(runtime.input_projections()[0].frame_completed);
    assert!(runtime.input_projections()[0].epoch > second.epoch);
    target.teardown();
}
