use super::*;
use sophia_backend_live::{
    LiveScanoutLayoutWitness, LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus,
};

fn expected() -> LiveScanoutLayoutWitness {
    LiveScanoutLayoutWitness {
        source_image: sophia_renderer_live::LiveRendererImageId::from_raw(8812),
        alternative: correlation(false),
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        original_modifier: 7,
        alternative_modifier: 0,
    }
}

fn callback(serial: u64) -> LivePageFlipCallbackReport {
    LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::Accepted,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(serial),
        },
    }
}

fn cursor(assembly: &mut LiveBackendRuntimeAssembly) {
    let output = assembly
        .rendered_outputs()
        .outputs()
        .next()
        .unwrap()
        .output();
    assert!(assembly.set_cursor_ride_request(
        output,
        Some(sophia_backend_live::LibdrmNativeAtomicCursor {
            plane: drm::control::from_u32(61).unwrap(),
            properties: cursor_plane_property_handles(),
            placement: Some(sophia_backend_live::LibdrmNativeCursorPlacement {
                framebuffer: drm::control::from_u32(9).unwrap(),
                x: 40,
                y: 30,
                width: 64,
                height: 64,
            }),
        })
    ));
}

#[test]
fn only_the_unchanged_accepted_alternative_carries_the_layout_witness() {
    for (index, (outcomes, retry_cursor, carries)) in [
        (vec![Some(22), None, None], false, true),
        (vec![None, None, None], false, false),
        (vec![Some(11), None, None], false, false),
        (vec![Some(16), None, None], false, false),
        (vec![Some(5), None, None], false, false),
        (vec![Some(19), None, None], false, false),
        (vec![Some(12), None, None], false, false),
        (vec![Some(22), Some(22), None], false, false),
        (vec![Some(22), None, Some(5)], false, false),
        (vec![Some(22), None, Some(22), None], true, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut assembly = assembly(&format!("layout-witness-commit-{index}"));
        if retry_cursor {
            cursor(&mut assembly);
        }
        let device = Device::new(&outcomes);
        let mut exporter = Exporter::new();
        let result = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
        assert_eq!(
            result.layout_witness,
            carries.then(expected),
            "case {index}"
        );
        assert_eq!(
            exporter.reports.len(),
            1,
            "the negative case still performed its comparison"
        );
        assert_eq!(device.captured.borrow().len(), outcomes.len());
        assert!(device.outcomes.borrow().is_empty());
        assert_eq!(result.cursor_dropped, retry_cursor);
        if let Some(submission) = result.submission {
            assert_eq!(
                submission.layout_witness(),
                carries.then(expected),
                "case {index}"
            );
            retire(&device, submission);
        } else {
            assert_eq!(
                result.status,
                LiveRenderedPrimaryPlaneScanoutSubmitStatus::PrimaryPlaneSubmitFailed
            );
            assert!(result.cleanup.is_none());
        }
    }
}

#[test]
fn mapped_submission_keeps_its_own_witness_after_exporter_history_changes() {
    let mut assembly = assembly("layout-witness-owner");
    let device = Device::new(&[Some(22), None, None]);
    let mut exporter = Exporter::new();
    let result = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(result.layout_witness, Some(expected()));
    let submission = result.submission.unwrap().map_scanout_buffer(Box::new);
    exporter.reports[0].source_image = sophia_renderer_live::LiveRendererImageId::from_raw(9999);
    exporter.reports[0]
        .alternative
        .trace
        .as_mut()
        .unwrap()
        .scene_generation += 1;
    let mut refused = callback(55);
    refused.decision = LivePageFlipCallbackDecision::RejectedStaleFrameSerial;
    let waiting =
        retire_rendered_primary_plane_scanout_after_page_flip(&device, submission, &refused);
    assert_eq!(
        waiting.status,
        LibdrmNativePrimaryPlaneScanoutRetireStatus::WaitingForAcceptedPageFlip
    );
    assert!(waiting.layout_witness.is_none());
    let submission = waiting.submission.unwrap();
    assert_eq!(submission.layout_witness(), Some(expected()));
    let retired =
        retire_rendered_primary_plane_scanout_after_page_flip(&device, submission, &callback(56));
    assert_eq!(retired.layout_witness, Some(expected()));
    assert!(retired.submission.is_none());
    assert!(retired.cleanup.is_none());
}

#[test]
fn tracked_witness_waits_for_a_newer_accepted_callback_and_emits_once() {
    for persistent in [false, true] {
        let mut assembly = assembly(&format!("layout-witness-tracked-{persistent}"));
        if persistent {
            assembly = assembly.with_persistent_rendered_primary_plane_scanout();
        }
        let device = Device::new(&[Some(22), None, None]);
        let mut exporter = Exporter::new();
        let baseline = assembly.observe_page_flip_callback(LivePageFlipCallback {
            output: OutputId::from_raw(1),
            head: RenderHeadId::from_raw(1),
            frame_serial: 55,
        });
        assert_eq!(baseline.decision, LivePageFlipCallbackDecision::Accepted);
        let submitted =
            assembly.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut exporter);
        assert_eq!(submitted.layout_witness, Some(expected()));
        exporter.reports.clear();
        let waiting = assembly
            .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(55));
        assert_eq!(
            waiting.status,
            LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::WaitingForAcceptedPageFlip
        );
        assert!(waiting.layout_witness.is_none());
        assert!(waiting.in_flight);
        let presented = assembly
            .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(56));
        assert_eq!(
            presented.status,
            LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
        );
        assert_eq!(presented.layout_witness, Some(expected()));
        let duplicate = assembly
            .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(56));
        assert_eq!(
            duplicate.status,
            LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::NoSubmission
        );
        assert!(duplicate.layout_witness.is_none());
        if persistent {
            assert!(
                !assembly
                    .retire_displayed_rendered_primary_plane_scanout(&device)
                    .cleanup_pending
            );
        }
    }
}

