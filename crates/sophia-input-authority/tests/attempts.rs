//! Attempt completion cannot outrun a writer that might still send a release.
use sophia_input_authority::*;
use sophia_protocol::{DeviceId, SeatId};

struct Fixture {
    a: AuthorityInstance,
    issuer: IssuerHandle,
    submit: SubmitHandle,
    cap: DeviceCapability,
}
fn fixture() -> Fixture {
    let (mut a, issuer, submit) = AuthorityInstance::new(
        SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1)),
        Capacity::PLANNED,
        9,
    )
    .unwrap();
    let (grant, generation) = a.issue_grant(&issuer, fixture_connection()).unwrap();
    let cap = a
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(1))
        .unwrap();
    Fixture {
        a,
        issuer,
        submit,
        cap,
    }
}
fn context(f: &Fixture) -> ExecutionContext {
    ExecutionContext {
        connection: fixture_connection(),
        generation: f.cap.generation(),
        epoch: 0,
        publication: 0,
        request: 1,
    }
}
fn to(value: u64) -> Recipient {
    Recipient {
        recipient: value,
        connection_generation: 1,
    }
}
fn both() -> SettlementBit {
    SettlementBit {
        native_reconciled: true,
        recipient_settled: true,
    }
}
fn owe(f: &mut Fixture, key: Input, recipient: u64) -> HoldIncarnation {
    f.a.execute_press(&f.submit, f.cap, key, context(f), to(recipient))
        .unwrap();
    let ReleaseOutcome::DeliverTo(hold) = f.a.release(&f.submit, f.cap, key, context(f)).unwrap()
    else {
        panic!("last source owes release");
    };
    f.a.settle(
        &f.issuer,
        Some(f.cap.grant()),
        key,
        hold,
        SettlementBit {
            native_reconciled: true,
            recipient_settled: false,
        },
    )
    .unwrap();
    hold
}

#[test]
fn a_separate_settlement_does_not_let_a_pending_writer_overtake_a_new_press() {
    let mut f = fixture();
    let key = Input::key(30).unwrap();
    let old = owe(&mut f, key, 1);
    let mut cursor = 0;
    let claim =
        f.a.claim_next_attempt(&f.issuer, &mut cursor)
            .unwrap()
            .unwrap();
    assert_eq!(claim.hold, old);
    assert!(
        !f.a.settle(&f.issuer, Some(f.cap.grant()), key, old, both())
            .unwrap(),
        "even both bits must retain the pending writer barrier"
    );
    assert_eq!(
        f.a.execute_press(&f.submit, f.cap, key, context(&f), to(1))
            .unwrap_err(),
        RegistrationError::ReleaseBarrier
    );
    assert!(f.a.finish_attempt(&f.issuer, claim.token, both()).unwrap());
    assert!(
        f.a.execute_press(&f.submit, f.cap, key, context(&f), to(1))
            .unwrap()
            .first_press()
    );
    assert!(
        !f.a.finish_attempt(&f.issuer, claim.token, both()).unwrap(),
        "duplicate outcome settles nothing"
    );
}

#[test]
fn sixty_four_attempts_do_not_drop_the_sixty_fifth_debt_or_retry_inflight_work() {
    let mut f = fixture();
    for code in 8..73 {
        owe(&mut f, Input::key(code).unwrap(), 1);
    }
    let mut cursor = 0;
    let claims: Vec<_> = (0..64)
        .map(|_| {
            f.a.claim_next_attempt(&f.issuer, &mut cursor)
                .unwrap()
                .unwrap()
        })
        .collect();
    let unique: std::collections::BTreeSet<_> = claims.iter().map(|c| c.hold.hold()).collect();
    assert_eq!(unique.len(), 64, "no second attempt while first is pending");
    assert_eq!(
        f.a.claim_next_attempt(&f.issuer, &mut cursor).unwrap_err(),
        RegistrationError::Capacity(CapacityError::NoAttemptSlot)
    );
    assert!(
        !f.a.finish_attempt(&f.issuer, claims[0].token, SettlementBit::default())
            .unwrap(),
        "failed delivery retains its debt"
    );
    let next =
        f.a.claim_next_attempt(&f.issuer, &mut cursor)
            .unwrap()
            .unwrap();
    assert!(
        !unique.contains(&next.hold.hold()),
        "fair cursor first reaches the waiting65th"
    );
    assert!(f.a.finish_attempt(&f.issuer, next.token, both()).unwrap());
    let retry =
        f.a.claim_next_attempt(&f.issuer, &mut cursor)
            .unwrap()
            .unwrap();
    assert_eq!(
        retry.hold, claims[0].hold,
        "the failed first debt remains retryable"
    );
    assert_ne!(retry.token, claims[0].token);
    assert!(
        !f.a.finish_attempt(&f.issuer, claims[0].token, both())
            .unwrap(),
        "late old attempt cannot settle the retry that reused its slot"
    );
    assert!(f.a.finish_attempt(&f.issuer, retry.token, both()).unwrap());
    for claim in claims.iter().skip(1) {
        assert!(f.a.finish_attempt(&f.issuer, claim.token, both()).unwrap());
    }
    assert!(
        f.a.claim_next_attempt(&f.issuer, &mut cursor)
            .unwrap()
            .is_none()
    );
}

#[test]
fn attempt_receipts_are_authority_bound_even_with_identical_public_seats() {
    let mut first = fixture();
    let mut other = fixture();
    let key = Input::key(30).unwrap();
    owe(&mut first, key, 1);
    owe(&mut other, key, 1);
    let claim = first
        .a
        .claim_next_attempt(&first.issuer, &mut 0)
        .unwrap()
        .unwrap();
    let own = other
        .a
        .claim_next_attempt(&other.issuer, &mut 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        other
            .a
            .finish_attempt(&other.issuer, claim.token, both())
            .unwrap_err(),
        RegistrationError::ForeignAuthority
    );
    assert!(
        other
            .a
            .finish_attempt(&other.issuer, own.token, both())
            .unwrap()
    );
}

#[test]
fn native_reconciliation_must_finish_before_a_transport_attempt_is_eligible() {
    let mut f = fixture();
    let key = Input::key(30).unwrap();
    let press =
        f.a.execute_press(&f.submit, f.cap, key, context(&f), to(1))
            .unwrap();
    f.a.release(&f.submit, f.cap, key, context(&f)).unwrap();
    assert!(f.a.claim_next_attempt(&f.issuer, &mut 0).unwrap().is_none());
    f.a.settle(
        &f.issuer,
        Some(f.cap.grant()),
        key,
        press.incarnation(),
        SettlementBit {
            native_reconciled: true,
            recipient_settled: false,
        },
    )
    .unwrap();
    assert!(f.a.claim_next_attempt(&f.issuer, &mut 0).unwrap().is_some());
}

fn fixture_connection() -> sophia_input_authority::ConnectionIdentity {
    sophia_input_authority::ConnectionIdentity {
        recipient: 1,
        connection_generation: 1,
    }
}
