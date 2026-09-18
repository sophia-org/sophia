//! Independent interaction checks: completion cells and writer attempts are
//! separate reasons to retain a revoked grant. Neither receipt settles both.
use sophia_input_authority::*;
use sophia_protocol::{DeviceId, SeatId};

fn both() -> SettlementBit {
    SettlementBit {
        native_reconciled: true,
        recipient_settled: true,
    }
}

#[test]
fn epoch_revocation_keeps_completion_and_attempt_pins_in_either_consumption_order() {
    for completion_first in [false, true] {
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
        let key = Input::key(30).unwrap();
        let context = ExecutionContext {
            connection: fixture_connection(),
            generation,
            epoch: 0,
            publication: 0,
            request: 1,
        };
        let token = a.reserve_request(&submit, cap, context).unwrap();
        assert_eq!(
            a.execute_reserved(&issuer, token, fixture_connection(), |permit| {
                permit.press(
                    key,
                    Recipient {
                        recipient: 42,
                        connection_generation: 1,
                    },
                )?;
                Ok(())
            })
            .unwrap(),
            RequestCompletion::Processed
        );
        let ReleaseOutcome::DeliverTo(hold) = a.release(&submit, cap, key, context).unwrap() else {
            panic!("last holder must owe a release")
        };
        assert!(
            a.claim_next_attempt(&issuer, &mut 0).unwrap().is_none(),
            "transport cannot start while native reconciliation is outstanding"
        );
        assert!(
            !a.settle(
                &issuer,
                Some(grant),
                key,
                hold,
                SettlementBit {
                    native_reconciled: true,
                    recipient_settled: false
                }
            )
            .unwrap()
        );
        let claim = a.claim_next_attempt(&issuer, &mut 0).unwrap().unwrap();
        assert_eq!(claim.hold, hold);
        a.begin_transition(&issuer, 1, 1).unwrap();
        a.publish(&issuer, 1, 1).unwrap();
        assert!(!a.settle(&issuer, Some(grant), key, hold, both()).unwrap());
        assert_eq!(
            a.execute_reserved(&issuer, token, fixture_connection(), |_| panic!(
                "committed callback replay"
            ))
            .unwrap(),
            RequestCompletion::Processed
        );
        for _ in 0..15 {
            a.issue_grant(&issuer, fixture_connection()).unwrap();
        }
        assert_eq!(
            a.issue_grant(&issuer, fixture_connection()),
            Err(RegistrationError::Capacity(CapacityError::NoGrantSlot))
        );

        if completion_first {
            assert_eq!(
                a.take_completion(&submit, token, fixture_connection())
                    .unwrap(),
                Some(RequestCompletion::Processed)
            );
        } else {
            assert!(
                a.finish_attempt(&issuer, claim.token, SettlementBit::default())
                    .unwrap()
            );
        }
        assert_eq!(
            a.issue_grant(&issuer, fixture_connection()),
            Err(RegistrationError::Capacity(CapacityError::NoGrantSlot)),
            "the other pin must still retain the revoked grant"
        );
        if completion_first {
            assert!(
                a.finish_attempt(&issuer, claim.token, SettlementBit::default())
                    .unwrap()
            );
        } else {
            assert_eq!(
                a.take_completion(&submit, token, fixture_connection())
                    .unwrap(),
                Some(RequestCompletion::Processed)
            );
        }
        let (replacement, _) = a.issue_grant(&issuer, fixture_connection()).unwrap();
        assert_ne!(replacement, grant);
        assert!(!a.finish_attempt(&issuer, claim.token, both()).unwrap());
        assert!(
            a.take_completion(&submit, token, fixture_connection())
                .is_err()
        );
        assert!(a.abandon_request(&issuer, token).is_err());
        assert!(a.next_debt(&mut 0).is_none());
    }
}

#[test]
fn old_finished_attempt_cannot_settle_new_hold_after_record_and_attempt_slot_reuse() {
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
    let key = Input::key(30).unwrap();
    let context = ExecutionContext {
        connection: fixture_connection(),
        generation,
        epoch: 0,
        publication: 0,
        request: 1,
    };
    let to = Recipient {
        recipient: 42,
        connection_generation: 1,
    };
    a.execute_press(&submit, cap, key, context, to).unwrap();
    let ReleaseOutcome::DeliverTo(old_hold) = a.release(&submit, cap, key, context).unwrap() else {
        panic!("expected release")
    };
    assert!(
        !a.settle(
            &issuer,
            Some(grant),
            key,
            old_hold,
            SettlementBit {
                native_reconciled: true,
                recipient_settled: false
            }
        )
        .unwrap()
    );
    let old = a.claim_next_attempt(&issuer, &mut 0).unwrap().unwrap();
    assert!(a.finish_attempt(&issuer, old.token, both()).unwrap());

    a.execute_press(&submit, cap, key, context, to).unwrap();
    let ReleaseOutcome::DeliverTo(new_hold) = a.release(&submit, cap, key, context).unwrap() else {
        panic!("expected release")
    };
    assert!(
        !a.settle(
            &issuer,
            Some(grant),
            key,
            new_hold,
            SettlementBit {
                native_reconciled: true,
                recipient_settled: false
            }
        )
        .unwrap()
    );
    let new = a.claim_next_attempt(&issuer, &mut 0).unwrap().unwrap();
    assert_ne!(new.hold, old.hold);
    assert_ne!(new.token, old.token);
    assert!(!a.finish_attempt(&issuer, old.token, both()).unwrap());
    assert!(
        !a.settle(&issuer, Some(grant), key, old.hold, both())
            .unwrap()
    );
    assert_eq!(
        a.execute_press(&submit, cap, key, context, to),
        Err(RegistrationError::ReleaseBarrier)
    );
    assert_eq!(a.next_debt(&mut 0).unwrap().0, new.hold);
    assert!(a.finish_attempt(&issuer, new.token, both()).unwrap());
    assert!(
        a.execute_press(&submit, cap, key, context, to)
            .unwrap()
            .first_press()
    );
}

fn fixture_connection() -> sophia_input_authority::ConnectionIdentity {
    sophia_input_authority::ConnectionIdentity {
        recipient: 1,
        connection_generation: 1,
    }
}
