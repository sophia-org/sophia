// Actual prepared admission/common outcomes. The source fixture selects only
// buttons, so motion is refused without a source effect. Recovery entry
// removal/replacement below is an explicitly staged publication fault.

fn step_refused_request(fixture: &mut PreparedOrderedFixture) {
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
    assert!(matches!(
        private.terminal.turn.first(),
        Some(PrivateOrderedItem::Refused { .. })
    ));
}

#[test]
fn actual_refusal_answers_its_original_delivery_and_allows_bounded_grant_reuse() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let client = XServerFrontendClientId::from_raw(9691);
    let mut fixture = prepared_ordered_fixture_with_store(client, durable.clone());
    for offset in 0..12 {
        let id = XAuthorityInputDeliveryId::from_raw(99910 + offset);
        fixture
            .ingress
            .submit(&fixture.keeper.lease(), motion_to(fixture.surface, id))
            .unwrap();
        let recovery = fixture
            .runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .input_recovery
            .clone();
        let cell = recovery.completion_for(id).unwrap().unwrap();
        step_refused_request(&mut fixture);
        assert!(cell.answer().is_none());
        assert_eq!(durable.reserved(), Some(1));
        let private = fixture.runner.frontend.as_mut().unwrap();
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
        assert_eq!(
            cell.answer(),
            Some(XAuthorityClientInputDelivery {
                client,
                delivery: id,
                outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
            })
        );
        assert_eq!(durable.reserved(), Some(0));
        assert!(private.terminal.turn.is_empty());
        assert!(private.terminal.delivering.is_empty());
        assert!(private.terminal.undelivered.is_empty());
    }
}

#[test]
fn refused_receipt_publication_keeps_the_item_and_retries_only_its_actual_outcome() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9692),
        durable.clone(),
    );
    let id = XAuthorityInputDeliveryId::from_raw(99930);
    fixture
        .ingress
        .submit(&fixture.keeper.lease(), motion_to(fixture.surface, id))
        .unwrap();
    step_refused_request(&mut fixture);
    let private = fixture.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let entry = recovery.state.lock().unwrap().tickets.remove(&id).unwrap();
    let cell = entry.completion.clone();
    private.deliver_one(&mut |_, _| Ok(())).unwrap();
    assert!(cell.answer().is_none());
    assert_eq!(durable.reserved(), Some(1));
    assert_eq!(private.terminal.undelivered.len(), 1);
    let PrivateOrderedItem::Refused { custody, .. } = &private.terminal.undelivered[0].item else {
        unreachable!()
    };
    assert!(matches!(
        custody.observed_outcome.get(),
        Some(sophia_input_authority::RequestCompletion::Refused(_))
    ));
    recovery.state.lock().unwrap().tickets.insert(id, entry);
    let mut charges = 0;
    private
        .deliver_one(&mut |_, _| {
            charges += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(charges, 1);
    assert_eq!(
        cell.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::RouteRejected
    );
    assert!(private.terminal.undelivered.is_empty());
    assert_eq!(durable.reserved(), Some(0));
}

#[test]
fn refused_request_never_answers_a_replacement_delivery_with_the_same_number() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9693),
        durable.clone(),
    );
    let id = XAuthorityInputDeliveryId::from_raw(99931);
    let route = motion_to(fixture.surface, id);
    fixture
        .ingress
        .submit(&fixture.keeper.lease(), route.clone())
        .unwrap();
    step_refused_request(&mut fixture);
    let private = fixture.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let original = recovery.completion_for(id).unwrap().unwrap();
    recovery.abort_enqueue(Some(id));
    recovery
        .admit_typed(&route, 1, std::time::Instant::now())
        .unwrap();
    let replacement = recovery.completion_for(id).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&original, &replacement));
    for _ in 0..3 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
        assert!(original.answer().is_none());
        assert!(replacement.answer().is_none());
        assert_eq!(durable.reserved(), Some(1));
        assert_eq!(private.terminal.undelivered.len(), 1);
    }
}
