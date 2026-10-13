//! Shared native publication installation; only initialization and device
//! completion are supplied. Installation itself must queue the captured frame.
use super::presentation_instances::{
    commit_cpu_surface, presentation_output, published, rect, shown_instance,
};
use super::*;

#[test]
fn native_publication_installation_queues_exact_identity_and_rolls_back_refusal() {
    let outputs = [outputs()[0]];
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let source = SurfaceId::new(5, 1);
    commit_cpu_surface(&mut runtime, &mut scene, source, 55, 1, rect(0, 0, 8, 8));
    runtime.presentation_order = vec![source];
    runtime.surface_outputs.insert(source, output);
    runtime.outputs.mark_native_initialized(output).unwrap();
    let publication = published(
        7,
        vec![presentation_output(output, PolicyPresentationMode::Overlay)],
        vec![shown_instance(output, 9, 0, source, rect(4, 4, 8, 8))],
        vec![],
    );
    assert!(runtime.set_policy_presentation_on_target(
        Some(publication.clone()), &scene, Some(&mut target),
    ).unwrap());
    assert_eq!(runtime.policy_presentation(), Some(&publication));
    assert!(
        target.queue.pending(output),
        "publication installation must queue its native frame"
    );
    let batches = target.queued_batches;
    assert!(!runtime.set_policy_presentation_on_target(
        Some(publication.clone()), &scene, Some(&mut target),
    ).unwrap());
    assert_eq!(
        target.queued_batches, batches,
        "unchanged publication cannot queue another frame"
    );
    target.drain();
    runtime.publish_presented_input_layers(&target);
    let stamp = runtime.input_projections()[0]
        .policy_publication
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            stamp.owner_epoch,
            stamp.generation,
            stamp.output,
            stamp.output_generation
        ),
        (41, 7, output, 6)
    );
    assert_eq!(stamp.instances, vec![(9, 3)]);

    let mut successor = publication.clone();
    successor.presentation.generation += 1;
    target.refuse_queue = true;
    assert!(
        runtime
            .set_policy_presentation_on_target(Some(successor), &scene, Some(&mut target),)
            .is_err()
    );
    assert_eq!(
        runtime.policy_presentation(),
        Some(&publication),
        "failed native queue must restore the prior publication"
    );
    assert!(!target.queue.pending(output));
    target.refuse_queue = false;
    assert!(
        runtime
            .set_policy_presentation_on_target(None, &scene, Some(&mut target))
            .unwrap()
    );
    assert!(
        target.queue.pending(output),
        "withdrawal must queue its clearing frame"
    );
    target.drain();
    runtime.publish_presented_input_layers(&target);
    assert!(runtime.input_projections()[0].policy_publication.is_none());
    target.teardown();
}
