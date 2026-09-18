//! Independent retained-cell regressions. These prove common-authority state,
//! not eventfd delivery, executor liveness or recipient application processing.
use sophia_input_authority::{
    AuthorityInstance, Capacity, CapacityError, DeviceCapability, ExecutionContext,
    GrantGeneration, GrantId, Input, InstanceId, IssuerHandle, Recipient, RegistrationError,
    ReleaseOutcome, RequestCompletion, RequestToken, SeatBinding, SettlementBit, SubmitHandle,
};
use sophia_protocol::{DeviceId, SeatId};

struct Fixture {
    authority: AuthorityInstance,
    issuer: IssuerHandle,
    submit: SubmitHandle,
}

fn fixture() -> Fixture {
    let (authority, issuer, submit) = AuthorityInstance::new(
        SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1)),
        Capacity::PLANNED,
        9,
    )
    .unwrap();
    Fixture {
        authority,
        issuer,
        submit,
    }
}

fn granted(f: &mut Fixture) -> (GrantId, DeviceCapability, ExecutionContext) {
    let (id, generation) = f
        .authority
        .issue_grant(&f.issuer, fixture_connection())
        .unwrap();
    let cap = f
        .authority
        .allocate_device(&f.issuer, id, generation, DeviceId::from_raw(100))
        .unwrap();
    (id, cap, context(generation))
}

fn context(generation: GrantGeneration) -> ExecutionContext {
    ExecutionContext {
        connection: fixture_connection(),
        generation,
        epoch: 0,
        publication: 0,
        request: 1,
    }
}

fn key() -> Input {
    Input::key(30).unwrap()
}
fn recipient() -> Recipient {
    Recipient {
        recipient: 77,
        connection_generation: 1,
    }
}
fn reserve(f: &mut Fixture, cap: DeviceCapability, ctx: ExecutionContext) -> RequestToken {
    f.authority.reserve_request(&f.submit, cap, ctx).unwrap()
}

#[test]
fn duplicate_execution_observes_completion_without_replaying_effect() {
    let mut f = fixture();
    let (_, cap, ctx) = granted(&mut f);
    let token = reserve(&mut f, cap, ctx);
    let mut calls = 0;
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |permit| {
                calls += 1;
                assert_eq!(permit.source(), cap.source());
                assert_eq!(permit.context().request, ctx.request);
                assert_eq!(permit.context().generation, ctx.generation);
                assert!(permit.press(key(), recipient())?.first_press());
                Ok(())
            })
            .unwrap(),
        RequestCompletion::Processed
    );
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |_| {
                calls += 1;
                panic!("duplicate callback must not execute")
            })
            .unwrap(),
        RequestCompletion::Processed
    );
    assert_eq!(calls, 1);
    assert_eq!(
        f.authority
            .take_completion(&f.submit, token, fixture_connection())
            .unwrap(),
        Some(RequestCompletion::Processed)
    );
    assert!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |_| panic!(
                "consumed token replay"
            ))
            .is_err()
    );
}

#[test]
fn one_cell_per_grant_covers_both_devices_until_completion_consumed() {
    let mut f = fixture();
    let (id, cap, ctx) = granted(&mut f);
    let other = f
        .authority
        .allocate_device(&f.issuer, id, ctx.generation, DeviceId::from_raw(101))
        .unwrap();
    let token = reserve(&mut f, cap, ctx);
    assert_eq!(
        f.authority
            .take_completion(&f.submit, token, fixture_connection())
            .unwrap(),
        None
    );
    assert_eq!(
        f.authority.reserve_request(&f.submit, other, ctx),
        Err(RegistrationError::Capacity(CapacityError::NoCompletionCell))
    );
    f.authority
        .execute_reserved(&f.issuer, token, fixture_connection(), |_| Ok(()))
        .unwrap();
    assert_eq!(
        f.authority.reserve_request(&f.submit, other, ctx),
        Err(RegistrationError::Capacity(CapacityError::NoCompletionCell))
    );
    f.authority
        .take_completion(&f.submit, token, fixture_connection())
        .unwrap();
    assert!(f.authority.reserve_request(&f.submit, other, ctx).is_ok());
}

