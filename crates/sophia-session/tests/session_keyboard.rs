use sophia_protocol::{DeviceId, SeatId, SurfaceId};
use sophia_session::session_keyboard::{
    PhysicalKeyboardCoverage, RUNTIME_DEADLINE_KEY_RELEASE_TIMEOUT_MSEC, RuntimeDeadlineKeyDrain,
    RuntimeDeadlineKeyDrainDecision, SESSION_CLIENT_PRESSED_KEY_CAPACITY, SessionClientKeyState,
    SessionClientPressedKey, SessionPressedKeyAdmission, VirtualTerminalChordAction,
    VirtualTerminalChordState,
};

fn pressed_key(surface: u32, keycode: u32) -> SessionClientPressedKey {
    SessionClientPressedKey {
        surface: SurfaceId::new(surface, 1),
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(1),
        keycode,
    }
}

#[test]
fn runtime_deadline_releases_held_keys_then_waits_for_delivery_acknowledgement() {
    let mut drain = RuntimeDeadlineKeyDrain::default();
    assert_eq!(
        drain.observe(1_000, 1, 0, 0, 0),
        RuntimeDeadlineKeyDrainDecision::BeginRelease,
    );
    assert!(drain.is_draining());
    assert_eq!(
        drain.observe(1_001, 0, 1, 1, 0),
        RuntimeDeadlineKeyDrainDecision::Waiting,
    );
    assert_eq!(
        drain.observe(1_002, 0, 0, 0, 0),
        RuntimeDeadlineKeyDrainDecision::Complete,
    );
}

#[test]
fn runtime_deadline_key_release_is_immediate_when_idle_and_bounded_when_blocked() {
    let mut idle = RuntimeDeadlineKeyDrain::default();
    assert_eq!(
        idle.observe(2_000, 0, 0, 0, 0),
        RuntimeDeadlineKeyDrainDecision::Complete,
    );
    assert!(!idle.is_draining());

    let mut blocked = RuntimeDeadlineKeyDrain::default();
    assert_eq!(
        blocked.observe(2_000, 1, 0, 0, 0),
        RuntimeDeadlineKeyDrainDecision::BeginRelease,
    );
    assert_eq!(
        blocked.observe(2_499, 0, 1, 1, 0),
        RuntimeDeadlineKeyDrainDecision::Waiting,
    );
    assert_eq!(
        blocked.observe(2_500, 0, 1, 1, 0),
        RuntimeDeadlineKeyDrainDecision::TimedOut,
    );
}

#[test]
fn control_alt_function_keys_activate_each_linux_vt_once_per_press() {
    let mut chord = VirtualTerminalChordState::default();
    assert_eq!(chord.observe(29, true), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(56, true), VirtualTerminalChordAction::Pass);
    for (keycode, terminal) in (59..=68).zip(1..=10).chain([(87, 11), (88, 12)]) {
        assert_eq!(
            chord.observe(keycode, true),
            VirtualTerminalChordAction::Activate(terminal)
        );
        assert_eq!(
            chord.observe(keycode, true),
            VirtualTerminalChordAction::Consume
        );
        assert_eq!(
            chord.observe(keycode, false),
            VirtualTerminalChordAction::Consume
        );
    }
    assert_eq!(chord.observe(56, false), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(59, true), VirtualTerminalChordAction::Pass);
}

#[test]
fn either_control_and_alt_side_is_accepted_but_plain_function_keys_pass() {
    let mut chord = VirtualTerminalChordState::default();
    assert_eq!(chord.observe(59, true), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(59, false), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(97, true), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(100, true), VirtualTerminalChordAction::Pass);
    assert_eq!(
        chord.observe(88, true),
        VirtualTerminalChordAction::Activate(12)
    );
}

#[test]
fn desktop_shortcuts_do_not_activate_a_virtual_terminal_chord() {
    let mut chord = VirtualTerminalChordState::default();
    // Super+Enter and Super+Space are compositor shortcuts, not VT chords.
    assert_eq!(chord.observe(125, true), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(28, true), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(28, false), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(125, false), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(57, true), VirtualTerminalChordAction::Pass);
    assert_eq!(chord.observe(57, false), VirtualTerminalChordAction::Pass);
}

