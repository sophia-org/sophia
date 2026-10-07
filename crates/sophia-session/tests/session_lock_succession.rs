//! t294: replacing the lock provider's process to follow the render device,
//! inside the one service the session keeps for it. Every decision the
//! provider carries out is the succession's; what a replaced provider was
//! granted ends through the helper the owner loop calls.
use std::sync::Arc;

use sophia_engine::{SessionLockEpoch, SessionLockImageIdentity, SessionLockKeyboard};
use sophia_protocol::DeviceId;
use sophia_protocol::OutputId;
use sophia_protocol::lock_files::*;
use sophia_session::emergency_input::EmergencyChordState;
use sophia_session::session_keyboard::VirtualTerminalChordState;
use sophia_session::session_lock_frames::SessionLockFrames;
use sophia_session::session_lock_input::{SessionLockInput, session_lock_chords};
use sophia_session::session_lock_succession::{
    LockProviderStart::{AwaitDevice, Start, Wait},
    LockProviderSuccession, revoke_replaced_lock_provider,
};

/// A provider whose first launch was prepared and is running and authorized.
fn running() -> LockProviderSuccession {
    let mut succession = LockProviderSuccession::default();
    assert_eq!(succession.start(true), Start { regrant: true });
    succession.regranted();
    succession.child_started();
    assert!(succession.authorization_due());
    succession.authorization_queued();
    assert!(succession.admits_provider_events());
    succession
}

/// Queues the owed retirement as the provider does each pass.
fn queue(succession: &mut LockProviderSuccession) {
    assert!(succession.retire_owed());
    succession.retire_queued();
    assert!(!succession.retire_owed());
}

#[test]
fn the_successor_waits_for_both_the_marker_and_the_reap_in_either_order() {
    for marker_first in [true, false] {
        let mut succession = running();
        assert!(
            succession.device_changed(),
            "the old process is asked to exit"
        );
        assert!(
            !succession.admits_provider_events(),
            "ignored from the change on"
        );
        assert_eq!(succession.start(true), Wait);
        queue(&mut succession);
        if marker_first {
            succession.retired();
            assert_eq!(succession.start(true), Wait, "not reaped");
            assert!(succession.child_reaped(), "it was asked to exit");
        } else {
            assert!(succession.child_reaped());
            assert_eq!(succession.start(true), Wait, "not marked");
            succession.retired();
        }
        assert!(succession.admits_provider_events());
        assert_eq!(succession.start(true), Start { regrant: true });
    }
}

#[test]
fn the_old_process_events_are_dropped_from_the_change_until_the_marker() {
    use sophia_runtime::lock_files::{LockFileServiceEvent as Event, LockInbound};
    let connected = |epoch| Event::Connected {
        connection_epoch: epoch,
        chords: Vec::new(),
    };
    let inbound = |epoch| Event::Inbound {
        connection_epoch: epoch,
        inbound: LockInbound::ResourceRetired(LockResourceId {
            id: 1,
            generation: 1,
        }),
    };
    let mut succession = running();
    assert!(succession.hands_on(&connected(OLD)));
    succession.device_changed();
    // Before the retirement is even queued, and after, until its marker.
    assert!(!succession.hands_on(&connected(OLD)), "a late connection");
    assert!(!succession.hands_on(&inbound(OLD)), "a late submission");
    queue(&mut succession);
    assert!(!succession.hands_on(&inbound(OLD)));
    assert!(succession.hands_on(&Event::Disconnected {
        connection_epoch: OLD
    }));
    assert!(
        !succession.hands_on(&Event::Retired { next_epoch: NEXT }),
        "consumed"
    );
    succession.child_reaped();
    assert_eq!(succession.start(true), Start { regrant: true });
    succession.regranted();
    succession.child_started();
    succession.authorization_queued();
    assert!(succession.hands_on(&connected(NEXT)), "the successor's");
    assert!(succession.hands_on(&inbound(NEXT)));
}

