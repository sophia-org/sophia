use sophia_portal::capture_frame::*;
use sophia_protocol::{OutputId, output_capture::OutputCaptureFrameInfo};
use std::sync::{Arc, Barrier};

fn info() -> OutputCaptureFrameInfo {
    OutputCaptureFrameInfo {
        output: OutputId::from_raw(1),
        output_generation: 1,
        presentation_generation: 1,
        width: 2,
        height: 2,
    }
}

#[test]
fn cancelled_invalid_and_shared_images_release_only_their_own_charge() {
    let budget = CaptureRetentionBudget::new(32, 2).unwrap();
    let pending = budget.reserve(info()).unwrap();
    let invalid = budget.reserve(info()).unwrap();
    assert!(budget.reserve(info()).is_none());
    assert!(invalid.complete(vec![0; 15].into()).is_none());
    assert_eq!(
        budget.usage().unwrap(),
        CaptureRetentionUsage {
            bytes: 16,
            frames: 1
        }
    );
    let frame = Arc::new(pending.complete(vec![0; 16].into()).unwrap());
    let retained = frame.clone();
    drop(frame);
    assert_eq!(budget.usage().unwrap().bytes, 16);
    let pending = budget.reserve(info()).unwrap();
    drop(retained);
    assert_eq!(budget.usage().unwrap().bytes, 16);
    drop(pending);
    assert_eq!(budget.usage().unwrap(), CaptureRetentionUsage::default());
}

#[test]
fn frame_count_and_byte_limits_are_independent_and_cannot_be_raised() {
    for (bytes, frames) in [(16, 2), (32, 1)] {
        let budget = CaptureRetentionBudget::new(bytes, frames).unwrap();
        let _pending = budget.reserve(info()).unwrap();
        assert!(budget.reserve(info()).is_none());
        assert_eq!(budget.usage().unwrap().frames, 1);
    }
    for (bytes, frames) in [
        (0, 1),
        (16, 0),
        (MAX_CAPTURE_RETAINED_BYTES + 1, 1),
        (16, MAX_CAPTURE_RETAINED_FRAMES + 1),
    ] {
        assert!(CaptureRetentionBudget::new(bytes, frames).is_none());
    }
    let budget = CaptureRetentionBudget::default();
    let mut invalid = info();
    invalid.width = 0;
    assert!(budget.reserve(invalid).is_none());
    assert_eq!(budget.usage().unwrap(), CaptureRetentionUsage::default());
}

#[test]
fn simultaneous_reservations_cannot_overbook_a_shared_budget() {
    let budget = CaptureRetentionBudget::new(32, 2).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let budget = budget.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let reservation = budget.reserve(info());
                // Keep every successful reservation until every contender tried.
                barrier.wait();
                reservation
            })
        })
        .collect();
    let reservations: Vec<_> = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(reservations.len(), 2);
    assert_eq!(
        budget.usage().unwrap(),
        CaptureRetentionUsage {
            bytes: 32,
            frames: 2
        }
    );
    drop(reservations);
    assert_eq!(budget.usage().unwrap(), CaptureRetentionUsage::default());
}
