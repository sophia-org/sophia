#![cfg(test)]

use super::*;
use crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus as Status;
use sophia_engine::{EngineHeadRegistry, HeadRenderTarget, RenderHeadId};
use sophia_protocol::{OutputHeadMapping, OutputTransform, Size};

fn context() -> LayoutWitnessContext {
    LayoutWitnessContext {
        device: LiveRenderDeviceNodeIdentity {
            device: 1,
            inode: 2,
            device_number: 3,
        },
        output: OutputId::from_raw(1),
        head: RenderHeadId::from_raw(2),
        target_generation: 4,
        context_generation: 5,
    }
}

fn witness() -> crate::LiveScanoutLayoutWitness {
    crate::LiveScanoutLayoutWitness {
        source_image: sophia_renderer_live::LiveRendererImageId::from_raw(91),
        alternative: crate::LiveRendererFrameCorrelation {
            native: None,
            request: None,
            trace: Some(sophia_renderer_live::LiveCompositionTrace {
                output: context().output,
                head: context().head,
                scene_generation: 99,
            }),
            direct_scanout: Some(sophia_engine::DirectScanoutVerdict::CompositionRequired(
                "refused",
            )),
        },
        format: sophia_protocol::DRM_FORMAT_XRGB8888,
        original_modifier: 7,
        alternative_modifier: 0,
    }
}

fn content(frame: u64) -> LiveProductionScanoutContent {
    LiveProductionScanoutContent::MixedPresent {
        frame: LiveProductionNativeFrameId::from_raw(frame),
        transaction: TransactionId::from_raw(91),
        nonzero_rgb_pixels: 0,
    }
}

fn report(status: Status) -> crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport {
    crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport {
        status,
        layout_witness: matches!(
            status,
            Status::RetiredAfterPageFlip | Status::ResourceRetireFailed
        )
        .then_some(witness()),
        destroy: None,
        runtime_scanout_state: None,
        in_flight: false,
        in_flight_ticks: 0,
        cleanup_pending: status == Status::ResourceRetireFailed,
    }
}

fn submitted() -> NativeLayoutWitnessState {
    let mut state = NativeLayoutWitnessState::default();
    state.submit(Some(context()), 8, Some(content(9)), Some(witness()));
    state
}

#[test]
fn matching_physical_retirement_transfers_a_submitted_witness_once() {
    let mut state = submitted();
    state.retire(
        Some(context()),
        Some(8),
        Some(content(9)),
        report(Status::WaitingForAcceptedPageFlip),
    );
    assert!(
        state
            .take_completed(Some(context()), 8, content(9).frame())
            .is_none()
    );
    state.retire(
        Some(context()),
        Some(8),
        Some(content(9)),
        report(Status::RetiredAfterPageFlip),
    );
    let completed = state
        .take_completed(Some(context()), 8, content(9).frame())
        .unwrap();
    assert_eq!(completed.witness, witness());
    assert_eq!(completed.device, context().device);
    assert_eq!(completed.head, context().head);
    assert_eq!(completed.target_generation, context().target_generation);
    assert_eq!(completed.context_generation, context().context_generation);
    assert!(
        state
            .take_completed(Some(context()), 8, content(9).frame())
            .is_none()
    );
    state.retire(
        Some(context()),
        Some(8),
        Some(content(9)),
        report(Status::RetiredAfterPageFlip),
    );
    assert!(
        state
            .take_completed(Some(context()), 8, content(9).frame())
            .is_none()
    );
}

#[test]
fn retirement_must_match_the_frame_cycle_context_and_submitted_source() {
    let mut replaced_image = report(Status::RetiredAfterPageFlip);
    replaced_image.layout_witness.as_mut().unwrap().source_image =
        sophia_renderer_live::LiveRendererImageId::from_raw(92);
    for (current, cycle, frame, retirement) in [
        (
            None,
            Some(8),
            Some(content(9)),
            report(Status::RetiredAfterPageFlip),
        ),
        (
            Some(context()),
            None,
            Some(content(9)),
            report(Status::RetiredAfterPageFlip),
        ),
        (
            Some(context()),
            Some(7),
            Some(content(9)),
            report(Status::RetiredAfterPageFlip),
        ),
        (
            Some(context()),
            Some(8),
            Some(content(10)),
            report(Status::RetiredAfterPageFlip),
        ),
        (
            Some(context()),
            Some(8),
            None,
            report(Status::RetiredAfterPageFlip),
        ),
        (Some(context()), Some(8), Some(content(9)), replaced_image),
    ] {
        let mut state = submitted();
        state.retire(current, cycle, frame, retirement);
        assert!(
            state
                .take_completed(Some(context()), 8, content(9).frame())
                .is_none()
        );
    }
    let mut changed_device = context();
    changed_device.device.inode += 1;
    for current in [
        changed_device,
        LayoutWitnessContext {
            output: OutputId::from_raw(2),
            ..context()
        },
        LayoutWitnessContext {
            head: RenderHeadId::from_raw(3),
            ..context()
        },
        LayoutWitnessContext {
            target_generation: 5,
            ..context()
        },
        LayoutWitnessContext {
            context_generation: 6,
            ..context()
        },
    ] {
        let mut state = submitted();
        state.retire(
            Some(current),
            Some(8),
            Some(content(9)),
            report(Status::RetiredAfterPageFlip),
        );
        assert!(
            state
                .take_completed(Some(current), 8, content(9).frame())
                .is_none()
        );
    }
}

