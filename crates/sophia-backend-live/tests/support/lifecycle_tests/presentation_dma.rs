//! Full DMA driver and composed retirement with simulated worker allocation
//! and device completion. The driver creates the submitted candidate itself.
use super::*;

#[test]
fn native_dma_driver_retires_the_exact_candidate_after_image_promotion() {
    let outputs = [outputs()[0]];
    let output = outputs[0].id;
    let application = SurfaceId::new(7, 1);
    let mut scene = PresentScene {
        runtime: LiveProductionVisualRuntime::new(&outputs, None).unwrap(),
        scene: LiveProductionCpuScene::new(outputs[0].size),
        target: MirroredTarget::new(&outputs),
        output,
        application,
        previewed: SurfaceId::new(8, 1),
    };
    scene.target.track_retirements();
    scene
        .runtime
        .outputs
        .mark_native_initialized(output)
        .unwrap();
    scene.runtime.presentation_order = vec![application];
    scene.runtime.surface_outputs.insert(application, output);
    let (transaction, candidate) = scene.queue_present(900, Instant::now());
    scene
        .runtime
        .surface_content_stream
        .begin(candidate)
        .unwrap();
    scene
        .runtime
        .drive_gpu_presentation_on_target(&scene.scene, Some(&mut scene.target))
        .unwrap();
    assert!(
        scene.target.queue.pending(output),
        "DMA driver must queue the Present frame"
    );
    scene.target.install(output).unwrap();
    scene.target.prepare(output);
    let frame = NativeRetirementTarget::submitted_frame(&scene.target, output)
        .expect("DMA driver must queue and submit an actual native frame");
    assert_eq!(
        scene.runtime.present_scheduler.in_flight_frame(output),
        Some(frame)
    );
    assert!(!scene.runtime.present_scheduler.has_submitted());
    let image = renderer_image_for_present(transaction);
    assert!(
        !scene
            .target
            .images
            .get(&image)
            .expect("worker must capture the queued DMA layer")
            .is_promoted()
    );
    assert!(scene.runtime.committed_surfaces().is_empty());
    let content = NativePresentTarget::submitted_content(&scene.target, output).unwrap();
    scene
        .runtime
        .settle_submission_ownership(
            &mut scene.target,
            output,
            content,
            Some((frame, transaction)),
        )
        .unwrap();
    assert!(scene.runtime.present_scheduler.has_submitted());
    scene.target.completion_inputs.push_back((output, 1));
    assert!(
        scene
            .runtime
            .retire_native_output_on_target(&mut scene.target, output, false)
            .unwrap()
            .is_none()
    );
    assert!(
        !scene.target.images[&image].is_promoted(),
        "sibling completion cannot promote the image"
    );
    assert!(scene.runtime.committed_surfaces().is_empty());
    scene.target.completion_inputs.push_back((output, 0));
    let retired = scene
        .runtime
        .retire_native_output_on_target(&mut scene.target, output, false)
        .unwrap()
        .expect("composed DMA retirement must produce the exact Present candidate");
    assert_eq!(retired.candidate, candidate);
    assert_eq!(retired.transaction, transaction);
    assert_eq!(retired.surface, application);
    assert_eq!(retired.outputs, vec![output]);
    assert!(
        scene.target.images[&image].is_promoted(),
        "DMA retirement must promote its captured image"
    );
    assert!(!scene.runtime.present_scheduler.has_submitted());
    let committed = scene.runtime.committed_surfaces();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].surface, application);
    assert_eq!(committed[0].buffer(), candidate.target_buffer);
    assert_eq!(committed[0].committed_generation, 1);
    assert_eq!(scene.runtime.surface_content_stream.active_len(), 0);
    let mut feedback = Vec::new();
    scene
        .runtime
        .drain_present_feedback_into(&mut feedback)
        .unwrap();
    assert_eq!(feedback.len(), 1);
    assert_eq!(
        feedback[0].feedback,
        vec![
            LivePresentProtocolFeedback::Idle { transaction },
            LivePresentProtocolFeedback::Complete {
                transaction,
                ust: retired.ust_usec,
                msc: retired.msc,
                disposition: LivePresentBufferDisposition::Copied
            },
        ]
    );
    assert!(
        scene
            .runtime
            .retire_native_output_on_target(&mut scene.target, output, false)
            .unwrap()
            .is_none()
    );
    scene.target.teardown();
    assert!(scene.target.images.is_empty());
}
