//! Guarded ineligibility keeps the original reserved identity and completion.
//! These controls establish common semantics, not adapter freeze ordering.
use sophia_input_authority::{
    AuthorityInstance, Capacity, CapacityError, ConnectionIdentity, DeviceCapability,
    ExecutionContext, ExecutionDisposition, GrantId, Input, InstanceId, IssuerHandle, Recipient,
    RegistrationError, ReleaseOutcome, RequestCompletion, RequestExecution, RequestToken,
    SeatBinding, SubmitHandle,
};
use sophia_protocol::{DeviceId, SeatId};

struct Fixture {
    authority: AuthorityInstance,
    issuer: IssuerHandle,
    submit: SubmitHandle,
    grant: GrantId,
    capability: DeviceCapability,
    context: ExecutionContext,
    token: RequestToken,
}

fn connection() -> ConnectionIdentity {
    ConnectionIdentity {
        recipient: 31,
        connection_generation: 7,
    }
}

fn fixture() -> Fixture {
    let (mut authority, issuer, submit) = AuthorityInstance::new(
        SeatBinding::new(InstanceId::new(17), SeatId::from_raw(1)),
        Capacity::PLANNED,
        9,
    )
    .unwrap();
    let (grant, generation) = authority.issue_grant(&issuer, connection()).unwrap();
    let capability = authority
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(1))
        .unwrap();
    let context = ExecutionContext {
        connection: connection(),
        generation,
        epoch: 0,
        publication: 0,
        request: 99,
    };
    let token = authority
        .reserve_request(&submit, capability, context)
        .unwrap();
    Fixture {
        authority,
        issuer,
        submit,
        grant,
        capability,
        context,
        token,
    }
}

fn defer(f: &mut Fixture) -> RequestExecution {
    f.authority
        .execute_reserved_or_defer(&f.issuer, f.token, connection(), |permit| {
            assert_eq!(permit.request_token(), f.token);
            assert_original_context(permit.context(), f.context);
            Ok(ExecutionDisposition::Defer)
        })
        .unwrap()
}

fn assert_original_context(actual: ExecutionContext, expected: ExecutionContext) {
    assert_eq!(actual.connection, expected.connection);
    assert_eq!(actual.generation, expected.generation);
    assert_eq!(actual.epoch, expected.epoch);
    assert_eq!(actual.publication, expected.publication);
    assert_eq!(actual.request, expected.request);
}

#[test]
fn deferral_retains_original_identity_credit_and_empty_completion_until_one_real_execution() {
    let mut f = fixture();
    for _ in 0..3 {
        assert_eq!(defer(&mut f), RequestExecution::Deferred);
        assert_eq!(
            f.authority
                .take_completion(&f.submit, f.token, connection())
                .unwrap(),
            None
        );
        assert_eq!(
            f.authority
                .reserve_request(&f.submit, f.capability, f.context),
            Err(RegistrationError::Capacity(CapacityError::NoCompletionCell))
        );
        assert!(f.authority.next_debt(&mut 0).is_none());
    }
    let mut calls = 0;
    assert_eq!(
        f.authority
            .execute_reserved_or_defer(&f.issuer, f.token, connection(), |permit| {
                calls += 1;
                assert_eq!(permit.request_token(), f.token);
                assert_original_context(permit.context(), f.context);
                assert!(
                    permit
                        .press(
                            Input::key(50).unwrap(),
                            Recipient {
                                recipient: 91,
                                connection_generation: 3
                            }
                        )?
                        .first_press()
                );
                Ok(ExecutionDisposition::Complete)
            })
            .unwrap(),
        RequestExecution::Completed(RequestCompletion::Processed)
    );
    assert_eq!(
        f.authority
            .execute_reserved_or_defer(&f.issuer, f.token, connection(), |_| {
                calls += 1;
                panic!("the completed original token cannot execute twice")
            })
            .unwrap(),
        RequestExecution::Completed(RequestCompletion::Processed)
    );
    assert_eq!(calls, 1);
    assert_eq!(
        f.authority
            .take_completion(&f.submit, f.token, connection())
            .unwrap(),
        Some(RequestCompletion::Processed)
    );
}

#[test]
fn defer_after_an_external_effect_is_terminal_and_cannot_authorize_replay() {
    let mut f = fixture();
    let completion = RequestCompletion::FailedAfterApplication(RegistrationError::RequestConsumed);
    assert_eq!(
        f.authority
            .execute_reserved_or_defer(&f.issuer, f.token, connection(), |permit| {
                permit.begin_external_effect()?;
                Ok(ExecutionDisposition::Defer)
            })
            .unwrap(),
        RequestExecution::Completed(completion)
    );
    assert_eq!(
        f.authority
            .execute_reserved_or_defer(&f.issuer, f.token, connection(), |_| {
                panic!("post-effect work cannot become eligible a second time")
            })
            .unwrap(),
        RequestExecution::Completed(completion)
    );
}

#[test]
fn consuming_a_permit_without_an_effect_still_forbids_deferral() {
    let mut f = fixture();
    assert_eq!(
        f.authority
            .execute_reserved_or_defer(&f.issuer, f.token, connection(), |permit| {
                assert_eq!(
                    permit.release(Input::key(50).unwrap())?,
                    ReleaseOutcome::NotHeld
                );
                Ok(ExecutionDisposition::Defer)
            })
            .unwrap(),
        RequestExecution::Completed(RequestCompletion::Refused(
            RegistrationError::RequestConsumed
        ))
    );
}

#[test]
fn deferred_work_cannot_be_restamped_after_a_publication_or_security_transition() {
    for epoch in [0, 1] {
        let mut f = fixture();
        assert_eq!(defer(&mut f), RequestExecution::Deferred);
        f.authority.begin_transition(&f.issuer, 1, epoch).unwrap();
        f.authority.publish(&f.issuer, 1, epoch).unwrap();
        assert_eq!(
            f.authority
                .execute_reserved_or_defer(&f.issuer, f.token, connection(), |_| {
                    panic!(
                        "thaw must observe original cancellation rather than borrow current context"
                    )
                })
                .unwrap(),
            RequestExecution::Completed(RequestCompletion::Cancelled)
        );
        assert_eq!(
            f.authority
                .take_completion(&f.submit, f.token, connection())
                .unwrap(),
            Some(RequestCompletion::Cancelled)
        );
    }
}

#[test]
fn deferred_work_preserves_revocation_and_replacement_connection_checks() {
    let mut f = fixture();
    assert_eq!(defer(&mut f), RequestExecution::Deferred);
    let replacement = ConnectionIdentity {
        connection_generation: 8,
        ..connection()
    };
    assert_eq!(
        f.authority
            .execute_reserved_or_defer(&f.issuer, f.token, replacement, |_| {
                panic!("a replacement connection cannot thaw old custody")
            }),
        Err(RegistrationError::WrongConnection)
    );
    assert_eq!(
        f.authority
            .take_completion(&f.submit, f.token, connection())
            .unwrap(),
        None
    );
    f.authority.revoke_grant(&f.issuer, f.grant).unwrap();
    assert_eq!(
        f.authority
            .execute_reserved_or_defer(&f.issuer, f.token, connection(), |_| {
                panic!("a revoked request cannot become a new native effect")
            })
            .unwrap(),
        RequestExecution::Completed(RequestCompletion::Cancelled)
    );
}