#[test]
fn cleanup_failure_after_an_accepted_flip_keeps_evidence_but_head_loss_does_not() {
    let mut state = submitted();
    state.retire(
        Some(context()),
        Some(8),
        Some(content(9)),
        report(Status::ResourceRetireFailed),
    );
    assert!(
        state
            .take_completed(Some(context()), 8, content(9).frame())
            .is_some()
    );
    let mut state = submitted();
    state.retire(
        Some(context()),
        Some(8),
        Some(content(9)),
        report(Status::HeadLost),
    );
    state.retire(
        Some(context()),
        Some(8),
        Some(content(9)),
        report(Status::RetiredAfterPageFlip),
    );
    assert!(
        state
            .take_completed(Some(context()), 8, content(9).frame())
            .is_none()
    );
}

#[test]
fn invalidation_prevents_reuse_even_when_the_old_target_generation_returns() {
    for before_retirement in [false, true] {
        let mut state = submitted();
        if before_retirement {
            state.invalidate();
        }
        state.retire(
            Some(context()),
            Some(8),
            Some(content(9)),
            report(Status::RetiredAfterPageFlip),
        );
        state.invalidate();
        assert!(
            state
                .take_completed(Some(context()), 8, content(9).frame())
                .is_none()
        );
        state.retire(
            Some(context()),
            Some(8),
            Some(content(9)),
            report(Status::RetiredAfterPageFlip),
        );
        assert!(
            state
                .take_completed(Some(context()), 8, content(9).frame())
                .is_none()
        );
    }
}

fn tracker() -> crate::LiveProductionPageFlipTracker {
    let mut heads = EngineHeadRegistry::new();
    assert!(
        heads
            .admit(HeadRenderTarget {
                head: context().head,
                output: context().output,
                target_generation: 4,
                native_size: Size {
                    width: 640,
                    height: 480
                },
                scale: 1,
                refresh_millihz: 60_000,
                transform: OutputTransform::Normal,
                mapping: OutputHeadMapping::Fit,
            })
            .is_admitted()
    );
    crate::LiveProductionPageFlipTracker::from_outputs(&heads)
}

fn payload(frame: u64, direct: bool) -> LiveProductionNativeRetirementContent {
    LiveProductionNativeRetirementContent {
        content: content(frame),
        submission: frame - 1,
        direct,
        layout_witness: (!direct).then_some(LiveProductionRetiredLayoutWitness {
            witness: witness(),
            device: context().device,
            head: context().head,
            target_generation: context().target_generation,
            context_generation: context().context_generation,
        }),
    }
}

#[test]
fn queued_retirements_keep_their_own_content_and_disposition_after_a_newer_flip() {
    let mut tracker = tracker();
    tracker.submit(context().output, 8).unwrap();
    tracker
        .observe_native_page_flip(context().output, 10, 12_345, Some(payload(9, false)))
        .unwrap();
    tracker.submit(context().output, 9).unwrap();
    tracker
        .observe_native_page_flip(context().output, 11, 28_345, Some(payload(10, true)))
        .unwrap();
    for (cycle, frame, direct, ust) in [(8, 9, false, 12_345), (9, 10, true, 28_345)] {
        let (retirement, native) = tracker.take_native_retirement(context().output).unwrap();
        assert_eq!(retirement.cycle, cycle);
        assert_eq!(retirement.retirement.ust, ust);
        assert_eq!(native, Some(payload(frame, direct)));
    }
    assert!(tracker.take_native_retirement(context().output).is_none());
}

#[test]
fn invalidating_queued_evidence_preserves_required_retirement_content() {
    let mut tracker = tracker();
    tracker.submit(context().output, 8).unwrap();
    tracker
        .observe_native_page_flip(context().output, 10, 12_345, Some(payload(9, false)))
        .unwrap();
    tracker.invalidate_layout_witnesses();
    let (retirement, native) = tracker.take_native_retirement(context().output).unwrap();
    assert_eq!(retirement.cycle, 8);
    assert_eq!(retirement.retirement.ust, 12_345);
    assert_eq!(
        native,
        Some(LiveProductionNativeRetirementContent {
            layout_witness: None,
            ..payload(9, false)
        })
    );
}

#[test]
fn rejected_timing_cannot_overwrite_an_older_queued_frame() {
    let mut tracker = tracker();
    tracker.submit(context().output, 8).unwrap();
    tracker
        .observe_native_page_flip(context().output, 10, 12_345, Some(payload(9, false)))
        .unwrap();
    tracker.submit(context().output, 9).unwrap();
    assert!(
        tracker
            .observe_native_page_flip(context().output, 10, 28_345, Some(payload(10, true)))
            .is_err()
    );
    let (retirement, native) = tracker.take_native_retirement(context().output).unwrap();
    assert_eq!(retirement.cycle, 8);
    assert_eq!(native, Some(payload(9, false)));
    assert!(tracker.take_native_retirement(context().output).is_none());
}

#[test]
fn a_different_native_submission_withholds_evidence_without_losing_retirement() {
    let mut tracker = tracker();
    tracker.submit(context().output, 8).unwrap();
    let native = payload(10, false);
    tracker
        .observe_native_page_flip(context().output, 10, 12_345, Some(native))
        .unwrap();
    let (retirement, queued) = tracker.take_native_retirement(context().output).unwrap();
    assert_eq!(retirement.cycle, 8);
    assert_eq!(retirement.retirement.ust, 12_345);
    assert_eq!(
        queued,
        Some(LiveProductionNativeRetirementContent {
            layout_witness: None,
            ..native
        })
    );
}