#[test]
fn revoke_before_execution_cancels_without_callback_or_debt() {
    let mut f = fixture();
    let (id, cap, ctx) = granted(&mut f);
    let token = reserve(&mut f, cap, ctx);
    assert_eq!(
        f.authority
            .revoke_grant(&f.issuer, id)
            .unwrap()
            .owed_releases,
        0
    );
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |_| panic!(
                "revoked callback"
            ))
            .unwrap(),
        RequestCompletion::Cancelled
    );
    assert!(f.authority.next_debt(&mut 0).is_none());
    assert_eq!(
        f.authority
            .take_completion(&f.submit, token, fixture_connection())
            .unwrap(),
        Some(RequestCompletion::Cancelled)
    );
}

#[test]
fn committed_completion_and_debt_survive_revocation_without_a_wakeup() {
    let mut f = fixture();
    let (id, cap, ctx) = granted(&mut f);
    let token = reserve(&mut f, cap, ctx);
    f.authority
        .execute_reserved(&f.issuer, token, fixture_connection(), |permit| {
            permit.press(key(), recipient())?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.authority
            .revoke_grant(&f.issuer, id)
            .unwrap()
            .owed_releases,
        1
    );
    let (hold, _) = f
        .authority
        .next_debt(&mut 0)
        .expect("applied press must retain debt");
    assert_eq!(
        f.authority
            .take_completion(&f.submit, token, fixture_connection())
            .unwrap(),
        Some(RequestCompletion::Processed)
    );
    assert_eq!(f.authority.next_debt(&mut 0).unwrap().0, hold);
    assert!(
        f.authority
            .settle(
                &f.issuer,
                Some(id),
                key(),
                hold,
                SettlementBit {
                    native_reconciled: true,
                    recipient_settled: true
                }
            )
            .unwrap()
    );
    assert!(f.authority.next_debt(&mut 0).is_none());
}

#[test]
fn callback_error_after_application_preserves_effect_and_completion() {
    let mut f = fixture();
    let (id, cap, ctx) = granted(&mut f);
    let token = reserve(&mut f, cap, ctx);
    let refused = RequestCompletion::FailedAfterApplication(RegistrationError::StaleExecution);
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |permit| {
                permit.press(key(), recipient())?;
                Err(RegistrationError::StaleExecution)
            })
            .unwrap(),
        refused
    );
    assert_eq!(
        f.authority
            .revoke_grant(&f.issuer, id)
            .unwrap()
            .owed_releases,
        1
    );
    assert!(f.authority.next_debt(&mut 0).is_some());
    assert_eq!(
        f.authority
            .take_completion(&f.submit, token, fixture_connection())
            .unwrap(),
        Some(refused)
    );
}

#[test]
fn permit_refuses_a_second_transition_without_mutating_it() {
    let mut f = fixture();
    let (id, cap, ctx) = granted(&mut f);
    let token = reserve(&mut f, cap, ctx);
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |permit| {
                permit.press(key(), recipient())?;
                assert_eq!(
                    permit.release(key()),
                    Err(RegistrationError::RequestConsumed)
                );
                assert_eq!(
                    permit.press(Input::key(31).unwrap(), recipient()),
                    Err(RegistrationError::RequestConsumed)
                );
                Ok(())
            })
            .unwrap(),
        RequestCompletion::Processed
    );
    assert_eq!(
        f.authority
            .revoke_grant(&f.issuer, id)
            .unwrap()
            .owed_releases,
        1
    );
    assert_eq!(f.authority.next_debt(&mut 0).unwrap().0.input, key());
}