#[test]
fn accepted_presentation_keeps_witness_when_previous_owner_cleanup_fails() {
    let mut assembly =
        assembly("layout-witness-old-cleanup").with_persistent_rendered_primary_plane_scanout();
    let device = Device::new(&[None, Some(22), None, None]);
    let mut first = Exporter::new();
    first.source = None;
    let submitted =
        assembly.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut first);
    assert!(submitted.layout_witness.is_none());
    let retired = assembly
        .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(1));
    assert_eq!(
        retired.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert!(retired.layout_witness.is_none());
    device.refuse_destroy.borrow_mut().insert(100);
    let mut second = Exporter::new();
    let submitted =
        assembly.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut second);
    assert_eq!(submitted.layout_witness, Some(expected()));
    let retired = assembly
        .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(2));
    assert_eq!(
        retired.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert_eq!(
        retired.runtime_scanout_state,
        Some(sophia_engine::RuntimeScanoutState::Retired)
    );
    assert!(retired.destroy.is_some());
    assert_ne!(
        retired.destroy,
        Some(LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed)
    );
    assert_eq!(retired.layout_witness, Some(expected()));
    assert!(retired.cleanup_pending);
    assert!(assembly.rendered_primary_plane_scanout_displayed());
    device.refuse_destroy.borrow_mut().clear();
    assert!(
        !assembly
            .retry_tracked_rendered_primary_plane_scanout_cleanup(&device)
            .cleanup_pending
    );
    assert!(
        !assembly
            .retire_displayed_rendered_primary_plane_scanout(&device)
            .cleanup_pending
    );
}

#[test]
fn head_loss_does_not_emit_the_held_layout_witness() {
    let mut assembly = assembly("layout-witness-head-lost");
    let head = |connector: u32| {
        LibdrmNativePrimaryPlaneSelection::new(
            drm::control::from_u32(connector).unwrap(),
            drm::control::from_u32(connector + 100).unwrap(),
            drm::control::from_u32(connector + 200).unwrap(),
            FRAME_SIZE,
            None,
        )
    };
    assert!(assembly.configure_native_output_heads(OutputId::from_raw(1), [head(21), head(22)]));
    let device = Device::new(&[Some(22), None, None]);
    let mut exporter = Exporter::new();
    let submitted =
        assembly.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(submitted.layout_witness, Some(expected()));
    assert!(assembly.lose_native_output_head(OutputId::from_raw(1), 22));
    let retired = assembly
        .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(56));
    assert_eq!(
        retired.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::HeadLost
    );
    assert!(retired.layout_witness.is_none());
    assert!(!retired.in_flight);
}

