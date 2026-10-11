use sophia_9p::{
    ConnectionId,
    authentication::{Attempt, Identity},
};
use sophia_factotum::launch::{Delegation, LaunchAuthority, Refusal, Verdict};

fn attempt(connection: u64) -> Attempt {
    Attempt {
        server: 1,
        connection: ConnectionId(connection),
        session: 2,
        audience: 3,
        wire_generation: 1,
        attempt_generation: 1,
        afid: 4,
    }
}
fn delegation(launch: u64, principal: u64, audience: u64) -> Delegation {
    Delegation {
        identity: Identity {
            principal,
            namespace: 6,
            launch,
        },
        audience,
    }
}
fn active(owner: &mut LaunchAuthority, launch: u64, principal: u64) -> Delegation {
    let d = delegation(launch, principal, 3);
    owner.reserve(d, 0).unwrap();
    owner.activate(launch, 0).unwrap();
    d
}

#[test]
fn reserved_launch_cannot_verify_and_rollback_burns_its_generation() {
    let mut owner = LaunchAuthority::new(2, 0).unwrap();
    let d = delegation(1, 5, 3);
    owner.reserve(d, 0).unwrap();
    let a = attempt(1);
    let nonce = owner.begin(a, 5000, 0).unwrap();
    assert_eq!(
        owner.relay(1, nonce, 1),
        Err(Refusal::Reserved),
        "a Reserved launch must not receive a verified result"
    );
    assert_eq!(owner.take_verdict(a, 1), Ok(None));
    assert_eq!(owner.activate(2, 1), Err(Refusal::Stale));
    owner.retire(1, 2).unwrap();
    assert_eq!(owner.reserve(d, 2), Err(Refusal::Stale));
    assert_eq!(owner.activate(1, 2), Err(Refusal::Stale));
    assert_eq!(owner.relay(1, nonce, 2), Err(Refusal::Stale));
    owner.cancel(a, 2).unwrap();
}

#[test]
fn audience_refusal_preserves_challenge_and_success_consumes_it_once() {
    let mut owner = LaunchAuthority::new(2, 0).unwrap();
    owner.reserve(delegation(1, 7, 9), 0).unwrap();
    owner.activate(1, 0).unwrap();
    let d = active(&mut owner, 2, 5);
    let a = attempt(1);
    let nonce = owner.begin(a, 5000, 0).unwrap();
    assert_eq!(
        owner.relay(1, nonce, 0),
        Err(Refusal::Audience),
        "a channel for another audience cannot verify this attempt"
    );
    assert_eq!(owner.take_verdict(a, 0), Ok(None));
    owner.relay(2, nonce, 0).unwrap();
    assert_eq!(owner.relay(2, nonce, 0), Err(Refusal::Stale));
    assert_eq!(
        owner.take_verdict(a, 0),
        Ok(Some(Verdict {
            attempt: a,
            identity: d.identity
        }))
    );
    assert_eq!(owner.take_verdict(a, 0), Err(Refusal::Stale));
    assert_eq!(owner.relay(2, nonce, 0), Err(Refusal::Stale));
}

#[test]
fn every_attempt_field_correlates_result_and_cancel() {
    let mut owner = LaunchAuthority::new(2, 0).unwrap();
    let d = active(&mut owner, 1, 5);
    let a = attempt(1);
    let nonce = owner.begin(a, 5000, 0).unwrap();
    owner.relay(1, nonce, 0).unwrap();
    for field in 0..7 {
        let mut foreign = a;
        match field {
            0 => foreign.server += 1,
            1 => foreign.connection.0 += 1,
            2 => foreign.session += 1,
            3 => foreign.audience += 1,
            4 => foreign.wire_generation += 1,
            5 => foreign.attempt_generation += 1,
            6 => foreign.afid += 1,
            _ => unreachable!(),
        }
        owner.cancel(foreign, 0).unwrap();
        assert_eq!(
            owner.take_verdict(foreign, 0),
            Err(Refusal::Stale),
            "field {field}"
        );
    }
    assert_eq!(
        owner.take_verdict(a, 0),
        Ok(Some(Verdict {
            attempt: a,
            identity: d.identity
        }))
    );
}

