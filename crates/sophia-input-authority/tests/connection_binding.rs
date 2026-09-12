//! Caller admission identity and routed recipient identity are different roles.
//! Session supplies the current caller; these checks never derive it from wire data.
use sophia_input_authority::*;
use sophia_protocol::{DeviceId, SeatId};

struct Fixture {
    authority: AuthorityInstance,
    issuer: IssuerHandle,
    submit: SubmitHandle,
    cap: DeviceCapability,
    context: ExecutionContext,
}

fn caller() -> ConnectionIdentity {
    ConnectionIdentity {
        recipient: 10,
        connection_generation: 7,
    }
}

fn mismatches() -> [ConnectionIdentity; 2] {
    [
        ConnectionIdentity {
            recipient: 10,
            connection_generation: 8,
        },
        ConnectionIdentity {
            recipient: 11,
            connection_generation: 7,
        },
    ]
}

fn target() -> Recipient {
    Recipient {
        recipient: 90,
        connection_generation: 42,
    }
}

fn key() -> Input {
    Input::key(30).unwrap()
}

fn fixture() -> Fixture {
    let (mut authority, issuer, submit) = AuthorityInstance::new(
        SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1)),
        Capacity::PLANNED,
        9,
    )
    .unwrap();
    let (grant, generation) = authority.issue_grant(&issuer, caller()).unwrap();
    let cap = authority
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(1))
        .unwrap();
    Fixture {
        authority,
        issuer,
        submit,
        cap,
        context: ExecutionContext {
            connection: caller(),
            generation,
            epoch: 0,
            publication: 0,
            request: 1,
        },
    }
}

#[test]
fn wrong_current_connection_never_runs_callback_or_consumes_pending_cell() {
    for wrong in mismatches() {
        let mut f = fixture();
        let token = f
            .authority
            .reserve_request(&f.submit, f.cap, f.context)
            .unwrap();
        assert!(
            f.authority
                .execute_reserved(&f.issuer, token, wrong, |_| panic!(
                    "wrong connection executed"
                ))
                .is_err()
        );
        assert_eq!(
            f.authority
                .take_completion(&f.submit, token, caller())
                .unwrap(),
            None
        );
        assert_eq!(
            f.authority
                .execute_reserved(&f.issuer, token, caller(), |permit| {
                    assert_eq!(permit.context().connection, caller());
                    assert!(permit.press(key(), target())?.first_press());
                    Ok(())
                })
                .unwrap(),
            RequestCompletion::Processed
        );
    }
}

#[test]
fn wrong_current_connection_cannot_observe_or_consume_completed_cell() {
    for wrong in mismatches() {
        let mut f = fixture();
        let token = f
            .authority
            .reserve_request(&f.submit, f.cap, f.context)
            .unwrap();
        f.authority
            .execute_reserved(&f.issuer, token, caller(), |_| Ok(()))
            .unwrap();
        assert!(
            f.authority
                .execute_reserved(&f.issuer, token, wrong, |_| panic!(
                    "completed callback replay"
                ))
                .is_err()
        );
        assert!(
            f.authority
                .take_completion(&f.submit, token, wrong)
                .is_err()
        );
        assert_eq!(
            f.authority
                .take_completion(&f.submit, token, caller())
                .unwrap(),
            Some(RequestCompletion::Processed)
        );
    }
}

#[test]
fn mismatched_context_cannot_reserve_a_cell_or_apply_a_press() {
    for wrong in mismatches() {
        let mut f = fixture();
        let bad = ExecutionContext {
            connection: wrong,
            ..f.context
        };
        assert!(f.authority.reserve_request(&f.submit, f.cap, bad).is_err());
        assert!(
            f.authority
                .execute_press(&f.submit, f.cap, key(), bad, target())
                .is_err()
        );
        assert_eq!(
            f.authority
                .release(&f.submit, f.cap, key(), f.context)
                .unwrap(),
            ReleaseOutcome::NotHeld
        );
        let token = f
            .authority
            .reserve_request(&f.submit, f.cap, f.context)
            .expect("refusal must not occupy completion capacity");
        assert_eq!(
            f.authority
                .execute_reserved(&f.issuer, token, caller(), |permit| {
                    assert!(permit.press(key(), target())?.first_press());
                    Ok(())
                })
                .unwrap(),
            RequestCompletion::Processed
        );
    }
}