#[test]
fn old_token_cannot_take_or_abandon_a_reused_slots_new_request() {
    let mut f = fixture();
    let (id, cap, ctx) = granted(&mut f);
    let old = reserve(&mut f, cap, ctx);
    f.authority.revoke_grant(&f.issuer, id).unwrap();
    f.authority
        .take_completion(&f.submit, old, fixture_connection())
        .unwrap();
    let (_, cap, ctx) = granted(&mut f);
    let new = reserve(&mut f, cap, ctx);
    assert_ne!(old, new);
    assert!(
        f.authority
            .take_completion(&f.submit, old, fixture_connection())
            .is_err()
    );
    assert!(f.authority.abandon_request(&f.issuer, old).is_err());
    assert_eq!(
        f.authority
            .take_completion(&f.submit, new, fixture_connection())
            .unwrap(),
        None
    );
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, new, fixture_connection(), |_| Ok(()))
            .unwrap(),
        RequestCompletion::Processed
    );
}

#[test]
fn sixteen_retired_cells_pin_slots_until_each_exact_completion_is_consumed() {
    let mut f = fixture();
    let mut tokens = Vec::new();
    for i in 0..16 {
        let (id, cap, ctx) = granted(&mut f);
        let token = reserve(&mut f, cap, ctx);
        if i % 2 == 0 {
            f.authority
                .execute_reserved(&f.issuer, token, fixture_connection(), |_| Ok(()))
                .unwrap();
        }
        f.authority.revoke_grant(&f.issuer, id).unwrap();
        tokens.push((
            token,
            if i % 2 == 0 {
                RequestCompletion::Processed
            } else {
                RequestCompletion::Cancelled
            },
        ));
    }
    assert_eq!(
        f.authority.issue_grant(&f.issuer, fixture_connection()),
        Err(RegistrationError::Capacity(CapacityError::NoGrantSlot))
    );
    for (token, expected) in tokens {
        assert_eq!(
            f.authority
                .take_completion(&f.submit, token, fixture_connection())
                .unwrap(),
            Some(expected)
        );
        assert!(
            f.authority
                .issue_grant(&f.issuer, fixture_connection())
                .is_ok()
        );
        assert_eq!(
            f.authority.issue_grant(&f.issuer, fixture_connection()),
            Err(RegistrationError::Capacity(CapacityError::NoGrantSlot))
        );
    }
}

#[test]
fn abandonment_invalidates_pending_token_but_keeps_applied_debt() {
    let mut f = fixture();
    let (id, cap, ctx) = granted(&mut f);
    let old = reserve(&mut f, cap, ctx);
    f.authority.abandon_request(&f.issuer, old).unwrap();
    assert!(
        f.authority
            .execute_reserved(&f.issuer, old, fixture_connection(), |_| panic!(
                "abandoned callback"
            ))
            .is_err()
    );
    let new = reserve(&mut f, cap, ctx);
    f.authority
        .execute_reserved(&f.issuer, new, fixture_connection(), |permit| {
            permit.press(key(), recipient())?;
            Ok(())
        })
        .unwrap();
    f.authority.abandon_request(&f.issuer, new).unwrap();
    assert_eq!(
        f.authority
            .revoke_grant(&f.issuer, id)
            .unwrap()
            .owed_releases,
        1
    );
    assert!(f.authority.next_debt(&mut 0).is_some());
}

#[test]
fn publication_transition_cancels_original_context_without_restamping() {
    let mut f = fixture();
    let (_, cap, ctx) = granted(&mut f);
    let old = reserve(&mut f, cap, ctx);
    f.authority.begin_transition(&f.issuer, 1, 0).unwrap();
    f.authority.publish(&f.issuer, 1, 0).unwrap();
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, old, fixture_connection(), |_| panic!(
                "old publication executed"
            ))
            .unwrap(),
        RequestCompletion::Cancelled
    );
    f.authority
        .take_completion(&f.submit, old, fixture_connection())
        .unwrap();
    assert!(f.authority.reserve_request(&f.submit, cap, ctx).is_err());
    assert!(
        f.authority
            .reserve_request(
                &f.submit,
                cap,
                ExecutionContext {
                    connection: fixture_connection(),
                    publication: 1,
                    ..ctx
                }
            )
            .is_ok()
    );
}

