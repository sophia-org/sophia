#![cfg(feature = "experimental-shell-capture")]
use sophia_renderer_image::*;
use std::{sync::Arc, time::Instant};

fn prepare(
    budget: &Arc<SnapshotBudget>,
    epoch: &Arc<SnapshotEpoch>,
    wake: &sophia_wake::Wake,
    charge: Option<ManagedCaptureCharge>,
) -> (PreparedCaptureDestination, PreparedCaptureWatch) {
    PreparedCaptureDestination::new(
        PreparedCaptureInfo {
            allocation_id: 1,
            width: 4,
            height: 4,
            bytes: 64,
            bound: 96,
        },
        budget.clone(),
        epoch.clone(),
        charge,
        wake.notifier(),
    )
    .unwrap()
}

#[test]
fn reused_allocation_keeps_one_charge_after_logical_custody_ends() {
    let wake = sophia_wake::Wake::new().unwrap();
    let budget = SnapshotBudget::new(1, 96);
    let epoch = SnapshotEpoch::new(1, 1, 1);
    let (token, watch) = prepare(&budget, &epoch, &wake, None);
    let reservation = token.reserve(&budget).unwrap();
    assert_eq!(budget.usage(), (1, 96));
    assert!(watch.matches(reservation.prepared().unwrap()));
    let (other_token, _) = prepare(&budget, &epoch, &wake, None);
    assert!(
        !watch.matches(&other_token),
        "identical metadata is not the same preparation"
    );
    drop(other_token);
    assert!(reservation.prepared().unwrap().consume());
    assert!(!reservation.prepared().unwrap().consume());
    let charge = reservation.retain_managed(&budget, &epoch, 64).unwrap();
    assert_eq!(budget.usage(), (1, 64));
    assert!(!watch.is_live());
    let (token, _) = prepare(&budget, &epoch, &wake, Some(charge.clone()));
    let reservation = token.reserve(&budget).unwrap();
    assert_eq!(budget.usage(), (1, 64));
    assert!(reservation.prepared().unwrap().consume());
    let reused = reservation.retain_managed(&budget, &epoch, 64).unwrap();
    let lease = ManagedCaptureLease::new(
        ManagedCaptureInfo {
            image_id: RendererImageId::from_raw(1),
            allocation_id: 1,
            generation: 2,
            width: 4,
            height: 4,
            bytes: 64,
        },
        epoch,
        wake.notifier(),
    )
    .unwrap();
    let image = Arc::new(CapturedImage::Managed(lease.clone()));
    assert!(!CapturedImage::custody_is_exclusive(&image));
    drop(lease);
    assert!(CapturedImage::custody_is_exclusive(&image));
    image.retire_import_cache();
    assert!(image.is_current()); // Retirement does not invalidate queued frames.
    drop(image);
    drop(reused);
    assert_eq!(budget.usage(), (1, 64));
    drop(charge);
    assert_eq!(budget.usage(), (0, 0));
    assert_eq!(budget.peak_usage(), (1, 96));
}

#[test]
fn cancellation_wakes_owner_and_wrong_budget_does_not_spend_credit() {
    let wake = sophia_wake::Wake::new().unwrap();
    let budget = SnapshotBudget::new(1, 96);
    let other = SnapshotBudget::new(1, 96);
    let epoch = SnapshotEpoch::new(1, 1, 1);
    let (token, watch) = prepare(&budget, &epoch, &wake, None);
    assert!(!wake.wait(Some(Instant::now())).unwrap());
    let (error, token) = token.reserve(&other).unwrap_err();
    assert_eq!(error, SnapshotError::InvalidRendererImageId);
    assert!(watch.is_live());
    assert_eq!(budget.usage(), (0, 0));
    assert_eq!(other.usage(), (0, 0));
    drop(token);
    assert!(!watch.is_live());
    assert!(wake.wait(Some(Instant::now())).unwrap());
}

#[test]
fn withdrawal_and_failed_reconciliation_do_not_create_managed_storage() {
    let wake = sophia_wake::Wake::new().unwrap();
    let budget = SnapshotBudget::new(1, 96);
    let epoch = SnapshotEpoch::new(1, 1, 1);
    let (token, _) = prepare(&budget, &epoch, &wake, None);
    let reservation = token.reserve(&budget).unwrap();
    let (error, reservation) = reservation.retain_managed(&budget, &epoch, 64).unwrap_err();
    assert_eq!(error, SnapshotError::InvalidRendererImageId); // Not consumed by owner.
    assert_eq!(budget.usage(), (1, 96));
    assert!(reservation.prepared().unwrap().consume());
    let (_, reservation) = reservation.retain_managed(&budget, &epoch, 65).unwrap_err();
    epoch.invalidate();
    let (_, reservation) = reservation.retain_managed(&budget, &epoch, 64).unwrap_err();
    assert_eq!(budget.usage(), (1, 96));
    drop(reservation);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn queued_managed_metadata_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<CapturedImage>();
    check::<ManagedCaptureLease>();
    check::<PreparedCaptureDestination>();
}

#[test]
fn bound_storage_keeps_credit_when_completion_is_withdrawn() {
    let wake = sophia_wake::Wake::new().unwrap();
    let budget = SnapshotBudget::new(1, 96);
    let epoch = SnapshotEpoch::new(1, 1, 1);
    let (token, _) = prepare(&budget, &epoch, &wake, None);
    let mut reservation = token.reserve(&budget).unwrap();
    assert!(reservation.prepared().unwrap().consume());
    let allocation_charge = reservation.bind_managed(&budget, &epoch, 64).unwrap();
    assert_eq!(budget.usage(), (1, 64));
    epoch.invalidate();
    let (_, reservation) = reservation.retain_managed(&budget, &epoch, 64).unwrap_err();
    drop(reservation);
    assert_eq!(budget.usage(), (1, 64));
    assert!(budget.reserve_capture(64).is_err());
    drop(allocation_charge);
    assert_eq!(budget.usage(), (0, 0));
    assert_eq!(budget.peak_usage(), (1, 96));
}

#[test]
fn retirement_and_last_queued_lease_drop_both_wake_the_owner() {
    let wake = sophia_wake::Wake::new().unwrap();
    let lease = ManagedCaptureLease::new(
        ManagedCaptureInfo {
            image_id: RendererImageId::from_raw(1),
            allocation_id: 1,
            generation: 1,
            width: 4,
            height: 4,
            bytes: 64,
        },
        SnapshotEpoch::new(1, 1, 1),
        wake.notifier(),
    )
    .unwrap();
    let queued = lease.clone();
    lease.retire();
    assert!(wake.wait(Some(Instant::now())).unwrap());
    wake.clear().unwrap();
    drop(lease);
    assert!(!wake.wait(Some(Instant::now())).unwrap());
    assert!(queued.is_current());
    drop(queued);
    assert!(wake.wait(Some(Instant::now())).unwrap());
}
