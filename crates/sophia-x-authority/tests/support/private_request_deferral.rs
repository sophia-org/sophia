#[test]
fn private_guarded_deferral_keeps_original_request_and_executes_once() {
    use sophia_input_authority::{
        ExecutionDisposition as D, RequestCompletion as C, RequestExecution as E,
    };
    let client = XServerFrontendClientId::from_raw(9750);
    let fixture = prepared_ordered_fixture(client);
    let private = fixture.runner.frontend.as_ref().unwrap();
    let role = private
        .reservation_role(client, DeviceId::from_raw(2))
        .unwrap();
    let stamp = private.control_gate().stamp().unwrap();
    let custody = role.reserve(stamp, 17).unwrap().accepted();
    let mut original_context = None;
    for _ in 0..3 {
        assert_eq!(
            private
                .participant
                .execute_current_or_defer(&custody, client, |permit, _| {
                    assert_eq!(permit.request_token(), custody.token());
                    let context = permit.context();
                    let fields = (
                        context.connection,
                        context.generation,
                        context.epoch,
                        context.publication,
                        context.request,
                    );
                    if let Some(original) = original_context {
                        assert_eq!(fields, original);
                    }
                    original_context = Some(fields);
                    Ok(D::Defer)
                })
                .unwrap()
                .unwrap(),
            E::Deferred
        );
        assert_eq!(
            custody.phase.get(),
            PrivateRequestPhase::DeferredBeforeEffect
        );
        assert_eq!(custody.observe().unwrap(), None);
        assert!(
            role.reserve(stamp, 18).is_err(),
            "the original grant still owes its request cell"
        );
    }
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(&custody, client, |permit, _| {
                assert_eq!(permit.request_token(), custody.token());
                assert_eq!(permit.context().request, 17);
                permit.begin_external_effect()?;
                Ok(D::Complete)
            })
            .unwrap()
            .unwrap(),
        E::Completed(C::Processed)
    );
    assert_eq!(custody.phase.get(), PrivateRequestPhase::Settled);
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(&custody, client, |_, _| {
                panic!("a completed original request must not execute again")
            })
            .unwrap()
            .unwrap(),
        E::Completed(C::Processed)
    );
    assert_eq!(custody.observe().unwrap(), Some(C::Processed));
    assert!(role.reserve(stamp, 18).is_ok());
}

#[test]
fn dropping_a_deferred_private_request_does_not_dispose_its_accepted_cell() {
    use sophia_input_authority::{ExecutionDisposition as D, RequestExecution as E};
    let client = XServerFrontendClientId::from_raw(9751);
    let fixture = prepared_ordered_fixture(client);
    let private = fixture.runner.frontend.as_ref().unwrap();
    let role = private
        .reservation_role(client, DeviceId::from_raw(2))
        .unwrap();
    let stamp = private.control_gate().stamp().unwrap();
    let custody = role.reserve(stamp, 17).unwrap().accepted();
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(&custody, client, |_, _| Ok(D::Defer))
            .unwrap()
            .unwrap(),
        E::Deferred
    );
    drop(custody);
    assert!(
        role.reserve(stamp, 18).is_err(),
        "losing custody is not a terminal answer or unpublished disposal"
    );
}

#[test]
fn an_entered_private_request_cannot_be_relabelled_as_effect_free_deferral() {
    let client = XServerFrontendClientId::from_raw(9752);
    let fixture = prepared_ordered_fixture(client);
    let private = fixture.runner.frontend.as_ref().unwrap();
    let role = private
        .reservation_role(client, DeviceId::from_raw(2))
        .unwrap();
    let stamp = private.control_gate().stamp().unwrap();
    let custody = role.reserve(stamp, 17).unwrap().accepted();
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = private
            .participant
            .execute_current_or_defer(&custody, client, |_, _| {
                panic!("interrupt the actual guarded source callback")
            });
    }));
    assert!(interrupted.is_err());
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(&custody, client, |_, _| {
                panic!("an interrupted attempt cannot re-enter its source")
            }),
        Ok(Err(PrivateAuthorityRefusal::RequestUnresolved))
    );
    assert_eq!(custody.phase.get(), PrivateRequestPhase::Entered);
    assert_eq!(custody.observe(), Err(PrivateAuthorityRefusal::Unreachable));
}

#[test]
fn deferred_private_request_rechecks_its_exact_admission_before_the_source() {
    use sophia_input_authority::{ExecutionDisposition as D, RequestExecution as E};
    let client = XServerFrontendClientId::from_raw(9753);
    let fixture = prepared_ordered_fixture(client);
    let private = fixture.runner.frontend.as_ref().unwrap();
    let role = private
        .reservation_role(client, DeviceId::from_raw(2))
        .unwrap();
    let stamp = private.control_gate().stamp().unwrap();
    let custody = role.reserve(stamp, 17).unwrap().accepted();
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(&custody, client, |_, _| Ok(D::Defer))
            .unwrap()
            .unwrap(),
        E::Deferred
    );
    private
        .participant
        .revoke_admission(client, custody.admission())
        .unwrap();
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(&custody, client, |_, _| {
                panic!("revocation cannot be replaced by a fresh source decision")
            }),
        Err(PrivateAdmissionRefusal::NotAdmitted)
    );
    assert_eq!(
        custody.phase.get(),
        PrivateRequestPhase::DeferredBeforeEffect
    );
}
