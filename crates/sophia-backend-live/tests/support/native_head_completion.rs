#![cfg(test)]

use super::*;
use sophia_engine::HeadlessCompositorBackendAssembly;
use sophia_renderer_live::{
    LiveRendererImportHealth, LiveRendererImportPathStatus, LiveRendererRuntimeObservation,
    LiveRendererSelectionObservation,
};

fn head(id: u32) -> LiveProductionNativeHead {
    native_head_fixture::head(
        id,
        1,
        crate::LibdrmNativePlaneFormatCapabilities::parse(id + 200, 0, &[]),
    )
}

fn runtime(head: &LiveProductionNativeHead) -> crate::LiveBackendRuntimeAssembly {
    crate::LiveBackendRuntimeAssembly::from_ready_headless_scanout(
        HeadlessCompositorBackendAssembly::new(head.output),
        head.output,
        LiveRendererRuntimeObservation {
            health: LiveRendererImportHealth::CpuFallback,
            xpixmap: LiveRendererImportPathStatus::Disabled,
            dmabuf: LiveRendererImportPathStatus::Disabled,
            selection: LiveRendererSelectionObservation::CpuFallback,
        },
    )
}

fn event(head: &LiveProductionNativeHead, serial: u64) -> crate::LivePageFlipCallback {
    crate::LivePageFlipCallback {
        output: head.output.id,
        head: head.head,
        frame_serial: serial,
    }
}

// Card/worker execution is not under test. Drive the production head reset,
// physical callback routing and runtime intake; the fixture settles a fence
// after its accepted completion as the mirror custody reducer does.
fn retire_fence(
    head: &mut LiveProductionNativeHead,
    runtime: &mut crate::LiveBackendRuntimeAssembly,
) {
    assert!(head.submitted_group_frame.is_some());
    let callback = head.synthesize_out_fence_callback();
    assert_eq!(
        runtime.observe_mirror_page_flip_callback(callback).decision,
        crate::LivePageFlipCallbackDecision::Accepted
    );
    head.last_callback_serial = Some(callback.frame_serial);
    head.submitted_group_frame = None;
    assert_eq!(
        runtime.observe_mirror_page_flip_callback(callback).decision,
        crate::LivePageFlipCallbackDecision::RejectedStaleFrameSerial
    );
}

fn installed(head: &mut LiveProductionNativeHead) -> crate::LiveBackendRuntimeAssembly {
    head.target_generation += 1;
    head.install_topology_presentation(OutputFramePresentationState::new(head.output).unwrap());
    // Runtime rebinding creates a fresh intake even though the card route lives on.
    runtime(head)
}

fn late_event_after_install(successor_submitted: bool) {
    for id in [1, 2] {
        let mut head = head(id);
        let mut intake = runtime(&head);
        head.submitted_group_frame = Some(LiveProductionNativeFrameId::from_raw(2));
        retire_fence(&mut head, &mut intake);
        let baseline = head.last_callback_serial;
        let mut intake = installed(&mut head);
        let successor = LiveProductionNativeFrameId::from_raw(3);
        if successor_submitted {
            head.submitted_group_frame = Some(successor);
        }
        // Kernel serials come from vblank counters, not the synthetic fence
        // counter. A high serial is still an old physical event.
        for serial in [81, 82] {
            let callback = event(&head, serial);
            assert!(
                !head.queue_page_flip_callback(callback).unwrap(),
                "late kernel event must not enter the replacement mirror intake"
            );
            assert!(head.pending_callback.is_none());
            assert_eq!(
                head.submitted_group_frame,
                successor_submitted.then_some(successor)
            );
        }
        assert_eq!(head.late_page_flip_events, 2);
        assert_eq!(head.last_callback_serial, baseline);
        head.submitted_group_frame = Some(successor);
        retire_fence(&mut head, &mut intake);
        assert_eq!(head.last_callback_serial, baseline.map(|serial| serial + 1));
        assert_eq!(head.out_fence_retirements, 2);

        // Rollback uses the same installation path and must not revive events
        // for either the original or candidate presentation.
        let _rollback_intake = installed(&mut head);
        let callback = event(&head, 83);
        assert!(!head.queue_page_flip_callback(callback).unwrap());
        assert!(head.pending_callback.is_none());
    }
}

#[test]
fn fence_retirement_then_install_rejects_late_event_before_successor_submit() {
    late_event_after_install(false);
}

#[test]
fn fence_retirement_then_install_rejects_late_event_after_successor_submit() {
    late_event_after_install(true);
}

#[test]
fn topology_rebind_keeps_the_event_serial_basis() {
    let mut head = head(1);
    let mut intake = runtime(&head);
    let callback = event(&head, 400);
    assert!(head.queue_page_flip_callback(callback).unwrap());
    assert_eq!(
        intake
            .observe_mirror_page_flip_callback(head.pending_callback.take().unwrap())
            .decision,
        crate::LivePageFlipCallbackDecision::Accepted
    );
    head.last_callback_serial = Some(callback.frame_serial);
    let mut intake = installed(&mut head);
    head.submitted_group_frame = Some(LiveProductionNativeFrameId::from_raw(3));
    retire_fence(&mut head, &mut intake);
    assert_eq!(head.last_callback_serial, Some(401));
}

#[test]
fn event_only_head_still_accepts_its_successor_and_refuses_duplicates() {
    let mut head = head(2);
    let mut intake = installed(&mut head);
    let callback = event(&head, 82);
    assert!(head.queue_page_flip_callback(callback).unwrap());
    assert!(head.queue_page_flip_callback(callback).is_err());
    assert_eq!(
        intake
            .observe_mirror_page_flip_callback(head.pending_callback.take().unwrap())
            .decision,
        crate::LivePageFlipCallbackDecision::Accepted
    );
    assert_eq!(
        intake.observe_mirror_page_flip_callback(callback).decision,
        crate::LivePageFlipCallbackDecision::RejectedStaleFrameSerial
    );
}

#[test]
fn completion_authority_is_local_to_the_physical_head_and_owner() {
    let mut first = head(1);
    first.synthesize_out_fence_callback();
    let mut second = head(2);
    let callback = event(&second, 81);
    assert!(first.queue_page_flip_callback(callback).is_err());
    assert_eq!(first.late_page_flip_events, 0);
    assert!(second.queue_page_flip_callback(callback).unwrap());
    let foreign_output = crate::LivePageFlipCallback {
        output: OutputId::from_raw(2),
        ..event(&first, 82)
    };
    assert!(first.queue_page_flip_callback(foreign_output).is_err());
    // Reconstructing a native owner creates fresh heads and card routes.
    let fresh = head(1);
    assert_eq!(
        fresh.completion_mode,
        LiveProductionKmsCompletionMode::PageFlipPreferred
    );
    assert_eq!(fresh.last_callback_serial, None);
}
