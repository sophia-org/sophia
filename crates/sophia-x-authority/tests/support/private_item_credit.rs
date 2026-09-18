// Prepared private runner controls using the actual admission store and
// common completion. Native/render facts are supplied by the shared fixture.

#[test]
fn completed_ordered_input_returns_its_storage_credit_after_item_disposal() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9680),
        durable.clone(),
    );
    for offset in 0..12 {
        let route = button_to(
            fixture.surface,
            XAuthorityInputDeliveryId::from_raw(99800 + offset),
            272,
            false,
        );
        fixture
            .ingress
            .submit(&fixture.keeper.lease(), route)
            .unwrap();
        assert_eq!(durable.reserved(), Some(1));
        let PrivatePreparedRunner {
            frontend,
            keyboards,
            watch,
            ..
        } = &mut fixture.runner;
        let private = frontend.as_mut().unwrap();
        assert!(matches!(
            private
                .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
                .unwrap(),
            PrivateOrderedStep::Decided(_)
        ));
        assert_eq!(
            durable.reserved(),
            Some(1),
            "execution does not dispose the item"
        );
        assert!(matches!(
            private.deliver_one(&mut |_, _| Ok(())).unwrap(),
            PrivateDeliveryStep::Advanced {
                report: Some(PrivateDelivered {
                    completion: Some(_),
                    ..
                }),
                ..
            }
        ));
        assert!(private.terminal.turn.is_empty() && private.terminal.delivering.is_empty());
        assert_eq!(
            durable.reserved(),
            Some(0),
            "the exact disposed item returns one storage credit"
        );
    }
}

#[test]
fn unobserved_or_dropped_ordered_items_keep_their_exact_storage_charge() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9681),
        durable.clone(),
    );
    fixture
        .ingress
        .submit(
            &fixture.keeper.lease(),
            button_to(
                fixture.surface,
                XAuthorityInputDeliveryId::from_raw(99820),
                272,
                false,
            ),
        )
        .unwrap();
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().unwrap();
    private
        .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
        .unwrap();
    let PrivateOrderedItem::Ran { mut custody, .. } = private.terminal.turn.pop().unwrap() else {
        panic!("the original no-held-button release completed");
    };
    assert!(
        !custody.finish_item(),
        "an unobserved outcome retains its charge"
    );
    assert_eq!(durable.reserved(), Some(1));
    assert!(custody.observe().unwrap().is_some());
    assert_eq!(
        durable.reserved(),
        Some(1),
        "observation alone retains item storage"
    );
    assert!(custody.finish_item());
    assert_eq!(durable.reserved(), Some(0));
    assert!(custody.finish_item());
    assert_eq!(
        durable.reserved(),
        Some(0),
        "the release right was consumed once"
    );
    drop(custody);

    fixture
        .ingress
        .submit(
            &fixture.keeper.lease(),
            button_to(
                fixture.surface,
                XAuthorityInputDeliveryId::from_raw(99821),
                272,
                false,
            ),
        )
        .unwrap();
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().unwrap();
    private
        .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
        .unwrap();
    let item = private.terminal.turn.pop().unwrap();
    drop(item);
    assert_eq!(
        durable.reserved(),
        Some(1),
        "losing accepted custody is not disposal evidence"
    );
}

#[test]
fn refused_ordered_item_keeps_its_charge_through_retained_handover() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9682),
        durable.clone(),
    );
    // This recipient selected buttons only. The actual source refuses motion.
    fixture
        .ingress
        .submit(
            &fixture.keeper.lease(),
            motion_to(fixture.surface, XAuthorityInputDeliveryId::from_raw(99822)),
        )
        .unwrap();
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().unwrap();
    assert!(matches!(
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
            .unwrap(),
        PrivateOrderedStep::Decided(..)
    ));
    assert!(matches!(
        private.terminal.turn.first(),
        Some(PrivateOrderedItem::Refused { .. })
    ));
    private.deliver_one(&mut |_, _| Ok(())).unwrap();
    assert_eq!(private.terminal.undelivered.len(), 1);
    let settlement = fixture.runner.shutdown();
    assert_eq!(durable.reserved(), Some(1));
    drop(settlement);
    assert_eq!(durable.reserved(), Some(1));
    let held = durable.inner.lock().unwrap();
    assert_eq!(held.terminal.len(), 1);
    assert_eq!(held.terminal[0].undelivered.len(), 1);
}

#[test]
fn retained_item_buffers_use_the_accepted_store_bound_before_exposure() {
    let fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9683),
        PrivateSettlementOwner::with_capacity(40),
    );
    let inventory = &fixture.runner.frontend.as_ref().unwrap().terminal;
    assert_eq!(inventory.item_capacity, 40);
    for capacity in [
        inventory.turn.capacity(),
        inventory.delivering.capacity(),
        inventory.undelivered.capacity(),
    ] {
        assert!(capacity >= 40);
    }
    assert_eq!(
        inventory.transients.records.capacity(),
        2 * 4 + PRIVATE_CLEANUP_RESERVE
    );
}