#[test]
fn a_lost_modifier_release_cannot_activate_a_late_function_key() {
    let mut chord = VirtualTerminalChordState::default();
    assert_eq!(
        chord.observe_at(29, true, 1_000),
        VirtualTerminalChordAction::Pass
    );
    assert_eq!(
        chord.observe_at(56, true, 1_001),
        VirtualTerminalChordAction::Pass
    );
    assert_eq!(
        chord.observe_at(59, true, 7_000),
        VirtualTerminalChordAction::Pass
    );
}

#[test]
fn modifiers_from_another_keyboard_cannot_activate_a_virtual_terminal() {
    let mut chord = VirtualTerminalChordState::default();
    assert_eq!(
        chord.observe_at_device(DeviceId::from_raw(1), 29, true, 1_000),
        VirtualTerminalChordAction::Pass
    );
    assert_eq!(
        chord.observe_at_device(DeviceId::from_raw(1), 56, true, 1_001),
        VirtualTerminalChordAction::Pass
    );
    assert_eq!(
        chord.observe_at_device(DeviceId::from_raw(2), 59, true, 1_002),
        VirtualTerminalChordAction::Pass
    );
}

#[test]
fn active_vt_chord_exposes_modifier_keys_that_need_synthetic_release() {
    let mut chord = VirtualTerminalChordState::default();
    let _ = chord.observe(29, true);
    let _ = chord.observe(100, true);
    assert_eq!(
        chord.pressed_modifier_keycodes(),
        [Some(29), None, None, Some(100)]
    );
}

#[test]
fn physical_coverage_reduces_shifted_positions_and_virtual_terminals() {
    let mut coverage = PhysicalKeyboardCoverage::default();
    coverage.observe_key(42, true);
    for keycode in [
        41, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 26, 27, 43, 39, 40, 51, 52, 53,
    ] {
        coverage.observe_key(keycode, true);
        coverage.observe_key(keycode, false);
    }
    coverage.observe_key(42, false);
    for terminal in 1..=12 {
        coverage.observe_virtual_terminal(terminal);
    }

    let snapshot = coverage.snapshot();
    assert_eq!(snapshot.shifted_positions, 21);
    assert_eq!(snapshot.shifted_positions_required, 21);
    assert_eq!(snapshot.virtual_terminals, 12);
    assert_eq!(snapshot.virtual_terminals_required, 12);
}

#[test]
fn a_shift_on_one_keyboard_does_not_cover_a_printable_on_another() {
    let mut coverage = PhysicalKeyboardCoverage::default();
    let a = DeviceId::from_raw(257);
    let b = DeviceId::from_raw(258);
    coverage.observe_key_at_device(a, 42, true);
    coverage.observe_key_at_device(b, 2, true);
    coverage.observe_key_at_device(b, 2, false);
    assert_eq!(coverage.snapshot().shifted_positions, 0);

    coverage.observe_key_at_device(a, 2, true);
    coverage.observe_key_at_device(a, 2, false);
    assert_eq!(coverage.snapshot().shifted_positions, 1);

    // A departure takes the shift with it.
    coverage.forget_device(a);
    coverage.observe_key_at_device(a, 3, true);
    assert_eq!(coverage.snapshot().shifted_positions, 1);
}

#[test]
fn client_key_state_drains_one_surface_without_touching_another() {
    let mut state = SessionClientKeyState::default();
    let old_super = pressed_key(1, 125);
    let old_letter = pressed_key(1, 30);
    let new_letter = pressed_key(2, 31);
    state.record_routed(old_super, true);
    state.record_routed(old_letter, true);
    state.record_routed(new_letter, true);

    let mut releases = Vec::new();
    state.copy_surface_keys(old_super.surface, &mut releases);
    assert_eq!(releases.len(), 2);
    for release in releases {
        state.record_synthetic_release(release);
    }

    assert_eq!(state.pending_len(), 1);
    assert!(state.release_is_routable(new_letter));
    assert_eq!(state.metrics().synthetic_releases, 2);
}