#[test]
fn rapid_changes_coalesce_into_one_retirement_and_the_latest_device() {
    // A to B to C: B is never launched, and one marker is owed.
    let mut succession = running();
    assert!(succession.device_changed());
    queue(&mut succession);
    assert!(!succession.device_changed(), "already exiting");
    assert!(!succession.retire_owed(), "one retirement outstanding");
    assert_eq!(succession.start(true), Wait);
    succession.retired();
    assert_eq!(succession.start(true), Wait, "still not reaped");
    succession.child_reaped();
    assert_eq!(succession.start(true), Start { regrant: true });

    // A to none to A: A's old launch is never reused.
    let mut succession = running();
    succession.device_changed();
    queue(&mut succession);
    succession.device_changed();
    succession.child_reaped();
    succession.retired();
    assert_eq!(succession.start(true), Start { regrant: true });

    // A change after the marker but before the reap owes one more retirement
    // and still waits for the reap.
    let mut succession = running();
    succession.device_changed();
    queue(&mut succession);
    succession.retired();
    succession.device_changed();
    assert!(succession.retire_owed());
    queue(&mut succession);
    succession.retired();
    assert_eq!(succession.start(true), Wait);
    succession.child_reaped();
    assert_eq!(succession.start(true), Start { regrant: true });
}

#[test]
fn full_queues_keep_the_retirement_and_the_authorization_owed() {
    let mut succession = running();
    succession.device_changed();
    // A full queue leaves the retirement owed pass after pass.
    for _ in 0..3 {
        assert!(succession.retire_owed());
        assert!(!succession.admits_provider_events());
    }
    queue(&mut succession);
    succession.retired();
    succession.child_reaped();
    assert_eq!(succession.start(true), Start { regrant: true });
    succession.regranted();
    succession.child_started();
    for _ in 0..3 {
        assert!(
            succession.authorization_due(),
            "kept while the queue is full"
        );
    }
    succession.authorization_queued();
    assert!(!succession.authorization_due());

    // An authorization not yet queued is obsolete once the device changes,
    // and none is queued ahead of the retirement it would follow.
    let mut succession = running();
    succession.device_changed();
    queue(&mut succession);
    succession.retired();
    succession.child_reaped();
    succession.start(true);
    succession.regranted();
    succession.child_started();
    assert!(succession.authorization_due());
    assert!(succession.device_changed());
    assert!(
        !succession.authorization_due(),
        "not ahead of the retirement"
    );
    queue(&mut succession);
    assert!(
        !succession.authorization_due(),
        "the obsolete grant is cancelled"
    );
}

#[test]
fn a_direct_grant_without_a_device_waits_for_one_from_the_start() {
    let mut succession = LockProviderSuccession::default();
    assert_eq!(succession.start(false), AwaitDevice);
    assert!(succession.awaiting_device());
    assert_eq!(succession.start(false), AwaitDevice, "nothing launches");
    // The device appears.
    assert!(!succession.device_changed(), "no process to ask");
    queue(&mut succession);
    succession.retired();
    assert_eq!(succession.start(true), Start { regrant: true });
    assert!(!succession.awaiting_device());

    // A device lost while running: the provider waits again.
    let mut succession = running();
    succession.device_changed();
    queue(&mut succession);
    succession.retired();
    succession.child_reaped();
    assert_eq!(succession.start(false), AwaitDevice);
}

#[test]
fn a_failed_preparation_keeps_the_grant_owed_and_a_crash_keeps_the_launch() {
    let mut succession = LockProviderSuccession::default();
    assert_eq!(succession.start(true), Start { regrant: true });
    // Preparation failed: the next start prepares again.
    assert!(succession.regrant_pending());
    assert_eq!(succession.start(true), Start { regrant: true });
    succession.regranted();
    succession.child_started();
    // An ordinary crash: the same grant, after the ordinary backoff.
    assert!(!succession.child_reaped(), "it was not asked to exit");
    assert_eq!(succession.start(true), Start { regrant: false });
}

#[test]
fn a_stopped_service_is_terminal() {
    let mut succession = running();
    assert_eq!(
        succession.fail(),
        Some(true),
        "the process is asked to exit"
    );
    assert_eq!(succession.fail(), None, "reported once");
    assert!(!succession.admits_provider_events());
    assert!(!succession.device_changed());
    assert!(!succession.retire_owed());
    assert!(!succession.authorization_due());
    succession.child_reaped();
    assert_eq!(succession.start(true), Wait, "nothing starts again");
}

