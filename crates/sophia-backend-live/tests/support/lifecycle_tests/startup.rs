use super::*;

#[test]
fn retained_projection_waits_for_every_initial_modeset_without_queuing_a_prefix() {
    for inactive in 0..2 {
        check_initial_modeset_barrier(inactive);
    }
}

fn check_initial_modeset_barrier(inactive: usize) {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    scene.compose(&[], None, None).unwrap();
    let mut target = Target::new(&outputs);
    // Model one ready display and one inactive CRTC, in both output orders.
    // Only the CRTC fact is injected; admission, lowering and queues are real.
    runtime
        .outputs
        .require_native_initialization_for_test(outputs[inactive].id);
    assert!(runtime.outputs.native_initialized(outputs[1 - inactive].id));
    for _ in 0..2 {
        assert!(
            !runtime
                .queue_retained_projection(&scene, &mut target)
                .unwrap(),
            "retained work must not bypass an initial modeset"
        );
        assert!(runtime.retained_projection_pending);
        for output in outputs {
            assert!(
                !target.queue.pending(output.id),
                "no output prefix may queue"
            );
        }
        assert_eq!(target.next, 1, "no native frame may be allocated");
    }
    // The production initialization path marks this after its synchronous commit.
    runtime
        .outputs
        .mark_native_initialized(outputs[inactive].id)
        .unwrap();
    assert!(
        runtime
            .queue_retained_projection(&scene, &mut target)
            .unwrap()
    );
    for output in outputs {
        assert!(target.queue.pending(output.id));
    }
    assert!(!runtime.retained_projection_pending);
    target.teardown();
}
