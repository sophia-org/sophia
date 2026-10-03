//! t294: the lock object a provider reads. It names the lock epoch and phase,
//! and grants one allocation per logical output only while the cover is
//! drawn, sized to the output's largest head and valid for the contract.
use std::num::NonZeroU64;

use sophia_engine::SessionLockEpoch;
use sophia_protocol::lock_files::{LockFileLimits, LockObject, LockPhase};
use sophia_protocol::*;
use sophia_session::session_lock::{SessionLockPhase, SessionUnlockAttempt};
use sophia_session::session_lock_object::{session_lock_file_limits, session_lock_object};

fn epoch(raw: u64) -> SessionLockEpoch {
    SessionLockEpoch::from_raw(raw).unwrap()
}

fn head(id: u64, width: i32, height: i32, enabled: bool) -> OutputHeadDescriptor {
    OutputHeadDescriptor {
        head: DisplayHeadId::from_raw(id),
        generation: 1,
        label: format!("head-{id}"),
        connected: true,
        enabled,
        vrr_capable: false,
        transforms: OutputTransformSet::ALL,
        current_mode: Some(DisplayModeId::from_raw(id * 10)),
        modes: vec![OutputModeDescriptor {
            mode: DisplayModeId::from_raw(id * 10),
            pixel_size: Size { width, height },
            refresh_millihz: 60_000,
            preferred: true,
        }],
    }
}

fn group(
    output: u64,
    generation: u64,
    width: i32,
    height: i32,
    heads: &[u64],
) -> OutputLogicalGroupState {
    OutputLogicalGroupState {
        output: OutputId::from_raw(output),
        generation,
        logical: Rect {
            x: 0,
            y: 0,
            width,
            height,
        },
        members: heads
            .iter()
            .map(|head| OutputGroupMember {
                head: DisplayHeadId::from_raw(*head),
                mapping: OutputHeadMapping::Fit,
            })
            .collect(),
    }
}

/// A 1280x720 logical output mirrored on a 2560x1440 and a 1920x1080 head,
/// and a 1920x1080 output shown at 1.5x on a 2880x1620 head.
fn topology(epoch: u64, generation: u64) -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: epoch,
        primary_output: OutputId::from_raw(1),
        heads: vec![
            head(1, 2560, 1440, true),
            head(2, 1920, 1080, true),
            head(3, 2880, 1620, true),
        ],
        groups: vec![
            group(1, generation, 1280, 720, &[1, 2]),
            group(2, 1, 1920, 1080, &[3]),
        ],
    }
}

fn locked() -> SessionLockPhase {
    SessionLockPhase::Locked {
        epoch: epoch(4),
        attempt: None,
    }
}

#[test]
fn allocations_exist_only_while_the_cover_is_drawn() {
    let topology = topology(7, 2);
    let unlocked = session_lock_object(SessionLockPhase::Unlocked, Some(&topology));
    assert_eq!(
        (
            unlocked.lock_epoch,
            unlocked.phase,
            unlocked.allocations.len()
        ),
        (0, LockPhase::Unlocked, 0)
    );
    for (phase, expected) in [
        (
            SessionLockPhase::Locking {
                epoch: epoch(4),
                attempt: None,
            },
            LockPhase::Locking,
        ),
        (locked(), LockPhase::Locked),
    ] {
        let object = session_lock_object(phase, Some(&topology));
        assert_eq!((object.lock_epoch, object.phase), (4, expected));
        assert_eq!(object.allocations.len(), 2);
    }
    let attempt = SessionUnlockAttempt {
        epoch: epoch(4),
        serial: NonZeroU64::new(1).unwrap(),
    };
    let checking = session_lock_object(
        SessionLockPhase::Locked {
            epoch: epoch(4),
            attempt: Some(attempt),
        },
        Some(&topology),
    );
    assert_eq!(checking.allocations.len(), 2, "an attempt changes nothing");
    let unlocking = session_lock_object(
        SessionLockPhase::Unlocking { epoch: epoch(4) },
        Some(&topology),
    );
    assert_eq!(
        (
            unlocking.lock_epoch,
            unlocking.phase,
            unlocking.allocations.len()
        ),
        (4, LockPhase::Unlocking, 0)
    );
}