#[test]
fn client_key_state_copies_all_surfaces_for_session_shutdown() {
    let mut state = SessionClientKeyState::default();
    let first = pressed_key(1, 29);
    let second = pressed_key(2, 56);
    state.record_routed(first, true);
    state.record_routed(second, true);

    let mut snapshot = Vec::new();
    state.copy_all_keys(&mut snapshot);

    assert_eq!(snapshot, [first, second]);
    assert_eq!(state.pending_len(), 2);
}

#[test]
fn client_key_state_suppresses_orphan_release_and_is_bounded() {
    let mut state = SessionClientKeyState::default();
    let orphan = pressed_key(1, 30);
    assert!(!state.release_is_routable(orphan));
    state.record_routed(orphan, false);
    assert_eq!(state.metrics().orphan_releases_suppressed, 1);

    for keycode in 1..=SESSION_CLIENT_PRESSED_KEY_CAPACITY {
        assert_eq!(
            state.record_routed(pressed_key(1, keycode as u32), true),
            SessionPressedKeyAdmission::Recorded
        );
    }
    // Saturation is reported rather than raised. No hand holds this many keys,
    // so reaching the bound means releases were already lost, and the caller
    // answers that by closing the endpoint epoch and flushing what is held.
    assert_eq!(
        state.record_routed(pressed_key(2, 999), true),
        SessionPressedKeyAdmission::Saturated
    );
    // A key already recorded is not a new occupant, so it never saturates.
    assert_eq!(
        state.record_routed(pressed_key(1, 1), true),
        SessionPressedKeyAdmission::AlreadyPressed
    );
}

#[test]
fn state_only_release_retires_key_from_a_removed_surface() {
    let mut state = SessionClientKeyState::default();
    let key = pressed_key(4, 28);
    state.record_routed(key, true);
    state.record_state_only_release(key);
    assert_eq!(state.pending_len(), 0);
    assert_eq!(state.metrics().state_only_releases, 1);
    assert_eq!(state.metrics().removed_surface_keys, 0);
}

/// A deadline lands at an arbitrary instant, and the last pointer motion
/// before it can raise a focus request that cannot settle in the same tick.
///
/// The drain waits for it on the same bounded terms as held keys, so the user's
/// final intent gets its chance. Where the two part company is at expiry: a key
/// the session believes a client holds is a fault if nobody released it, while
/// an answer nobody will read once the session stops is not. A thirty-second
/// run that ended in error owing exactly one projection request is what made
/// that difference worth stating.
#[test]
fn runtime_deadline_waits_for_a_policy_request_but_does_not_fail_on_one() {
    let mut drain = RuntimeDeadlineKeyDrain::default();
    // No keys owed, one policy request in flight.
    assert_eq!(
        drain.observe(3_000, 0, 0, 0, 1),
        RuntimeDeadlineKeyDrainDecision::Waiting,
    );
    assert!(drain.is_draining());
    assert_eq!(
        drain.observe(3_001, 0, 0, 0, 1),
        RuntimeDeadlineKeyDrainDecision::Waiting,
    );
    // It settles, and the session may end cleanly.
    assert_eq!(
        drain.observe(3_002, 0, 0, 0, 0),
        RuntimeDeadlineKeyDrainDecision::Complete,
    );

    // One that never settles is abandoned, named, and not a failure.
    let mut unanswered = RuntimeDeadlineKeyDrain::default();
    assert_eq!(
        unanswered.observe(4_000, 0, 0, 0, 1),
        RuntimeDeadlineKeyDrainDecision::Waiting,
    );
    assert_eq!(
        unanswered.observe(
            4_000 + RUNTIME_DEADLINE_KEY_RELEASE_TIMEOUT_MSEC,
            0,
            0,
            0,
            1
        ),
        RuntimeDeadlineKeyDrainDecision::AbandonedPolicyRequests(1),
    );

    // A delivery still owed at the same instant is a fault, request or not.
    let mut stuck = RuntimeDeadlineKeyDrain::default();
    assert_eq!(
        stuck.observe(5_000, 1, 0, 0, 1),
        RuntimeDeadlineKeyDrainDecision::BeginRelease,
    );
    assert_eq!(
        stuck.observe(
            5_000 + RUNTIME_DEADLINE_KEY_RELEASE_TIMEOUT_MSEC,
            0,
            1,
            0,
            1
        ),
        RuntimeDeadlineKeyDrainDecision::TimedOut,
    );
}