#[test]
fn cancellation_reset_reuse_and_retirement_cannot_deliver_old_results() {
    let mut owner = LaunchAuthority::new(2, 0).unwrap();
    active(&mut owner, 1, 5);
    let a = attempt(1);
    let old = owner.begin(a, 5000, 0).unwrap();
    owner.cancel(a, 0).unwrap();
    let next = Attempt {
        wire_generation: 2,
        attempt_generation: 2,
        ..a
    };
    let fresh = owner.begin(next, 5000, 0).unwrap();
    assert_eq!(owner.relay(1, old, 0), Err(Refusal::Stale));
    owner.relay(1, fresh, 0).unwrap();
    owner.retire(1, 0).unwrap();
    assert_eq!(
        owner.take_verdict(next, 0),
        Err(Refusal::Stale),
        "retirement must invalidate an undelivered verdict"
    );
    assert_eq!(
        owner.begin(Attempt { session: 3, ..a }, 5000, 0).err(),
        Some(Refusal::Invalid)
    );
}

#[test]
fn deadlines_cover_undelivered_results_and_backwards_clock_closes_owner() {
    let mut owner = LaunchAuthority::new(2, 0).unwrap();
    active(&mut owner, 1, 5);
    let a = attempt(1);
    assert_eq!(owner.begin(a, 5001, 0).err(), Some(Refusal::Invalid));
    let nonce = owner.begin(a, 10, 0).unwrap();
    owner.relay(1, nonce, 9).unwrap();
    assert_eq!(owner.take_verdict(a, 10), Err(Refusal::Stale));
    let nonce = owner.begin(a, 20, 10).unwrap();
    assert_eq!(owner.relay(1, nonce, 20), Err(Refusal::Stale));
    assert_eq!(owner.expire(19), Err(Refusal::ClockReversed));
    assert_eq!(owner.begin(a, 30, 20).err(), Some(Refusal::Closed));
}

#[test]
fn named_capacities_refuse_without_evicting_other_work() {
    let mut owner = LaunchAuthority::new(2, 0).unwrap();
    for launch in 1..=64 {
        active(&mut owner, launch, launch);
    }
    assert_eq!(
        owner.reserve(delegation(65, 65, 3), 0),
        Err(Refusal::Capacity)
    );
    owner.retire(64, 0).unwrap();
    active(&mut owner, 65, 65);
    for connection in 1..=32 {
        owner.begin(attempt(connection), 5000, 0).unwrap();
    }
    assert_eq!(
        owner.begin(attempt(33), 5000, 0).err(),
        Some(Refusal::Capacity)
    );
    let a = Attempt {
        attempt_generation: 2,
        afid: 5,
        ..attempt(1)
    };
    assert_eq!(owner.begin(a, 5000, 0).err(), Some(Refusal::Busy));
    owner.cancel(attempt(1), 0).unwrap();
    let nonce = owner.begin(a, 5000, 0).unwrap();
    owner.relay(1, nonce, 0).unwrap();
    // Completed work still owns its global slot until consumed/cancelled.
    assert_eq!(
        owner.begin(attempt(33), 5000, 0).err(),
        Some(Refusal::Capacity)
    );
    assert!(owner.take_verdict(a, 0).unwrap().is_some());
    owner.begin(attempt(33), 5000, 0).unwrap();
    assert_eq!(owner.take_verdict(attempt(2), 0), Ok(None));
}

#[test]
fn principal_and_launch_quotas_use_registered_identity_not_unverified_claims() {
    let mut owner = LaunchAuthority::new(2, 0).unwrap();
    for launch in 1..=5 {
        active(&mut owner, launch, 9);
    }
    active(&mut owner, 6, 10);
    let mut nonces = Vec::new();
    for connection in 1..=10 {
        nonces.push(owner.begin(attempt(connection), 5000, 0).unwrap());
    }
    for (i, nonce) in nonces.iter().take(8).enumerate() {
        owner.relay(i as u64 / 2 + 1, *nonce, 0).unwrap();
    }
    assert_eq!(owner.relay(1, nonces[8], 0), Err(Refusal::Capacity));
    assert_eq!(owner.relay(5, nonces[8], 0), Err(Refusal::Capacity));
    // Another registered principal can proceed despite that principal's limit.
    owner.relay(6, nonces[9], 0).unwrap();
    owner.take_verdict(attempt(1), 0).unwrap().unwrap();
    owner.relay(1, nonces[8], 0).unwrap();
    assert_eq!(
        owner
            .take_verdict(attempt(9), 0)
            .unwrap()
            .unwrap()
            .identity
            .principal,
        9
    );
}
