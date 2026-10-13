//! A supplied device flip must traverse native retirement selection before
//! the software Present's exact candidate can leave the backend.
use super::presentation_instances::{commit_cpu_surface, rect};
use super::*;

#[test]
fn native_software_retirement_uses_primary_clock_and_exact_candidate() {
    let outputs = [outputs()[0]];
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let surface = SurfaceId::new(5, 1);
    commit_cpu_surface(&mut runtime, &mut scene, surface, 55, 1, rect(0, 0, 8, 8));
    runtime.presentation_order = vec![surface];
    runtime.surface_outputs.insert(surface, output);
    let mut target = MirroredTarget::new(&outputs);
    target.track_retirements();

    for (transaction, first_head) in [(101, 1), (102, 0)] {
        let transaction = TransactionId::from_raw(transaction);
        let candidate = SurfaceTransactionKey {
            transaction,
            surface,
            target_buffer: BufferSource::CpuBuffer { handle: 55 },
        };
        runtime.surface_content_stream.begin(candidate).unwrap();
        let resources = runtime.presentation_feedback.resources_mut();
        resources.begin_software(transaction, None, None).unwrap();
        assert!(resources.poll_acquire_fence(transaction).unwrap());
        runtime
            .queue_software_present_frame(
                &mut scene,
                &outputs,
                vec![LiveProductionSoftwarePresentSubmission {
                    candidate,
                    source_size: Size {
                        width: 8,
                        height: 8,
                    },
                    transaction,
                    surface,
                    acquire_fence: None,
                    idle_fence: None,
                }],
            )
            .unwrap();
        assert!(
            runtime
                .stage_software_present_frame(&mut target, output)
                .unwrap()
        );
        target.install(output).unwrap();
        target.prepare(output);
        let submitted = NativeRetirementTarget::submitted_frame(&target, output).unwrap();
        assert!(runtime.retired_software_presents.is_empty());

        // The retirement owner observes the retained submitted identity before
        // consuming either completion, including the asymmetric mirror turn.
        target.completion_inputs.push_back((output, first_head));
        assert!(
            runtime
                .retire_native_output_on_target(&mut target, output, false)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            runtime.retired_software_presents.len(),
            usize::from(first_head == 0),
            "only the primary completion may settle the software Present"
        );
        assert_eq!(
            NativeRetirementTarget::submitted_frame(&target, output),
            (first_head != 0).then_some(submitted)
        );
        target.completion_inputs.push_back((output, 1 - first_head));
        assert!(
            runtime
                .retire_native_output_on_target(&mut target, output, false)
                .unwrap()
                .is_none()
        );
        let mut retired = Vec::new();
        runtime
            .drain_retired_software_presents_into(&mut retired)
            .unwrap();
        assert_eq!(
            retired.len(),
            1,
            "native retirement must settle the software Present"
        );
        assert_eq!(retired[0].candidate, candidate);
        assert_eq!(retired[0].frame, submitted);
        assert_eq!(retired[0].native_submission, submitted.raw());
        assert!(retired[0].ust_usec > 0 && retired[0].msc > 0);
        assert!(runtime.software_present_frames_bound.is_empty());
        assert!(runtime.software_present_frame_owners.is_empty());
        runtime
            .retire_native_output_on_target(&mut target, output, false)
            .unwrap();
        assert!(
            runtime.retired_software_presents.is_empty(),
            "retirement is consumed once"
        );
    }
    target.teardown();
}