const ALT: u16 = 0b0100;
const LEFT_ALT: u32 = 56;
const B: u32 = 48;
const OLD: u64 = 11;
/// The next epoch of the same transport, as the service hands it out
/// (`a_retired_provider_is_followed_by_a_successor_under_the_next_epoch`).
const NEXT: u64 = 12;

fn lock_input() -> SessionLockInput {
    let keyboard = SessionLockKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    SessionLockInput::new(keyboard).unwrap()
}

/// Alt+b on the lock keyboard; returns the chords it fired.
fn alt_b(input: &mut SessionLockInput) -> Vec<u16> {
    let (mut emergency, mut terminal) = (
        EmergencyChordState::default(),
        VirtualTerminalChordState::default(),
    );
    let device = DeviceId::from_raw(3);
    for (key, pressed) in [(LEFT_ALT, true), (B, true), (B, false), (LEFT_ALT, false)] {
        input.observe_key(device, key, pressed, 0, &mut emergency, &mut terminal);
    }
    input.take_chords()
}

fn candidate(generation: u64) -> LockCandidate {
    LockCandidate {
        transaction: 100 + generation,
        lock_epoch: 3,
        output_id: 1,
        output_generation: 1,
        allocation_id: 1,
        allocation_generation: 1,
        candidate_generation: generation,
        pacing_permit: generation,
        resource: LockResourceId {
            id: 1,
            generation: 1,
        },
    }
}

/// A provider on `connection` draws resource 1, generation 1, on output 1:
/// the same numbers whichever process sends them.
fn draw(frames: &mut SessionLockFrames, connection: u64) -> SessionLockImageIdentity {
    frames.connected(connection);
    let resource = LockResourceId {
        id: 1,
        generation: 1,
    };
    frames.resource_ready(connection, resource, 2, 1, Arc::from(vec![1; 8]));
    assert!(frames.candidate(connection, candidate(1)).0);
    frames.images()[&OutputId::from_raw(1)].image.identity
}

#[test]
fn a_replaced_providers_grants_end_at_once_and_its_successor_is_granted_anew() {
    let granted = [LockChordRequest {
        keysym: 0x62,
        modifiers: ALT,
    }];
    let mut input = lock_input();
    let mut chords = session_lock_chords(&granted);
    input.set_chords(chords.clone());
    let mut frames = SessionLockFrames::default();
    frames.lock(SessionLockEpoch::from_raw(3));
    draw(&mut frames, OLD);
    assert_eq!(alt_b(&mut input), [0]);

    // The old process is still alive; its grants end now.
    assert!(revoke_replaced_lock_provider(
        &mut chords,
        Some(&mut input),
        &mut frames
    ));
    assert!(chords.is_empty());
    assert!(alt_b(&mut input).is_empty(), "its chord no longer fires");
    assert!(frames.images().is_empty(), "every head shows the fill");
    assert!(
        !frames.candidate(OLD, candidate(2)).0,
        "nothing more of its own"
    );

    // With no lock keyboard the chords still end.
    let mut chords = session_lock_chords(&granted);
    assert!(!revoke_replaced_lock_provider(
        &mut chords,
        None,
        &mut frames
    ));
    assert!(chords.is_empty());

    // The successor's grant is its own.
    let chords = session_lock_chords(&granted);
    input.set_chords(chords);
    assert_eq!(alt_b(&mut input), [0]);
}

#[test]
fn feedback_for_the_old_image_cannot_settle_a_successor_reusing_its_numbers() {
    let mut frames = SessionLockFrames::default();
    frames.lock(SessionLockEpoch::from_raw(3));
    let old = draw(&mut frames, OLD);
    // A head still shows the old image when the provider is replaced.
    revoke_replaced_lock_provider(&mut Vec::new(), None, &mut frames);
    let new = draw(&mut frames, NEXT);
    assert_ne!(old, new, "the epoch tells the two apart");
    assert_eq!(
        (old.resource_id, old.resource_generation),
        (new.resource_id, new.resource_generation)
    );
    assert_eq!(
        frames.presented(OutputId::from_raw(1), Some((old, 1))),
        None,
        "the old image does not settle the new candidate"
    );
    let settled = frames
        .presented(OutputId::from_raw(1), Some((new, 1)))
        .unwrap();
    assert_eq!(settled.status, LockCandidateStatus::Presented);
}
