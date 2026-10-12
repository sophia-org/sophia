use sophia_9p::{
    ConnectionId,
    authentication::{Attempt, Identity},
};
use sophia_factotum::launch::{
    Audiences, Delegation, LaunchAuthority, MAX_AUDIENCES, Refusal,
    control::LaunchControl,
    wire::{Command, MAX_RECORD_BYTES, Response},
};

fn delegation() -> Delegation {
    Delegation {
        identity: Identity {
            principal: 1,
            namespace: 2,
            launch: 3,
        },
        audience: 4,
    }
}
fn attempt(audience: u64) -> Attempt {
    Attempt {
        server: 1,
        connection: ConnectionId(audience),
        session: 1,
        audience,
        wire_generation: 1,
        attempt_generation: 1,
        afid: 1,
    }
}

#[test]
fn approved_endpoints_share_identity_but_foreign_audience_refuses() {
    let mut owner = LaunchAuthority::new(1, 0).unwrap();
    owner
        .reserve_with_audiences(delegation(), Audiences::new(&[4, 7]).unwrap(), 0)
        .unwrap();
    owner.activate(3, 0).unwrap();
    for audience in [4, 7] {
        let a = attempt(audience);
        let nonce = owner.begin(a, 5000, 0).unwrap();
        owner.relay(3, nonce, 0).unwrap();
        assert_eq!(
            owner.take_verdict(a, 0).unwrap().unwrap().identity,
            delegation().identity
        );
    }
    let a = attempt(8);
    let nonce = owner.begin(a, 5000, 0).unwrap();
    assert_eq!(
        owner.relay(3, nonce, 0),
        Err(Refusal::Audience),
        "recipe authentication must refuse an unapproved endpoint"
    );
    assert!(owner.take_verdict(a, 0).unwrap().is_none());
    owner.retire(3, 0).unwrap();
    assert_eq!(owner.relay(3, nonce, 0), Err(Refusal::Stale));
}

#[test]
fn old_single_role_registration_remains_narrow_and_sets_cannot_be_extended() {
    let mut owner = LaunchAuthority::new(1, 0).unwrap();
    owner.reserve(delegation(), 0).unwrap();
    owner.activate(3, 0).unwrap();
    let nonce = owner.begin(attempt(7), 5000, 0).unwrap();
    assert_eq!(owner.relay(3, nonce, 0), Err(Refusal::Audience));
    assert_eq!(
        owner.reserve_with_audiences(delegation(), Audiences::new(&[4, 7]).unwrap(), 0),
        Err(Refusal::Stale)
    );
    assert_eq!(owner.relay(3, nonce, 0), Err(Refusal::Audience));
}

#[test]
fn audience_set_bounds_and_private_wire_refuse_ambiguity_and_truncation() {
    for invalid in [vec![], vec![0], vec![4, 4], (1..=65).collect()] {
        assert!(Audiences::new(&invalid).is_err());
    }
    let values: Vec<_> = (1..=MAX_AUDIENCES as u64).collect();
    let command = Command::ReserveAudiences {
        delegation: delegation(),
        audiences: Audiences::new(&values).unwrap(),
    };
    let bytes = command.encode();
    assert_eq!(bytes.len(), MAX_RECORD_BYTES);
    assert_eq!(Command::decode(&bytes).unwrap().encode(), bytes);
    for end in 0..bytes.len() {
        assert!(Command::decode(&bytes[..end]).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(Command::decode(&extra).is_err());
    for count in [0u32, 65] {
        let mut bad = bytes.clone();
        bad[40..44].copy_from_slice(&count.to_le_bytes());
        assert!(Command::decode(&bad).is_err());
    }
    let mut unordered = bytes.clone();
    unordered[44..52].copy_from_slice(&2u64.to_le_bytes());
    unordered[52..60].copy_from_slice(&1u64.to_le_bytes());
    assert!(Command::decode(&unordered).is_err());
    let mut duplicate = bytes.clone();
    duplicate[44..52].copy_from_slice(&2u64.to_le_bytes());
    assert!(Command::decode(&duplicate).is_err());
    let mut foreign_primary = bytes;
    foreign_primary[32..40].copy_from_slice(&99u64.to_le_bytes());
    assert!(Command::decode(&foreign_primary).is_err());
}

#[test]
fn private_control_runs_multi_endpoint_conversations_and_retires_all() {
    let mut control = LaunchControl::default();
    let mut exchange = |command: Command| {
        control.write(&command.encode()).unwrap();
        Response::decode(&control.read(MAX_RECORD_BYTES).unwrap()).unwrap()
    };
    assert!(matches!(
        exchange(Command::Initialize { session: 1 }),
        Response::Done
    ));
    assert!(matches!(
        exchange(Command::ReserveAudiences {
            delegation: delegation(),
            audiences: Audiences::new(&[4, 7]).unwrap()
        }),
        Response::Done
    ));
    assert!(matches!(
        exchange(Command::Activate { launch: 3 }),
        Response::Done
    ));
    for audience in [4, 7] {
        let a = attempt(audience);
        let Response::Challenge(nonce) = exchange(Command::Begin {
            attempt: a,
            remaining_msec: 5000,
        }) else {
            panic!("challenge required")
        };
        assert!(matches!(
            exchange(Command::Relay { launch: 3, nonce }),
            Response::Done
        ));
        let Response::Verified(v) = exchange(Command::Take { attempt: a }) else {
            panic!("verdict required")
        };
        assert_eq!(v.identity, delegation().identity);
        assert_eq!(v.attempt, a);
    }
    assert!(matches!(
        exchange(Command::Retire { launch: 3 }),
        Response::Done
    ));
    assert!(
        control
            .write(&Command::Activate { launch: 3 }.encode())
            .is_err()
    );
}
