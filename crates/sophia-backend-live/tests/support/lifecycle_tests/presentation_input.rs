use super::*;

#[test]
fn deferred_replacement_keeps_retired_application_lease_eligibility_until_install() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let surface = SurfaceId::new(91, 1);
    commit_cpu_surface(&mut runtime, &mut scene, surface, 91, 1, rect(0, 0, 32, 16));
    runtime.presentation_order = vec![surface];
    runtime.surface_outputs.insert(surface, output);
    let mut target = MirroredTarget::new(&outputs);
    let retire = |runtime: &mut LiveProductionVisualRuntime, target: &mut MirroredTarget| {
        let frames = runtime
            .retained_output_head_composition_frames(&scene, &*target)
            .unwrap();
        target
            .queue_retained_batch(frames, &BTreeSet::new())
            .unwrap();
        target.install(output).unwrap();
        target.prepare(output);
        target.flip(output, 0);
        target.flip(output, 1);
        runtime.publish_presented_input_layers(target);
    };
    retire(&mut runtime, &mut target);
    // This is the same eligibility predicate used by the session's lease
    // recheck, fed by the real retired production projection rather than a
    // manually retained application layer.
    assert!(sophia_engine::scene_contains_input_surface(
        &runtime.input_projections()[0].layers,
        surface
    ));
    let replacement = published(
        1,
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
    );
    // Session owns the deferral (covered by its install-boundary control).
    // Other frame service while a capture is held must keep the old eligibility.
    runtime.validate_policy_presentation(&replacement).unwrap();
    runtime.publish_presented_input_layers(&target);
    assert!(sophia_engine::scene_contains_input_surface(
        &runtime.input_projections()[0].layers,
        surface
    ));
    assert!(runtime.input_projections()[0].policy_publication.is_none());
    runtime
        .set_policy_presentation(Some(replacement), &scene, None)
        .unwrap();
    assert!(sophia_engine::scene_contains_input_surface(
        &runtime.input_projections()[0].layers,
        surface
    ));
    retire(&mut runtime, &mut target);
    assert!(!sophia_engine::scene_contains_input_surface(
        &runtime.input_projections()[0].layers,
        surface
    ));
    assert!(runtime.input_projections()[0].policy_publication.is_some());
    target.teardown();
}

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
