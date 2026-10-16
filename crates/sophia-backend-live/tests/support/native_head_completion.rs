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

// Reproduce the service-pass arrival window with the real poller and head
// reducer. No DRM device is opened; the second reader models an event becoming
// readable after the first card drain and before the fence is observed.
#[test]
fn event_arriving_after_card_drain_loses_to_fence_unless_collected_again() {
    use crate::{
        FakeLibdrmNativePageFlipReader, LibdrmBackendFdAuthority, LibdrmNativeOutputRoute,
        LibdrmNativeOutputSlot, LibdrmNativePageFlipCallback, LibdrmNativePageFlipSource,
        NativeLibdrmPageFlipEventPoller,
    };
    for collect_again in [false, true] {
        let mut head = head(1);
        let slot = LibdrmNativeOutputSlot::new(1).unwrap();
        let source =
            LibdrmNativePageFlipSource::from_authority(LibdrmBackendFdAuthority::new(1).unwrap());
        let mut poller =
            NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
                slot,
                output: head.output.id,
                head: head.head,
            }]);
        let mut callbacks = Vec::new();
        let mut timestamps = Vec::new();
        poller.read_and_collect_page_flip_events(
            &mut FakeLibdrmNativePageFlipReader::new([]),
            &mut callbacks,
            &mut timestamps,
            1,
            1,
        );
        assert!(callbacks.is_empty());
        let mut arriving = FakeLibdrmNativePageFlipReader::new([
            LibdrmNativePageFlipCallback::new_with_kernel_timestamp(
                slot,
                81,
                std::time::Duration::from_micros(123_456),
            ),
        ]);
        if collect_again {
            poller.read_and_collect_page_flip_events(
                &mut arriving,
                &mut callbacks,
                &mut timestamps,
                1,
                1,
            );
            assert!(head.queue_page_flip_callback(callbacks[0]).unwrap());
        }
        // The same signaled fence is available in both orderings. Current
        // retirement prefers a queued event, otherwise synthesizes a callback.
        let (_, source) = head.take_signaled_fence_completion();
        if collect_again {
            assert_eq!(source, LiveProductionKmsCompletionSource::PageFlipEvent);
            assert_eq!(timestamps[0].ust_usec, 123_456);
            assert_eq!(
                head.completion_mode,
                LiveProductionKmsCompletionMode::PageFlipPreferred
            );
        } else {
            poller.read_and_collect_page_flip_events(
                &mut arriving,
                &mut callbacks,
                &mut timestamps,
                1,
                1,
            );
            assert_eq!(timestamps[0].ust_usec, 123_456);
            assert!(!head.queue_page_flip_callback(callbacks[0]).unwrap());
            assert_eq!(head.late_page_flip_events, 1);
            assert_eq!(
                head.completion_mode,
                LiveProductionKmsCompletionMode::OutFenceAuthoritative
            );
        }
    }
}

#[test]
fn fence_recheck_is_only_for_first_fallback_without_a_queued_event() {
    let mut head = head(1);
    assert!(head.needs_fence_recheck());
    let callback = event(&head, 80);
    head.queue_page_flip_callback(callback).unwrap();
    assert!(!head.needs_fence_recheck());
    assert_eq!(
        head.take_signaled_fence_completion(),
        (callback, LiveProductionKmsCompletionSource::PageFlipEvent)
    );
    assert!(head.needs_fence_recheck());
    let (_, source) = head.take_signaled_fence_completion();
    assert_eq!(source, LiveProductionKmsCompletionSource::OutFence);
    assert!(!head.needs_fence_recheck());
    let _ = installed(&mut head);
    assert!(!head.needs_fence_recheck());
}

#[test]
fn shared_card_recheck_keeps_each_heads_event_and_timestamp() {
    use crate::{
        FakeLibdrmNativePageFlipReader, LibdrmBackendFdAuthority, LibdrmNativeOutputRoute,
        LibdrmNativeOutputSlot, LibdrmNativePageFlipCallback, LibdrmNativePageFlipSource,
        NativeLibdrmPageFlipEventPoller,
    };
    let mut heads = [head(1), head(2)];
    let slots = [
        LibdrmNativeOutputSlot::new(1).unwrap(),
        LibdrmNativeOutputSlot::new(2).unwrap(),
    ];
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(LibdrmNativePageFlipSource::from_authority(
            LibdrmBackendFdAuthority::new(1).unwrap(),
        ))
        .with_routes(heads.iter().zip(slots).map(|(head, slot)| {
            LibdrmNativeOutputRoute {
                slot,
                output: head.output.id,
                head: head.head,
            }
        }));
    let mut reader =
        FakeLibdrmNativePageFlipReader::new(slots.into_iter().enumerate().map(|(i, slot)| {
            LibdrmNativePageFlipCallback::new_with_kernel_timestamp(
                slot,
                80 + i as u64,
                std::time::Duration::from_micros(123_456 + i as u64),
            )
        }));
    let mut callbacks = Vec::new();
    let mut timestamps = Vec::new();
    poller.read_and_collect_page_flip_events(&mut reader, &mut callbacks, &mut timestamps, 2, 2);
    assert_eq!(callbacks.len(), 2);
    assert_eq!(timestamps.len(), 2);
    for callback in callbacks {
        let head = heads.iter_mut().find(|h| h.head == callback.head).unwrap();
        head.queue_page_flip_callback(callback).unwrap();
    }
    for (i, head) in heads.iter_mut().enumerate() {
        assert!(!head.needs_fence_recheck());
        let (callback, source) = head.take_signaled_fence_completion();
        assert_eq!(source, LiveProductionKmsCompletionSource::PageFlipEvent);
        let timestamp = timestamps
            .iter()
            .find(|t| t.head == callback.head && t.frame_serial == callback.frame_serial)
            .unwrap();
        assert_eq!(timestamp.ust_usec, 123_456 + i as u64);
        assert_eq!(head.out_fence_retirements, 0);
        assert!(head.pending_callback.is_none());
    }
}
