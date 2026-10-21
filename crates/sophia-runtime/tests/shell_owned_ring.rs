//! Local D4 source/worker custody; no authenticated wire or native qualification.
#![cfg(feature = "experimental-owned-copy")]
use sophia_runtime::*;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

fn source() -> (File, OwnedRingGeometry, OwnedRing) {
    let geometry = OwnedRingGeometry::new(2, 8).unwrap();
    // Intentionally unsealed: Session uses only positional reads.
    let file = File::from(
        rustix::fs::memfd_create("owned-ring-test", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    let ring =
        OwnedRing::admit(file.try_clone().unwrap(), geometry, geometry.source_bytes()).unwrap();
    (file, geometry, ring)
}
fn publish(file: &File, geometry: OwnedRingGeometry, identity: OwnedSlotIdentity, value: u8) {
    let offset = (identity.slot * geometry.slot_extent()) as u64;
    file.write_all_at(&[value; 8], offset + 4096).unwrap();
    file.write_all_at(&identity.generation.to_le_bytes(), offset)
        .unwrap();
}
fn identity(offer: u64, slot: usize, generation: u64) -> OwnedSlotIdentity {
    OwnedSlotIdentity {
        offer,
        slot,
        generation,
    }
}
fn complete(worker: &mut IsbValidationWorker) -> IsbValidationCompletion {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(done) = worker.take_completed().unwrap() {
            return done;
        }
        assert!(Instant::now() < deadline, "worker deadline");
        let mut fds = [rustix::event::PollFd::from_borrowed_fd(
            worker.readiness(),
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            }),
        )
        .unwrap();
    }
}
fn submit(worker: &mut IsbValidationWorker, read: OwnedRingRead, deadline: Instant) -> u64 {
    let job = worker.reserve(8, deadline).unwrap();
    assert!(worker.submit_ring_reserved(job, read).is_ok());
    job
}

#[test]
fn slot_reuse_is_independent_of_retained_accepted_pixels() {
    let (file, geometry, mut ring) = source();
    let mut worker = IsbValidationWorker::new(1, 8).unwrap();
    let mut retained = Vec::new();
    for offer in 1..=8 {
        let identity = identity(offer, offer as usize % 2, offer * 2);
        publish(&file, geometry, identity, offer as u8);
        let read = ring.offer(identity).unwrap();
        assert_eq!(ring.pending_release(identity.slot), None);
        assert!(!ring.confirm_release(identity));
        let job = submit(&mut worker, read, Instant::now() + Duration::from_secs(5));
        let done = complete(&mut worker);
        assert_eq!(done.job, job);
        let pixels = done.result.unwrap();
        let address = pixels.bytes().as_ptr();
        retained.push(pixels.into_shared());
        assert_eq!(retained.last().unwrap().as_ptr(), address);
        assert_eq!(ring.pending_release(identity.slot), Some(identity));
        assert_eq!(ring.pending_release(identity.slot), Some(identity));
        assert!(
            ring.offer(OwnedSlotIdentity {
                offer: offer + 1,
                generation: identity.generation + 2,
                ..identity
            })
            .is_err()
        );
        assert!(!ring.confirm_release(OwnedSlotIdentity {
            offer: 999,
            ..identity
        }));
        assert!(ring.confirm_release(identity));
        assert!(!ring.confirm_release(identity));
        assert_eq!(ring.outstanding(), 0);
    }
    drop(ring);
    file.set_len(0).unwrap();
    for (i, pixels) in retained.iter().enumerate() {
        assert_eq!(pixels.as_ref(), &[(i + 1) as u8; 8]);
    }
}

#[test]
fn rejected_submit_keeps_ticket_and_original_credit_until_explicit_reconciliation() {
    let (_, _, mut ring) = source();
    let identity = identity(1, 0, 2);
    let read = ring.offer(identity).unwrap();
    let mut worker = IsbValidationWorker::new(1, 8).unwrap();
    let job = worker
        .reserve(4, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let (error, read) = worker.submit_ring_reserved(job, read).err().unwrap();
    assert_eq!(error, IsbSubmitFailure::Invalid);
    assert_eq!(ring.pending_release(0), None);
    assert_eq!(worker.pending(), 1);
    drop(read);
    assert_eq!(ring.pending_release(0), Some(identity));
    assert!(worker.release_reserved(job));
    assert!(ring.confirm_release(identity));
    assert!(
        ring.offer(OwnedSlotIdentity {
            offer: 2,
            slot: 0,
            generation: 2
        })
        .is_err()
    );
}

#[test]
fn truncation_changed_generation_bad_pixels_and_expiry_settle_once() {
    for case in 0..4 {
        let (file, geometry, mut ring) = source();
        let identity = identity(1, 0, 2);
        publish(&file, geometry, identity, 0);
        match case {
            0 => file.set_len(4097).unwrap(),
            1 => file.write_all_at(&4_u64.to_le_bytes(), 0).unwrap(),
            2 => file.write_all_at(&[255, 0, 0, 0], 4096).unwrap(),
            _ => (),
        }
        let mut worker = IsbValidationWorker::new(1, 8).unwrap();
        let deadline = if case == 3 {
            Instant::now() - Duration::from_secs(1)
        } else {
            Instant::now() + Duration::from_secs(5)
        };
        let job = submit(&mut worker, ring.offer(identity).unwrap(), deadline);
        let done = complete(&mut worker);
        assert_eq!(done.job, job);
        assert_eq!(
            done.result.err(),
            Some(match case {
                0 => IsbValidationFailure::SizeMismatch,
                1 => IsbValidationFailure::GenerationChanged,
                2 => IsbValidationFailure::Malformed,
                _ => IsbValidationFailure::Timeout,
            })
        );
        assert_eq!(ring.pending_release(0), Some(identity));
        assert!(worker.take_completed().unwrap().is_none());
        assert!(ring.confirm_release(identity));
    }
}

#[test]
fn admission_checks_geometry_extent_reservation_and_non_wrapping_identity() {
    for (slots, bytes) in [
        (0, 8),
        (5, 8),
        (1, 0),
        (1, 7),
        (1, (4 << 20) + 4),
        (usize::MAX, usize::MAX),
    ] {
        assert!(OwnedRingGeometry::new(slots, bytes).is_err());
    }
    let (file, geometry, mut ring) = source();
    assert!(
        OwnedRing::admit(
            file.try_clone().unwrap(),
            geometry,
            geometry.source_bytes() - 1
        )
        .is_err()
    );
    assert!(
        OwnedRing::admit(
            File::open("/dev/null").unwrap(),
            geometry,
            geometry.source_bytes()
        )
        .is_err()
    );
    for identity in [
        identity(0, 0, 2),
        identity(1, 2, 2),
        identity(1, 0, 0),
        identity(1, 0, 3),
    ] {
        assert!(ring.offer(identity).is_err());
    }
    let last = identity(u64::MAX, 0, u64::MAX - 1);
    drop(ring.offer(last).unwrap());
    assert!(ring.confirm_release(last));
    assert!(ring.offer(identity(1, 0, 2)).is_err());
    file.set_len(1).unwrap();
    assert!(OwnedRing::admit(file, geometry, geometry.source_bytes()).is_err());
}
