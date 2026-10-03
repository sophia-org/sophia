//! t292: the session lock reducer. Only the current attempt of the current
//! lock can end it; locking is reported only on proof of coverage and an
//! applied input epoch; input returns only once the unlock epoch applies.

use sophia_engine::SessionLockEpoch;
use sophia_session::session_lock::{
    SessionLockPhase, SessionLockStart, SessionLockState, SessionUnlockVerdict,
    SessionVerdictOutcome,
};

fn epoch(raw: u64) -> SessionLockEpoch {
    SessionLockEpoch::from_raw(raw).unwrap()
}

/// A session proven locked under its first epoch.
fn locked() -> SessionLockState {
    let mut state = SessionLockState::new();
    assert_eq!(state.lock(), Ok(SessionLockStart::Started(epoch(1))));
    assert!(state.observe_covered(Some(epoch(1)), true));
    state
}

#[test]
fn an_unlocked_session_holds_no_input_and_draws_no_cover() {
    let state = SessionLockState::new();
    assert_eq!(state.phase(), SessionLockPhase::Unlocked);
    assert!(!state.holds_input());
    assert_eq!(state.cover_epoch(), None);
}

#[test]
fn locking_takes_input_and_the_cover_at_once_but_reports_only_on_proof() {
    let mut state = SessionLockState::new();
    assert_eq!(state.lock(), Ok(SessionLockStart::Started(epoch(1))));
    assert!(state.holds_input());
    assert_eq!(state.cover_epoch(), Some(epoch(1)));

    assert!(!state.observe_covered(None, true), "no head proved it");
    assert!(
        !state.observe_covered(Some(epoch(1)), false),
        "the frontend has not applied the epoch"
    );
    assert_eq!(state.phase(), SessionLockPhase::Locking { epoch: epoch(1) });
    assert!(state.observe_covered(Some(epoch(1)), true));
    assert_eq!(
        state.phase(),
        SessionLockPhase::Locked {
            epoch: epoch(1),
            attempt: None
        }
    );
}

#[test]
fn locking_again_keeps_the_lock_in_force() {
    let mut state = locked();
    assert_eq!(state.lock(), Ok(SessionLockStart::AlreadyLocked(epoch(1))));
    assert_eq!(state.cover_epoch(), Some(epoch(1)));
}

#[test]
fn no_attempt_opens_before_the_lock_is_proven() {
    let mut state = SessionLockState::new();
    state.lock().unwrap();
    assert_eq!(state.begin_attempt(), None);
}

#[test]
fn only_one_attempt_is_in_flight() {
    let mut state = locked();
    assert!(state.begin_attempt().is_some());
    assert_eq!(state.begin_attempt(), None);
}

#[test]
fn an_accepted_current_attempt_unlocks_but_input_waits_for_the_epoch() {
    let mut state = locked();
    let attempt = state.begin_attempt().unwrap();
    assert_eq!(
        state.settle(attempt, SessionUnlockVerdict::Accepted),
        SessionVerdictOutcome::Unlocking(epoch(1))
    );
    assert_eq!(state.cover_epoch(), None, "the cover is cleared");
    assert!(state.holds_input(), "input waits for the unlock epoch");
    assert!(!state.observe_unlocked(false));
    assert!(state.observe_unlocked(true));
    assert!(!state.holds_input());
}

#[test]
fn a_rejected_or_undecided_attempt_keeps_the_lock_and_allows_another() {
    for verdict in [
        SessionUnlockVerdict::Rejected,
        SessionUnlockVerdict::Unavailable,
    ] {
        let mut state = locked();
        let attempt = state.begin_attempt().unwrap();
        assert_eq!(
            state.settle(attempt, verdict),
            SessionVerdictOutcome::Failed(attempt, verdict)
        );
        assert_eq!(state.cover_epoch(), Some(epoch(1)));
        let next = state.begin_attempt().unwrap();
        assert!(next.serial > attempt.serial, "serials are never reused");
    }
}

#[test]
fn a_verdict_for_a_settled_attempt_is_stale() {
    let mut state = locked();
    let first = state.begin_attempt().unwrap();
    state.settle(first, SessionUnlockVerdict::Rejected);
    let _second = state.begin_attempt().unwrap();
    assert_eq!(
        state.settle(first, SessionUnlockVerdict::Accepted),
        SessionVerdictOutcome::Stale,
        "a late acceptance of the rejected attempt"
    );
    assert_eq!(state.cover_epoch(), Some(epoch(1)));
}

#[test]
fn a_verdict_from_an_earlier_lock_never_unlocks_a_later_one() {
    let mut state = locked();
    let earlier = state.begin_attempt().unwrap();
    assert_eq!(
        state.settle(earlier, SessionUnlockVerdict::Accepted),
        SessionVerdictOutcome::Unlocking(epoch(1))
    );
    // Relocked before the unlock epoch applied: the unlock is abandoned.
    assert_eq!(state.lock(), Ok(SessionLockStart::Started(epoch(2))));
    assert!(state.observe_covered(Some(epoch(2)), true));
    let _current = state.begin_attempt().unwrap();
    assert_eq!(
        state.settle(earlier, SessionUnlockVerdict::Accepted),
        SessionVerdictOutcome::Stale
    );
    assert!(
        !state.observe_unlocked(true),
        "the abandoned unlock cannot complete"
    );
    assert_eq!(state.cover_epoch(), Some(epoch(2)));
}

#[test]
fn a_cover_proven_for_an_earlier_lock_does_not_prove_a_later_one() {
    let mut state = locked();
    let attempt = state.begin_attempt().unwrap();
    state.settle(attempt, SessionUnlockVerdict::Accepted);
    assert!(state.observe_unlocked(true));
    assert_eq!(state.lock(), Ok(SessionLockStart::Started(epoch(2))));
    assert!(!state.observe_covered(Some(epoch(1)), true));
    assert_eq!(state.phase(), SessionLockPhase::Locking { epoch: epoch(2) });
}

#[test]
fn a_verdict_while_locking_is_stale() {
    let mut state = locked();
    let attempt = state.begin_attempt().unwrap();
    state.settle(attempt, SessionUnlockVerdict::Accepted);
    state.lock().unwrap();
    assert_eq!(
        state.settle(attempt, SessionUnlockVerdict::Accepted),
        SessionVerdictOutcome::Stale
    );
}
