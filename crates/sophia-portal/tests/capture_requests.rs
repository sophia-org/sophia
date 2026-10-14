use sophia_portal::{capture_requests::*, execution_fence::PortalExecutionFence};
use sophia_protocol::{OutputId, output_capture_request::*};

fn request(serial: u64) -> OutputCaptureRequest {
    OutputCaptureRequest {
        serial,
        output: OutputId::from_raw(7),
    }
}

#[test]
fn one_slot_refuses_pressure_replay_and_late_completion() {
    let queue = CaptureRequestQueue::new(PortalExecutionFence::default());
    assert_eq!(queue.status().state, OutputCaptureState::Idle);
    assert_eq!(queue.submit(request(1)), Ok(()));
    assert_eq!(queue.submit(request(2)), Err(CaptureRequestError::Busy));
    let first = queue.take().unwrap();
    assert_eq!(first.request(), request(1));
    assert!(queue.take().is_none());
    assert!(queue.current(first));
    assert!(!queue.finish(first, OutputCaptureState::Queued));
    assert!(queue.finish(first, OutputCaptureState::Denied));
    assert_eq!(queue.submit(request(1)), Err(CaptureRequestError::Stale));
    assert_eq!(queue.submit(request(2)), Ok(()));
    assert!(!queue.finish(first, OutputCaptureState::Ready));
    let second = queue.take().unwrap();
    assert!(queue.finish(second, OutputCaptureState::Ready));
    assert_eq!(
        queue.status(),
        OutputCaptureStatus {
            request: Some(request(2)),
            state: OutputCaptureState::Ready
        }
    );
}

#[test]
fn equal_wire_serials_never_select_another_recipient_owner() {
    let fence = PortalExecutionFence::default();
    let first = CaptureRequestQueue::new(fence.clone());
    let second = CaptureRequestQueue::new(fence);
    first.submit(request(1)).unwrap();
    second.submit(request(1)).unwrap();
    let work = first.take().unwrap();
    let other = second.take().unwrap();
    assert!(!second.current(work));
    assert!(!second.finish(work, OutputCaptureState::Ready));
    assert!(second.current(other));
}

#[test]
fn lock_cancels_queued_and_active_requests_and_unlock_cannot_revive_them() {
    for active in [false, true] {
        let fence = PortalExecutionFence::default();
        let queue = CaptureRequestQueue::new(fence.clone());
        queue.submit(request(1)).unwrap();
        let work = active.then(|| queue.take().unwrap());
        fence.block();
        assert_eq!(queue.submit(request(2)), Err(CaptureRequestError::Denied));
        fence.unblock();
        if let Some(work) = work {
            assert!(!queue.current(work));
            assert!(!queue.finish(work, OutputCaptureState::Ready));
        }
        assert!(queue.take().is_none());
        assert_eq!(queue.status().state, OutputCaptureState::Cancelled);
        queue.submit(request(2)).unwrap();
        assert!(queue.current(queue.take().unwrap()));
        fence.close();
        fence.unblock();
        assert_eq!(queue.submit(request(3)), Err(CaptureRequestError::Denied));
    }
}

#[test]
fn request_records_are_canonical_and_output_only() {
    let bytes = request(1).encode().unwrap();
    assert_eq!(OutputCaptureRequest::decode(&bytes), Some(request(1)));
    assert!(OutputCaptureRequest::decode(&bytes[..23]).is_none());
    assert!(OutputCaptureRequest::decode(&[bytes.as_slice(), &[0]].concat()).is_none());
    for offset in [0, 8, 16] {
        let mut invalid = bytes;
        invalid[offset..offset + 8].fill(0);
        assert!(OutputCaptureRequest::decode(&invalid).is_none());
    }
    for state in [
        OutputCaptureState::Idle,
        OutputCaptureState::Queued,
        OutputCaptureState::Ready,
        OutputCaptureState::Cancelled,
    ] {
        let status = OutputCaptureStatus {
            request: (state != OutputCaptureState::Idle).then(|| request(1)),
            state,
        };
        let bytes = status.encode().unwrap();
        assert_eq!(OutputCaptureStatus::decode(&bytes), Some(status));
        let mut invalid = bytes;
        invalid[24] = 255;
        assert!(OutputCaptureStatus::decode(&invalid).is_none());
    }
}