#[test]
fn mismatched_release_preserves_the_original_connections_hold() {
    for wrong in mismatches() {
        let mut f = fixture();
        let applied = f
            .authority
            .execute_press(&f.submit, f.cap, key(), f.context, target())
            .unwrap();
        assert!(
            f.authority
                .release(
                    &f.submit,
                    f.cap,
                    key(),
                    ExecutionContext {
                        connection: wrong,
                        ..f.context
                    }
                )
                .is_err()
        );
        assert!(
            f.authority.next_debt(&mut 0).is_none(),
            "wrong caller must not retire the hold"
        );
        assert_eq!(
            f.authority
                .release(&f.submit, f.cap, key(), f.context)
                .unwrap(),
            ReleaseOutcome::DeliverTo(applied.incarnation())
        );
    }
}

#[test]
fn independent_reconnect_grant_does_not_inherit_old_request_or_source() {
    let mut f = fixture();
    let old = f
        .authority
        .reserve_request(&f.submit, f.cap, f.context)
        .unwrap();
    let reconnect = mismatches()[0];
    f.authority.revoke_grant(&f.issuer, f.cap.grant()).unwrap();
    let (grant, generation) = f.authority.issue_grant(&f.issuer, reconnect).unwrap();
    let cap = f
        .authority
        .allocate_device(&f.issuer, grant, generation, DeviceId::from_raw(1))
        .unwrap();
    assert_eq!(cap.connection(), reconnect);
    assert_ne!(cap.source(), f.cap.source());
    assert!(
        f.authority
            .take_completion(&f.submit, old, reconnect)
            .is_err()
    );
    assert!(
        f.authority
            .execute_reserved(&f.issuer, old, reconnect, |_| panic!(
                "reconnected caller replay"
            ))
            .is_err()
    );
    assert_eq!(
        f.authority
            .take_completion(&f.submit, old, caller())
            .unwrap(),
        Some(RequestCompletion::Cancelled)
    );
    let token = f
        .authority
        .reserve_request(
            &f.submit,
            cap,
            ExecutionContext {
                connection: reconnect,
                generation,
                ..f.context
            },
        )
        .unwrap();
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, reconnect, |permit| {
                permit.press(key(), target())?;
                Ok(())
            })
            .unwrap(),
        RequestCompletion::Processed
    );
}

#[test]
fn issuing_connection_is_not_confused_with_the_delivery_recipient() {
    let mut f = fixture();
    assert_eq!(f.cap.connection(), caller());
    assert_ne!(target().recipient, caller().recipient);
    assert_ne!(
        target().connection_generation,
        caller().connection_generation
    );
    let token = f
        .authority
        .reserve_request(&f.submit, f.cap, f.context)
        .unwrap();
    f.authority
        .execute_reserved(&f.issuer, token, caller(), |permit| {
            let press = permit.press(key(), target())?;
            assert_eq!(press.incarnation().recipient, target().recipient);
            assert_eq!(
                press.incarnation().connection_generation,
                target().connection_generation
            );
            Ok(())
        })
        .unwrap();
    f.authority
        .take_completion(&f.submit, token, caller())
        .unwrap();
    let release = f
        .authority
        .reserve_request(&f.submit, f.cap, f.context)
        .unwrap();
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, release, caller(), |permit| {
                let ReleaseOutcome::DeliverTo(hold) = permit.release(key())? else {
                    panic!("last hold missing")
                };
                assert_eq!(hold.recipient, target().recipient);
                assert_eq!(hold.connection_generation, target().connection_generation);
                Ok(())
            })
            .unwrap(),
        RequestCompletion::Processed
    );
}