#[test]
fn an_allocation_covers_its_output_at_its_largest_head() {
    let object = session_lock_object(locked(), Some(&topology(7, 2)));
    assert_eq!(object.topology_generation, 7);
    let mirrored = &object.allocations[0];
    assert_eq!((mirrored.output_id, mirrored.allocation_id), (1, 1));
    assert_eq!(
        (mirrored.output_generation, mirrored.allocation_generation),
        (2, 2)
    );
    assert_eq!((mirrored.pixel_width, mirrored.pixel_height), (2560, 1440));
    assert_eq!(
        (mirrored.scale_numerator, mirrored.scale_denominator),
        (2, 1)
    );
    let scaled = &object.allocations[1];
    assert_eq!((scaled.pixel_width, scaled.pixel_height), (2880, 1620));
    assert_eq!((scaled.scale_numerator, scaled.scale_denominator), (3, 2));
    // Every object Session builds is one the contract accepts.
    assert_eq!(
        LockObject::decode(&object.encode().unwrap()).unwrap(),
        object
    );
}

#[test]
fn a_topology_change_moves_the_allocation_generation() {
    let before = session_lock_object(locked(), Some(&topology(7, 2)));
    let after = session_lock_object(locked(), Some(&topology(8, 3)));
    assert_eq!(after.topology_generation, 8);
    assert_eq!(after.allocations[0].allocation_generation, 3);
    assert_ne!(
        before.allocations[0].allocation_generation,
        after.allocations[0].allocation_generation
    );
}

#[test]
fn heads_without_a_mode_grant_nothing_and_no_topology_grants_nothing() {
    let mut topology = topology(7, 2);
    topology.heads[1].enabled = false;
    topology.heads[0].current_mode = None;
    topology.heads[2].current_mode = Some(DisplayModeId::from_raw(999));
    let object = session_lock_object(locked(), Some(&topology));
    assert!(object.allocations.is_empty());
    assert!(object.encode().is_ok());
    let blind = session_lock_object(locked(), None);
    assert!(blind.allocations.is_empty());
    assert_eq!((blind.lock_epoch, blind.topology_generation), (4, 1));
    // A zero topology epoch is never published as a generation.
    let zero = session_lock_object(SessionLockPhase::Unlocked, Some(&self::topology(0, 1)));
    assert_eq!(zero.topology_generation, 1);
}

#[test]
fn fractional_scales_take_the_nearest_allowed_ratio() {
    for (pixels, logical, expected) in [
        (1600, 1280, (5, 4)),
        (1920, 1920, (1, 1)),
        (3840, 1280, (3, 1)),
        (1707, 1280, (4, 3)),
    ] {
        let mut topology = topology(7, 2);
        topology.heads = vec![head(1, pixels, 900, true)];
        topology.groups = vec![group(1, 1, logical, 720, &[1])];
        let object = session_lock_object(locked(), Some(&topology));
        let allocation = &object.allocations[0];
        assert_eq!(
            (allocation.scale_numerator, allocation.scale_denominator),
            expected,
            "{pixels}/{logical}"
        );
    }
}

#[test]
fn provider_limits_follow_the_screens_it_covers() {
    let limits = session_lock_file_limits(Some(&topology(7, 2)));
    // The largest allocation is 2880x1620; two live resources per output.
    assert_eq!((limits.max_width_px, limits.max_height_px), (2880, 1620));
    assert_eq!(limits.max_resource_bytes, 2880 * 1620 * 4);
    assert_eq!(limits.max_live_resources, 4);
    assert_eq!(
        LockFileLimits::decode(&limits.encode().unwrap()).unwrap(),
        limits,
        "the contract accepts them"
    );
    // Without a topology, an ordinary screen still fits.
    let blind = session_lock_file_limits(None);
    assert_eq!((blind.max_width_px, blind.max_height_px), (1920, 1080));
    assert_eq!(blind.max_resource_bytes, 1920 * 1080 * 4);
    assert_eq!(blind.max_live_resources, 2);
    assert!(blind.encode().is_ok());
}
