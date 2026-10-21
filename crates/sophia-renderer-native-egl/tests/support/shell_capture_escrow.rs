//! No GPU work: exercise the exact production custody guard with owned FDs and
//! real snapshot reservations. Intentionally quarantined objects live to exit.
use super::*;
use crate::NativeRendererSnapshotBudget;
use std::io::{ErrorKind, Read};
use std::os::unix::net::UnixStream;

fn payload() -> (
    UnixStream,
    UnixStream,
    Arc<NativeRendererSnapshotBudget>,
    crate::NativeRendererSnapshotReservation,
) {
    let (source, observer) = UnixStream::pair().unwrap();
    observer.set_nonblocking(true).unwrap();
    let budget = NativeRendererSnapshotBudget::new(1, 64);
    let reservation = budget.reserve_capture(64).unwrap();
    (source, observer, budget, reservation)
}

fn open(observer: &mut UnixStream) {
    assert_eq!(
        observer.read(&mut [0]).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
}

#[test]
fn slots_bound_all_pending_copies_and_only_safe_return_reopens_capacity() {
    let capacity = Arc::new(Capacity::default());
    let mut slots: Vec<_> = (0..MAX_COPIES)
        .map(|_| capacity.reserve().unwrap())
        .collect();
    assert!(capacity.reserve().is_none());
    slots.pop();
    assert!(capacity.reserve().is_some());
    drop(slots);
    assert_eq!(capacity.0.lock().unwrap().reserved, 0);
}

#[test]
fn completion_extracts_custody_and_returns_exactly_one_slot() {
    let capacity = Arc::new(Capacity::default());
    let (source, mut observer, budget, reservation) = payload();
    let escrow = Escrow::new((source, reservation), capacity.reserve().unwrap());
    let complete = escrow.finish();
    assert_eq!(capacity.0.lock().unwrap().reserved, 0);
    assert!(!capacity.0.lock().unwrap().quarantined);
    open(&mut observer);
    assert_eq!(budget.usage(), (1, 64));
    drop(complete);
    assert_eq!(observer.read(&mut [0]).unwrap(), 0);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn abandoned_submission_keeps_source_and_charge_and_blocks_replacement() {
    let capacity = Arc::new(Capacity::default());
    let replacement = capacity.clone();
    let (source, mut observer, budget, reservation) = payload();
    drop(Escrow::new(
        (source, reservation),
        capacity.reserve().unwrap(),
    ));
    drop(capacity);
    open(&mut observer);
    assert_eq!(budget.usage(), (1, 64));
    assert_eq!(replacement.0.lock().unwrap().reserved, 1);
    assert!(replacement.reserve().is_none());
}

#[test]
fn unwind_after_submission_is_uncertain_not_a_destructor_completion() {
    let capacity = Arc::new(Capacity::default());
    let (source, mut observer, budget, reservation) = payload();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _escrow = Escrow::new((source, reservation), capacity.reserve().unwrap());
        panic!("simulated native submission failure");
    }));
    assert!(result.is_err());
    open(&mut observer);
    assert_eq!(budget.usage(), (1, 64));
    assert!(capacity.reserve().is_none());
}

#[test]
fn one_uncertain_copy_cannot_be_hidden_by_other_successful_completions() {
    let capacity = Arc::new(Capacity::default());
    let pending = Escrow::new(1, capacity.reserve().unwrap());
    let completed = Escrow::new(2, capacity.reserve().unwrap());
    drop(pending);
    assert_eq!(completed.finish(), 2);
    assert_eq!(capacity.0.lock().unwrap().reserved, 1);
    assert!(capacity.reserve().is_none());
}
