// Accounting and descriptor custody only. A memfd is not a captured DMA-BUF,
// and these controls neither submit GPU work nor prove GPU completion.
use super::*;
use crate::{OwnedDmaBufPlane, RendererImageId};
use std::os::fd::AsRawFd;

fn snapshot(bytes: u64) -> RendererImageSnapshot {
    let fd = rustix::fs::memfd_create(c"snapshot-budget", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&fd, bytes).unwrap();
    RendererImageSnapshot {
        image_id: RendererImageId::from_raw(7),
        width: 4,
        height: 4,
        format: 0x3432_5258,
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
        charge: None,
        epoch: None,
    }
}

#[test]
fn uncharged_data_is_not_retained_custody() {
    let image = snapshot(64);
    let raw = RendererImageSnapshot::uncharged(
        image.image_id,
        crate::OwnedSnapshotData {
            width: image.width,
            height: image.height,
            format: image.format,
            modifier: image.modifier,
            plane_count: image.plane_count,
            planes: image.planes,
        },
    );
    assert!(!raw.is_retained());
    assert!(!Arc::new(raw).custody_is_exclusive());
}

#[test]
fn resource_retirement_waits_for_same_object_and_duplicate_descriptor_consumers() {
    let budget = SnapshotBudget::new(1, 100);
    let epoch = SnapshotEpoch::new(1, 2, 3);
    let original = budget
        .retain_reserved(
            snapshot(64),
            epoch.clone(),
            budget.reserve_capture(100).unwrap(),
        )
        .unwrap();
    assert!(original.is_retained());
    assert!(original.custody_is_exclusive());
    assert_eq!(budget.usage(), (1, 64));

    let cached_import = original.clone();
    assert!(!original.custody_is_exclusive());
    let replacement_renderer = Arc::new(original.try_clone().unwrap());
    drop(cached_import);
    // Same-object uniqueness alone misses the replacement's duplicated FDs.
    assert_eq!(Arc::strong_count(&original), 1);
    assert!(!original.custody_is_exclusive());
    assert!(!replacement_renderer.custody_is_exclusive());
    assert_eq!(budget.usage(), (1, 64));

    original.retire_import_cache();
    assert!(!replacement_renderer.import_cacheable());
    assert!(original.is_current());
    assert!(replacement_renderer.is_current());
    epoch.invalidate();
    assert!(!original.is_current());
    assert!(!original.custody_is_exclusive());
    assert_eq!(budget.usage(), (1, 64));
    drop(original);
    assert!(replacement_renderer.custody_is_exclusive());
    assert_eq!(budget.usage(), (1, 64));
    drop(replacement_renderer);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn reservation_limits_apply_before_capture_and_rollback_returns_capacity() {
    let budget = SnapshotBudget::new(2, 100);
    assert!(budget.reserve_capture(0).is_err());
    assert!(budget.reserve_capture(u64::MAX).is_err());
    let first = budget.reserve_capture(60).unwrap();
    assert_eq!(first.byte_bound(), 60);
    assert!(budget.reserve_capture(41).is_err());
    let second = budget.reserve_capture(40).unwrap();
    assert_eq!(budget.usage(), (2, 100));
    assert!(budget.reserve_capture(1).is_err());
    drop(first);
    assert_eq!(budget.usage(), (1, 40));
    drop(second);
    assert_eq!(budget.usage(), (0, 0));
    let a = budget.reserve_capture(1).unwrap();
    let b = budget.reserve_capture(1).unwrap();
    assert!(budget.reserve_capture(1).is_err());
    drop((a, b));
}

#[test]
fn retention_transfers_one_charge_and_counts_shared_planes_once() {
    let budget = SnapshotBudget::new(1, 100);
    let reservation = budget.reserve_capture(100).unwrap();
    let mut image = snapshot(64);
    image.planes[1] = Some(OwnedDmaBufPlane {
        fd: image.planes[0].as_ref().unwrap().fd.try_clone().unwrap(),
        offset: 32,
        stride: 16,
    });
    image.plane_count = 2;
    let epoch = SnapshotEpoch::new(1, 2, 3);
    let retained = budget
        .retain_reserved(image, epoch.clone(), reservation)
        .unwrap();
    assert_eq!(budget.usage(), (1, 64));
    let queued = retained.try_clone().unwrap();
    epoch.invalidate();
    assert!(!queued.is_current());
    drop(retained);
    assert_eq!(budget.usage(), (1, 64));
    assert!(budget.reserve_capture(1).is_err());
    drop(queued);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn retention_refusals_return_the_original_descriptors_and_full_reservation() {
    for case in [
        "epoch",
        "budget",
        "oversize",
        "zero",
        "descriptor",
        "charged",
    ] {
        let budget = SnapshotBudget::new(2, 200);
        let other = SnapshotBudget::new(2, 200);
        let reservation = budget.reserve_capture(100).unwrap();
        let epoch = SnapshotEpoch::new(1, 2, 3);
        let mut image = snapshot(match case {
            "oversize" => 101,
            "zero" => 0,
            _ => 64,
        });
        if case == "epoch" {
            epoch.invalidate();
        }
        if case == "charged" {
            image.charge = Some(budget.reserve(64).unwrap());
        }
        if case == "descriptor" {
            let (socket, _) = std::os::unix::net::UnixStream::pair().unwrap();
            image.planes[0].as_mut().unwrap().fd = socket.into();
        }
        let fd = image.planes[0].as_ref().unwrap().fd.as_raw_fd();
        let usage = budget.usage();
        let receiver = if case == "budget" { &other } else { &budget };
        let failure = receiver
            .retain_reserved(image, epoch, reservation)
            .unwrap_err();
        assert_eq!(
            failure.snapshot.planes[0].as_ref().unwrap().fd.as_raw_fd(),
            fd,
            "{case}"
        );
        assert!(rustix::fs::fstat(&failure.snapshot.planes[0].as_ref().unwrap().fd).is_ok());
        assert_eq!(failure.reservation.byte_bound(), 100, "{case}");
        assert_eq!(budget.usage(), usage, "{case}");
        assert_eq!(other.usage(), (0, 0), "{case}");
        drop(failure);
        assert_eq!(budget.usage(), (0, 0), "{case}");
    }
}

#[test]
fn a_refused_reservation_can_be_retried_without_losing_capacity() {
    let budget = SnapshotBudget::new(1, 100);
    let reservation = budget.reserve_capture(100).unwrap();
    // Only this private fault seam can duplicate a reservation's charge.
    let unexpected_owner = reservation.charge.clone();
    let epoch = SnapshotEpoch::new(1, 2, 3);
    let failure = budget
        .retain_reserved(snapshot(64), epoch.clone(), reservation)
        .unwrap_err();
    assert_eq!(budget.usage(), (1, 100));
    drop(unexpected_owner);
    let image = budget
        .retain_reserved(failure.snapshot, epoch, failure.reservation)
        .unwrap();
    assert_eq!(budget.usage(), (1, 64));
    drop(image);
    assert_eq!(budget.usage(), (0, 0));
}