#[test]
fn untracked_presentation_reports_witness_even_when_its_resource_cleanup_fails() {
    let mut assembly = assembly("layout-witness-untracked-cleanup");
    let device = Device::new(&[Some(22), None, None]);
    let mut exporter = Exporter::new();
    let submitted = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(submitted.layout_witness, Some(expected()));
    device.refuse_destroy.borrow_mut().insert(100);
    let retired = retire_rendered_primary_plane_scanout_after_page_flip(
        &device,
        submitted.submission.unwrap(),
        &callback(56),
    );
    assert_eq!(
        retired.status,
        LibdrmNativePrimaryPlaneScanoutRetireStatus::ResourceRetireFailed
    );
    assert_eq!(retired.layout_witness, Some(expected()));
    assert!(retired.submission.is_none());
    let cleanup = retired
        .cleanup
        .expect("cleanup debt remains separate from presentation");
    device.refuse_destroy.borrow_mut().clear();
    assert!(
        retry_rendered_primary_plane_scanout_cleanup(&device, cleanup)
            .cleanup
            .is_none()
    );
}

#[test]
fn cleanup_retries_release_a_healthy_successor_while_its_predecessor_stays_stuck() {
    use sophia_backend_live::LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus as Cleanup;
    let mut runtime =
        assembly("fair-persistent-cleanup").with_persistent_rendered_primary_plane_scanout();
    let device = Device::new(&[None, None]);
    let mut first = Exporter::new();
    first.source = None;
    runtime.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut first);
    runtime.retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(1));
    device.refuse_destroy.borrow_mut().insert(100);
    let mut second = Exporter::new();
    second.source = None;
    runtime.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut second);
    let presented = runtime
        .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &callback(2));
    assert_eq!(
        presented.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert!(presented.cleanup_pending);
    device.refuse_destroy.borrow_mut().insert(101);
    assert_eq!(
        runtime
            .retire_displayed_rendered_primary_plane_scanout(&device)
            .status,
        Cleanup::CleanupFailed
    );
    device.refuse_destroy.borrow_mut().remove(&101);

    let mut successful_with_remaining = 0;
    for _ in 0..2 {
        let outcome = runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&device);
        assert!(outcome.cleanup_pending);
        if outcome.status == Cleanup::CleanedUp {
            successful_with_remaining += 1;
            assert_eq!(
                outcome.destroy,
                Some(LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed)
            );
        } else {
            assert_eq!(outcome.status, Cleanup::CleanupFailed);
        }
    }
    assert_eq!(
        successful_with_remaining, 1,
        "a stuck first owner must not starve the healthy second owner"
    );
    let count = |exporter: &Exporter| {
        exporter
            .dropped
            .borrow()
            .iter()
            .filter(|id| **id == 1)
            .count()
    };
    assert_eq!(count(&first), 0);
    assert_eq!(count(&second), 1);
    for _ in 0..4 {
        let outcome = runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&device);
        assert_eq!(outcome.status, Cleanup::CleanupFailed);
        assert!(outcome.cleanup_pending);
    }
    assert_eq!(count(&first), 0);
    assert_eq!(count(&second), 1);
    device.refuse_destroy.borrow_mut().clear();
    let outcome = runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&device);
    assert_eq!(outcome.status, Cleanup::CleanedUp);
    assert!(!outcome.cleanup_pending);
    assert_eq!(count(&first), 1);
    assert_eq!(count(&second), 1);
    assert_eq!(
        runtime
            .retry_tracked_rendered_primary_plane_scanout_cleanup(&device)
            .status,
        Cleanup::NoCleanupPending
    );
}