#[test]
fn retained_request_uses_its_cached_actual_outcome_and_releases_only_its_store() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let sibling = PrivateSettlementOwner::with_capacity(4);
    sibling.reserve().unwrap();
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9684),
        durable.clone(),
    );
    fixture
        .ingress
        .submit(
            &fixture.keeper.lease(),
            button_to(
                fixture.surface,
                XAuthorityInputDeliveryId::from_raw(99830),
                272,
                false,
            ),
        )
        .unwrap();
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().unwrap();
    private
        .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
        .unwrap();
    let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.first() else {
        panic!("the actual request ran");
    };
    assert!(custody.observe().unwrap().is_some());
    // The common cell has gone; only its exact cached outcome can finish the
    // accepted item after this observation/disposal interval.
    assert!(custody.observe().is_err());
    let mut settlement = fixture.runner.shutdown();
    let terminal = settlement.terminal.as_mut().unwrap();
    assert_eq!(
        terminal.retire_request_one(&mut 0).unwrap(),
        PrivateTerminalVisit::Request { disposed: true }
    );
    assert_eq!(durable.reserved(), Some(0));
    assert_eq!(sibling.reserved(), Some(1));
    assert_eq!(
        terminal.retire_request_one(&mut 0).unwrap(),
        PrivateTerminalVisit::Request { disposed: false }
    );
    assert_eq!(sibling.reserved(), Some(1));
}

#[test]
fn retained_deferred_request_requires_actual_cancellation_before_disposal() {
    use sophia_input_authority::{ExecutionDisposition, RequestExecution};
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9685),
        durable.clone(),
    );
    fixture
        .ingress
        .submit(
            &fixture.keeper.lease(),
            motion_to(fixture.surface, XAuthorityInputDeliveryId::from_raw(99831)),
        )
        .unwrap();
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().unwrap();
    // Actual refused budget start leaves the accepted current item unrun.
    assert!(
        private
            .step_once(
                keyboards,
                &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
                watch.as_ref().unwrap()
            )
            .is_err()
    );
    let Some(PrivateOrderedItem::Refused { custody, .. }) = private.terminal.current.as_ref()
    else {
        panic!("owned current");
    };
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(custody, fixture.client, |_, _| Ok(
                ExecutionDisposition::Defer
            ))
            .unwrap()
            .unwrap(),
        RequestExecution::Deferred
    );
    let admission = custody.admission();
    assert_eq!(
        private.terminal.retire_request_one(&mut 0).unwrap(),
        PrivateTerminalVisit::Request { disposed: false }
    );
    assert_eq!(durable.reserved(), Some(1));
    private
        .participant
        .revoke_admission(fixture.client, admission)
        .unwrap();
    assert_eq!(
        private.terminal.retire_request_one(&mut 0).unwrap(),
        PrivateTerminalVisit::Request { disposed: true }
    );
    assert!(private.terminal.current.is_none());
    assert_eq!(durable.reserved(), Some(0));
}

#[test]
fn retained_failed_application_keeps_item_credit_after_completion_and_revocation() {
    use sophia_input_authority::{RegistrationError, RequestCompletion, RequestExecution};
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9686),
        durable.clone(),
    );
    fixture
        .ingress
        .submit(
            &fixture.keeper.lease(),
            motion_to(fixture.surface, XAuthorityInputDeliveryId::from_raw(99832)),
        )
        .unwrap();
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().unwrap();
    assert!(
        private
            .step_once(
                keyboards,
                &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
                watch.as_ref().unwrap()
            )
            .is_err()
    );
    let Some(PrivateOrderedItem::Refused { custody, .. }) = private.terminal.current.as_ref()
    else {
        panic!("owned current");
    };
    let admission = custody.admission();
    assert_eq!(
        private
            .participant
            .execute_current_or_defer(custody, fixture.client, |permit, _| {
                permit.begin_external_effect()?;
                Err(RegistrationError::RoutingUnavailable)
            })
            .unwrap()
            .unwrap(),
        RequestExecution::Completed(RequestCompletion::FailedAfterApplication(
            RegistrationError::RoutingUnavailable
        ))
    );
    for _ in 0..2 {
        assert_eq!(
            private.terminal.retire_request_one(&mut 0).unwrap(),
            PrivateTerminalVisit::Request { disposed: false }
        );
        assert_eq!(durable.reserved(), Some(1));
    }
    private
        .participant
        .revoke_admission(fixture.client, admission)
        .unwrap();
    assert_eq!(
        private.terminal.retire_request_one(&mut 0).unwrap(),
        PrivateTerminalVisit::Request { disposed: false }
    );
    assert_eq!(durable.reserved(), Some(1));
    assert!(private.terminal.current.is_some());
}
