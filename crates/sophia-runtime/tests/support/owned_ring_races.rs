//! Deterministic interruption of the real pread adapter at worker checkpoints.
use crate::*;
use std::cell::Cell;
use std::os::unix::fs::FileExt;

#[test]
fn owned_ring_changed_after_payload_read_is_rejected_without_retry() {
    let geometry = OwnedRingGeometry::new(1, 8).unwrap();
    let file = std::fs::File::from(
        rustix::fs::memfd_create("ring-race", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    file.write_all_at(&2_u64.to_le_bytes(), 0).unwrap();
    let mut ring =
        OwnedRing::admit(file.try_clone().unwrap(), geometry, geometry.source_bytes()).unwrap();
    let identity = OwnedSlotIdentity {
        offer: 1,
        slot: 0,
        generation: 2,
    };
    let checks = Cell::new(0);
    let result = ring.offer(identity).unwrap().copy(|| {
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
    let mut ring = OwnedRing::admit(file, geometry, geometry.source_bytes()).unwrap();
    let identity = OwnedSlotIdentity {
        offer: 1,
        slot: 0,
        generation: 2,
    };
    let checks = Cell::new(0);
    let result = ring.offer(identity).unwrap().copy(|| {
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