#[test]
fn changed_epoch_cannot_be_laundered_by_fresh_context_on_old_capability() {
    let mut f = fixture();
    let (_, cap, ctx) = granted(&mut f);
    let old = reserve(&mut f, cap, ctx);
    f.authority.begin_transition(&f.issuer, 1, 1).unwrap();
    f.authority.publish(&f.issuer, 1, 1).unwrap();
    assert_eq!(
        f.authority
            .take_completion(&f.submit, old, fixture_connection())
            .unwrap(),
        Some(RequestCompletion::Cancelled)
    );
    assert!(
        f.authority
            .reserve_request(
                &f.submit,
                cap,
                ExecutionContext {
                    connection: fixture_connection(),
                    epoch: 1,
                    publication: 1,
                    request: 2,
                    ..ctx
                }
            )
            .is_err()
    );
}

#[test]
fn failed_release_and_refused_press_cannot_discharge_existing_debt() {
    let mut f = fixture();
    let (_, cap, ctx) = granted(&mut f);
    let press = reserve(&mut f, cap, ctx);
    f.authority
        .execute_reserved(&f.issuer, press, fixture_connection(), |permit| {
            permit.press(key(), recipient())?;
            Ok(())
        })
        .unwrap();
    f.authority
        .take_completion(&f.submit, press, fixture_connection())
        .unwrap();
    let release = reserve(&mut f, cap, ctx);
    let mut hold = None;
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, release, fixture_connection(), |permit| {
                let ReleaseOutcome::DeliverTo(incarnation) = permit.release(key())? else {
                    panic!("last release missing")
                };
                hold = Some(incarnation);
                Err(RegistrationError::StaleExecution)
            })
            .unwrap(),
        RequestCompletion::FailedAfterApplication(RegistrationError::StaleExecution)
    );
    f.authority
        .take_completion(&f.submit, release, fixture_connection())
        .unwrap();
    let next = reserve(&mut f, cap, ctx);
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, next, fixture_connection(), |permit| {
                permit.press(key(), recipient())?;
                Ok(())
            })
            .unwrap(),
        RequestCompletion::Refused(RegistrationError::ReleaseBarrier)
    );
    assert_eq!(f.authority.next_debt(&mut 0).unwrap().0, hold.unwrap());
    f.authority
        .take_completion(&f.submit, next, fixture_connection())
        .unwrap();
    assert_eq!(f.authority.next_debt(&mut 0).unwrap().0, hold.unwrap());
}

#[test]
fn foreign_control_handles_cannot_observe_or_execute_retained_cells() {
    let mut f = fixture();
    let foreign = fixture();
    let (_, cap, ctx) = granted(&mut f);
    let token = reserve(&mut f, cap, ctx);
    assert!(
        f.authority
            .take_completion(&foreign.submit, token, fixture_connection())
            .is_err()
    );
    assert!(f.authority.abandon_request(&foreign.issuer, token).is_err());
    assert!(
        f.authority
            .execute_reserved(&foreign.issuer, token, fixture_connection(), |_| panic!(
                "foreign executor"
            ))
            .is_err()
    );
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |_| Ok(()))
            .unwrap(),
        RequestCompletion::Processed
    );
}

#[test]
fn an_external_effect_failure_is_not_reported_as_an_effect_free_refusal() {
    let mut f = fixture();
    let (_, cap, ctx) = granted(&mut f);
    let token = reserve(&mut f, cap, ctx);
    assert_eq!(
        f.authority
            .execute_reserved(&f.issuer, token, fixture_connection(), |permit| {
                permit.begin_external_effect()?;
                Err(RegistrationError::StaleExecution)
            })
            .unwrap(),
        RequestCompletion::FailedAfterApplication(RegistrationError::StaleExecution)
    );
    assert_eq!(
        f.authority
            .take_completion(&f.submit, token, fixture_connection())
            .unwrap(),
        Some(RequestCompletion::FailedAfterApplication(
            RegistrationError::StaleExecution
        ))
    );
}

fn fixture_connection() -> sophia_input_authority::ConnectionIdentity {
    sophia_input_authority::ConnectionIdentity {
        recipient: 1,
        connection_generation: 1,
    }
}
