use super::*;

#[test]
fn shared_charges_live_until_the_last_owner_and_limits_do_not_reset() {
    let budget = NativeRendererSnapshotBudget::new(2, 100);
    let first = budget.reserve(60).unwrap();
    let in_flight = first.clone();
    assert!(budget.reserve(41).is_err());
    let second = budget.reserve(40).unwrap();
    assert!(budget.reserve(1).is_err());
    drop(first);
    assert_eq!(budget.usage(), (2, 100));
    drop(in_flight);
    assert_eq!(budget.usage(), (1, 40));
    drop(second);
    assert_eq!(budget.usage(), (0, 0));
    let a = budget.reserve(1).unwrap();
    let b = budget.reserve(1).unwrap();
    assert!(budget.reserve(1).is_err());
    drop((a, b));
    assert!(budget.reserve(0).is_err());
}

#[test]
fn reset_invalidates_every_queued_reference_without_releasing_storage() {
    let budget = NativeRendererSnapshotBudget::new(1, 100);
    let charge = budget.reserve(100).unwrap();
    let epoch = NativeRendererSnapshotEpoch::new(1, 2, 3);
    let queued = epoch.clone();
    epoch.invalidate();
    assert!(!queued.is_valid());
    assert!(budget.reserve(100).is_err());
    drop(charge);
    assert!(budget.reserve(100).is_ok());
}
