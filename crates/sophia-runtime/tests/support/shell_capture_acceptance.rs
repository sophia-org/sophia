//! Supplied retained storage tests accounting, not DMA-BUF capture or GPU reads.
use super::*;
use sophia_renderer_image::{
    OwnedDmaBufPlane, OwnedSnapshotData, RendererImageId, SnapshotBudget, SnapshotEpoch,
};

fn image(
    budget: &Arc<SnapshotBudget>,
    epoch: &Arc<SnapshotEpoch>,
    width: u32,
) -> Arc<ContentCapturedImage> {
    budget.retain(uncharged(width), epoch.clone()).unwrap()
}

fn uncharged(width: u32) -> ContentCapturedImage {
    let fd =
        rustix::fs::memfd_create(c"capture-accounting", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&fd, 64).unwrap();
    ContentCapturedImage::uncharged(
        RendererImageId::from_raw(7),
        OwnedSnapshotData {
            width,
            height: 4,
            format: DRM_FORMAT_XRGB8888,
            modifier: 0,
            plane_count: 1,
            planes: [
                Some(OwnedDmaBufPlane {
                    fd,
                    offset: 0,
                    stride: 16,
                }),
                None,
                None,
                None,
            ],
        },
    )
}

#[test]
fn unretained_storage_cannot_be_accepted_as_a_captured_resource() {
    let mut store = owner();
    let mut ticket = reserve(&mut store, 10);
    let mut snapshot = Some(Arc::new(uncharged(4)));
    assert!(
        store
            .accept_capture(&mut ticket, &mut snapshot, 10)
            .is_err()
    );
    assert!(ticket.is_some());
    assert!(snapshot.is_some());
    assert_eq!(store.usage().resident, 0);
    store
        .fail_capture(&mut ticket, ContentCaptureFailure::Renderer, 10)
        .unwrap();
}

fn accepted() -> (
    ContentResourceStore,
    Arc<SnapshotBudget>,
    Arc<SnapshotEpoch>,
) {
    let mut store = owner();
    let budget = SnapshotBudget::new(1, 64);
    let epoch = SnapshotEpoch::new(1, 2, 3);
    let mut ticket = reserve(&mut store, 0);
    let mut snapshot = Some(image(&budget, &epoch, 4));
    store.accept_capture(&mut ticket, &mut snapshot, 0).unwrap();
    assert!(ticket.is_none() && snapshot.is_none());
    assert_eq!(
        store.usage(),
        ContentMemoryUsage {
            resident: 64,
            backing: 64,
            ..Default::default()
        }
    );
    assert_eq!(budget.usage(), (1, 64));
    assert_eq!(store.response_credits, 1);
    let event = store.take_event().unwrap();
    assert!(matches!(event.record,ShellContentRecord::ResourceStatus(s) if s.status==2));
    assert!(store.take_event().is_none());
    (store, budget, epoch)
}

fn retire(store: &mut ContentResourceStore) {
    store
        .retire(
            TransactionId::from_raw(8),
            &ContentResourceRetire {
                grant: grant(),
                resource: description(1).resource,
            },
        )
        .unwrap();
}

#[test]
fn accepted_capture_has_no_cpu_view_and_release_waits_for_eviction_and_every_storage_owner() {
    let (mut store, budget, epoch) = accepted();
    let lease = store.lease(grant(), description(1).resource).unwrap();
    assert!(lease.cpu_bytes().is_none());
    assert!(lease.clone().into_cpu().is_err());
    let imported = lease.captured_image().unwrap().clone();
    let duplicate = Arc::new(imported.try_clone().unwrap());
    retire(&mut store);
    drop(lease);
    assert!(store.capture_retirement_pending());
    assert!(!store.isb_release_pending(description(1).resource));
    let mut calls = 0;
    let mut evict = |_: &Arc<ContentCapturedImage>| {
        calls += 1;
        Ok::<_, ()>(())
    };
    store.retire_capture_imports(&mut 0, &mut evict).unwrap();
    assert!(!imported.import_cacheable());
    assert!(epoch.is_valid());
    store.retire_capture_imports(&mut 1, &mut evict).unwrap();
    assert_eq!(calls, 1);
    assert!(!store.capture_retirement_pending());
    assert!(store.take_event().is_none());
    drop(imported);
    store.collect();
    assert!(store.take_event().is_none()); // duplicated FDs still share the charge
    drop(duplicate);
    store.collect();
    assert_eq!(
        store.confirm_isb_release(description(1).resource),
        Err(ContentStoreError::Incomplete)
    );
    assert_eq!(store.usage().backing, 64);
    assert_eq!(budget.usage(), (1, 64));
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceReleased(_)
    ));
    store.confirm_isb_release(description(1).resource).unwrap();
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    assert_eq!(budget.usage(), (0, 0));
    assert!(store.take_event().is_none());
}

#[test]
fn eviction_error_and_revocation_retain_both_budgets_and_retry_without_publication() {
    let (mut store, budget, epoch) = accepted();
    store.revoke();
    assert!(store.capture_retirement_pending());
    epoch.invalidate();
    assert_eq!(
        store.retire_capture_imports(&mut 1, &mut |_| Err("context unavailable")),
        Err("context unavailable")
    );
    assert_eq!(budget.usage(), (1, 64));
    assert_eq!(store.usage().backing, 64);
    assert!(store.take_event().is_none());
    store
        .retire_capture_imports(&mut 1, &mut |_| Ok::<_, ()>(()))
        .unwrap();
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    assert_eq!(budget.usage(), (0, 0));
    assert!(
        matches!(store.take_event().unwrap().record,ShellContentRecord::ResourceReleased(r) if r.reason==ContentReason::Revoked as u16)
    );
    assert!(store.take_event().is_none());
}

#[test]
fn acceptance_refusals_preserve_exact_ticket_and_snapshot_for_failure_settlement() {
    for case in 0..7 {
        let mut store = owner();
        let mut ticket = reserve(&mut store, 10);
        let budget = SnapshotBudget::new(1, 64);
        let epoch = SnapshotEpoch::new(1, 2, 3);
        let mut snapshot = Some(image(&budget, &epoch, if case == 0 { 3 } else { 4 }));
        match case {
            1 => epoch.invalidate(),
            2 => store.revoke(),
            3 => {
                store
                    .cancel(
                        TransactionId::from_raw(8),
                        &ContentResourceCancel {
                            grant: grant(),
                            resource: description(1).resource,
                        },
                    )
                    .unwrap();
            }
            4 => {
                let mut other = owner();
                ticket = reserve(&mut other, 10);
            }
            5 => {
                store
                    .expire(u64::from(store.limits.transfer_timeout_ms) + 10)
                    .unwrap();
            }
            _ => {}
        }
        let now = if case == 6 { 9 } else { store.last_now };
        let held = snapshot.as_ref().unwrap().clone();
        assert!(
            store
                .accept_capture(&mut ticket, &mut snapshot, now)
                .is_err()
        );
        assert!(ticket.is_some());
        assert!(Arc::ptr_eq(snapshot.as_ref().unwrap(), &held));
        assert_eq!(store.usage().staging, 64);
        assert_eq!(budget.usage(), (1, 64));
        assert!(store.take_event().is_none());
        if case != 4 {
            let now = store.last_now;
            store
                .fail_capture(&mut ticket, ContentCaptureFailure::Renderer, now)
                .unwrap();
            assert_eq!(store.usage(), ContentMemoryUsage::default());
            assert_eq!(budget.usage(), (1, 64)); // receipt/source owner still owns output
        }
    }
}
