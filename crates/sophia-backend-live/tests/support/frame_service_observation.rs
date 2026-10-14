use super::super::service::native_service_phase;
use super::*;

#[test]
fn protected_mirror_sibling_is_polled_before_waiting_present() {
    let output = OutputId::from_raw(1);
    let mut request = OutputFrameServiceRequest {
        outputs: vec![OutputFrameServiceObservation {
            output,
            primary: true,
            native_phase: native_service_phase(
                (false, false),
                (true, false),
                Some((false, false)),
                true,
            ),
            pending_frame: false,
        }],
        presentation_queued: true,
        software_frame_waiting: false,
        preparation_pending: false,
    };
    let mut reducer = OutputFrameServiceReducer::begin(&request).unwrap();
    assert_eq!(
        reducer.next_effect(&request).unwrap(),
        Some(OutputFrameServiceEffect::PollRetirement { output })
    );
    // An unsignaled sibling keeps the next Present queued.
    assert_eq!(reducer.next_effect(&request).unwrap(), None);
    request.outputs[0].native_phase =
        native_service_phase((false, false), (false, false), Some((false, false)), false);
    let mut reducer = OutputFrameServiceReducer::begin(&request).unwrap();
    assert_eq!(
        reducer.next_effect(&request).unwrap(),
        Some(OutputFrameServiceEffect::SubmitQueuedPresentation { output })
    );
}

#[test]
fn unprotected_repaint_keeps_primary_lane_and_runtime_cleanup() {
    assert_eq!(
        native_service_phase((false, false), (true, false), Some((false, false)), false),
        OutputNativeFramePhase::Idle
    );
    assert_eq!(
        native_service_phase((false, true), (true, false), Some((false, false)), false),
        OutputNativeFramePhase::CleanupPending
    );
    assert_eq!(
        native_service_phase((false, false), (false, true), Some((false, false)), true),
        OutputNativeFramePhase::CleanupPending
    );
}
