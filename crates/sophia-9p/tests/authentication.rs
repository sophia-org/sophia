use sophia_9p::{ConnectionId, authentication::*};

fn owner() -> Authentication {
    Authentication::new(1, ConnectionId(2), 3, 4, 0).unwrap()
}
fn identity() -> Identity {
    Identity {
        principal: 5,
        namespace: 6,
        launch: 7,
    }
}

#[test]
fn reset_burns_verdicts_but_preserves_transport_identity_and_budget() {
    let mut state = owner();
    let old = state.begin(10, 0).unwrap();
    assert_eq!(state.commit_attach(old, 1), Err(Refusal::Unverified));
    assert_eq!(state.pinned(), None);
    state.verified(old, identity(), 2).unwrap();
    state.commit_attach(old, 3).unwrap();
    state.reset(4).unwrap();
    let fresh = state.begin(10, 5).unwrap();
    assert_eq!(state.verified(old, identity(), 6), Err(Refusal::Stale));
    for changed in [
        Identity {
            principal: 8,
            ..identity()
        },
        Identity {
            namespace: 8,
            ..identity()
        },
        Identity {
            launch: 8,
            ..identity()
        },
    ] {
        assert_eq!(
            state.verified(fresh, changed, 7),
            Err(Refusal::IdentityChanged),
            "reset must preserve the pinned identity"
        );
        assert_eq!(state.pinned(), Some(identity()));
    }
    state.verified(fresh, identity(), 8).unwrap();
    assert_eq!(state.commit_attach(fresh, 9), Ok(identity()));
    assert_eq!(state.starts(), 2);
}

#[test]
fn every_result_correlation_field_is_checked_without_consuming_the_live_attempt() {
    let mut state = owner();
    let a = state.begin(10, 0).unwrap();
    let wrong = [
        Attempt { server: 9, ..a },
        Attempt {
            connection: ConnectionId(9),
            ..a
        },
        Attempt { session: 9, ..a },
        Attempt { audience: 9, ..a },
        Attempt {
            wire_generation: 9,
            ..a
        },
        Attempt {
            attempt_generation: 9,
            ..a
        },
        Attempt { afid: 9, ..a },
    ];
    for a in wrong {
        assert_eq!(
            state.verified(a, identity(), 1),
            Err(Refusal::Stale),
            "foreign verdict must not reach this attempt"
        );
        state.cancel(a);
    }
    state.verified(a, identity(), 2).unwrap();
    assert_eq!(state.verified(a, identity(), 3), Err(Refusal::Stale));
    state.commit_attach(a, 4).unwrap();
}

#[test]
fn reset_and_activity_cannot_renew_deadlines_or_attempt_budget() {
    let mut state = owner();
    for n in 0..MAX_ATTEMPTS {
        state.begin(10, u64::from(n)).unwrap();
        state.reset(u64::from(n)).unwrap();
    }
    assert_eq!(state.begin(10, 9), Err(Refusal::Exhausted));
    let mut state = owner();
    let a = state.begin(10, 0).unwrap();
    state.charge(a, 1, ATTEMPT_MSEC - 1).unwrap();
    assert_eq!(
        state.verified(a, identity(), ATTEMPT_MSEC),
        Err(Refusal::Stale)
    );
    state.reset(9_000).unwrap();
    let a = state.begin(10, 9_001).unwrap();
    assert_eq!(
        state.verified(a, identity(), FIRST_ATTACH_MSEC),
        Err(Refusal::Expired)
    );
    assert_eq!(
        state.begin(10, FIRST_ATTACH_MSEC + 1),
        Err(Refusal::Expired)
    );
}

#[test]
fn cancellation_bounds_and_revocation_do_not_retarget_reused_afids() {
    let mut state = owner();
    let old = state.begin(10, 0).unwrap();
    assert_eq!(state.begin(11, 0), Err(Refusal::Busy));
    state.cancel(old);
    let fresh = state.begin(10, 1).unwrap();
    state.cancel(old);
    assert_eq!(state.charge(old, usize::MAX, 1), Err(Refusal::Stale));
    state.charge(fresh, TRANSCRIPT_BYTES, 2).unwrap();
    assert_eq!(state.charge(fresh, 1, 3), Err(Refusal::Oversize));
    assert_eq!(state.verified(fresh, identity(), 4), Err(Refusal::Stale));
    let fresh = state.begin(10, 5).unwrap();
    state.verified(fresh, identity(), 6).unwrap();
    state.close();
    assert_eq!(state.commit_attach(fresh, 7), Err(Refusal::Closed));
    assert_eq!(state.reset(8), Err(Refusal::Closed));
    assert_eq!(state.pinned(), None);
}

#[test]
fn monotonic_clock_failure_closes_instead_of_extending_authority() {
    let mut state = owner();
    let a = state.begin(10, 100).unwrap();
    assert_eq!(
        state.verified(a, identity(), 99),
        Err(Refusal::ClockReversed)
    );
    assert_eq!(state.begin(10, 101), Err(Refusal::Closed));
}
