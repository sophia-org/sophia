//! Deterministic interruption of the real pread adapter at worker checkpoints.
use crate::*;
use std::cell::Cell;
use std::os::unix::fs::FileExt;

#[test]
fn diagnostic_copy_counts_exact_bytes_and_keeps_failure_and_release_semantics() {
    for short in [false, true] {
        let rows = crate::shell_work_cost::tests::test_snapshot(|| {
            let geometry = OwnedRingGeometry::new(1, 8).unwrap();
            let file = std::fs::File::from(
                rustix::fs::memfd_create("ring-cost", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
            );
            file.set_len(geometry.source_bytes() as u64).unwrap();
            file.write_all_at(&2_u64.to_le_bytes(), 0).unwrap();
            let pixels = [1, 2, 3, 255, 5, 6, 7, 255];
            file.write_all_at(&pixels, 4096).unwrap();
            let mut ring = OwnedRing::admit(file.try_clone().unwrap(), reserved(geometry)).unwrap();
            let id = OwnedSlotIdentity {
                offer: 1,
                slot: 0,
                generation: 2,
            };
            let read = ring.offer(id, 8).unwrap();
            if short {
                file.set_len(4099).unwrap();
            }
            let result = read.copy(|| Ok(()));
            if short {
                assert_eq!(result.err(), Some(IsbValidationFailure::SizeMismatch));
            } else {
                assert_eq!(&*result.unwrap(), &pixels);
            }
            assert_eq!(ring.pending_release(0), Some(id));
            assert!(ring.confirm_release(id));
        });
        for name in ["copy", "allocate", "read"] {
            let row = rows
                .iter()
                .find(|r| r.contains(&format!(" stage={name} ")))
                .unwrap();
            assert!(row.contains("calls=1"));
            assert!(row.contains("bytes=8"));
            assert!(row.contains(if short && name != "allocate" {
                "ok_bytes=0"
            } else {
                "ok_bytes=8"
            }));
        }
    }
}

#[test]
fn owned_ring_changed_after_payload_read_is_rejected_without_retry() {
    let geometry = OwnedRingGeometry::new(1, 8).unwrap();
    let file = std::fs::File::from(
        rustix::fs::memfd_create("ring-race", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    file.write_all_at(&2_u64.to_le_bytes(), 0).unwrap();
    let mut ring = OwnedRing::admit(file.try_clone().unwrap(), reserved(geometry)).unwrap();
    let identity = OwnedSlotIdentity {
        offer: 1,
        slot: 0,
        generation: 2,
    };
    let checks = Cell::new(0);
    let result = ring.offer(identity, 8).unwrap().copy(|| {
        checks.set(checks.get() + 1);
        if checks.get() == 4 {
            file.write_all_at(&4_u64.to_le_bytes(), 0).unwrap();
        }
        Ok(())
    });
    assert_eq!(result.err(), Some(IsbValidationFailure::GenerationChanged));
    assert_eq!(checks.get(), 4, "no recopy or post-failure progress");
    assert_eq!(ring.pending_release(0), Some(identity));
}

#[test]
fn owned_ring_cancellation_between_reads_releases_only_the_source_ticket() {
    let geometry = OwnedRingGeometry::new(1, 8).unwrap();
    let file = std::fs::File::from(
        rustix::fs::memfd_create("ring-cancel", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    file.write_all_at(&2_u64.to_le_bytes(), 0).unwrap();
    let mut ring = OwnedRing::admit(file, reserved(geometry)).unwrap();
    let identity = OwnedSlotIdentity {
        offer: 1,
        slot: 0,
        generation: 2,
    };
    let checks = Cell::new(0);
    let result = ring.offer(identity, 8).unwrap().copy(|| {
        checks.set(checks.get() + 1);
        if checks.get() == 3 {
            Err(IsbValidationFailure::Cancelled)
        } else {
            Ok(())
        }
    });
    assert_eq!(result.err(), Some(IsbValidationFailure::Cancelled));
    assert_eq!(checks.get(), 3);
    assert_eq!(
        ring.outstanding(),
        1,
        "read ending does not consume journal obligation"
    );
    assert_eq!(ring.pending_release(0), Some(identity));
    assert!(ring.confirm_release(identity));
}

fn reserved(geometry: OwnedRingGeometry) -> OwnedRingSourceReservation {
    let grant = sophia_protocol::ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    registry
        .admit(sophia_protocol::ContentLimits::prototype(grant))
        .unwrap();
    registry.reserve_owned_ring_source(grant, geometry).unwrap()
}
