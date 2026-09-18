#[test]
fn frozen_requests_keep_original_custody_allow_controls_and_resume_in_order() {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId::from_raw(9768));
    install_prepared_keyboard_freeze(&fixture);
    let lease = fixture.keeper.lease();
    let first = fixture.ingress.submit(&lease, key_service_route(fixture.surface, 997680, 42, false)).unwrap();
    assert!(matches!(fixture.runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Deferred { sequence, watched: true }, charge: Some(_) } if sequence == first));
    let original = {
        let terminal = &fixture.runner.frontend().terminal;
        assert!(terminal.turn.is_empty() && terminal.holds.is_empty());
        assert!(terminal.native_pending.is_none() && terminal.pending_custody.is_none());
        assert_eq!(terminal.next_event_order, 0);
        let row = terminal.frozen.front().unwrap();
        assert_eq!(row.custody.phase.get(), PrivateRequestPhase::DeferredBeforeEffect);
        assert!(row.custody.observe().unwrap().is_none());
        (row.custody.token(), Arc::clone(&row.custody.input_completion().unwrap().cell))
    };
    let later_ingress = fixture.runner.ingress_for(&lease, fixture.client, DeviceId::from_raw(2)).unwrap();
    let later = later_ingress.submit(&lease, button_to(fixture.surface, XAuthorityInputDeliveryId::from_raw(997681), 272, false)).unwrap();
    let control = fixture.runner.control_producer(&lease).unwrap()
        .submit(&lease, configure(fixture.client, fixture.surface, 997682)).unwrap();
    assert!(matches!(fixture.runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Resumed { sequence, deferred: true, watched: true }, .. } if sequence == first));
    assert!(matches!(fixture.runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Deferred { sequence, watched: true }, .. } if sequence == later));
    fixture.runner.execute_accounted_step().unwrap();
    assert!(matches!(fixture.runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Routed(sequence), .. } if sequence == control));
    let terminal = &fixture.runner.frontend().terminal;
    assert_eq!(terminal.frozen.len(), 2);
    assert!(terminal.turn.is_empty());
    assert!(terminal.frozen[1].source.is_none(), "later input has selected no source/recipient");
    assert_eq!(terminal.frozen[0].custody.token(), original.0);
    assert!(Arc::ptr_eq(&terminal.frozen[0].custody.input_completion().unwrap().cell, &original.1));
    fixture.runner.frontend().broker.registry.input_authority.lock().unwrap()
        .allow_events(fixture.namespace, fixture.client.raw(), 3).unwrap();
    for expected in [first, later] {
        assert!(matches!(fixture.runner.execute_accounted_step().unwrap(),
            PrivateAccountedStep::Step { step: PrivateOrderedStep::Resumed { sequence, deferred: false, watched: true }, charge: Some(_) } if sequence == expected));
    }
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert!(private.terminal.frozen.is_empty());
    assert_eq!(private.terminal.turn.iter().map(PrivateOrderedItem::sequence).collect::<Vec<_>>(), vec![first, later]);
    let PrivateOrderedItem::Ran { custody, .. } = &private.terminal.turn[0] else { panic!("original thaw runs once") };
    assert_eq!(custody.token(), original.0);
    assert!(Arc::ptr_eq(&custody.input_completion().unwrap().cell, &original.1));
    assert_eq!(custody.phase.get(), PrivateRequestPhase::Settled);
    for _ in 0..2 {
        assert!(matches!(private.deliver_one(&mut |_, _| Ok(())).unwrap(), PrivateDeliveryStep::Advanced { report: Some(_), .. }));
    }
    assert!(private.terminal.turn.is_empty());
}

#[test]
fn frozen_replacement_refuses_original_without_effect_and_handover_retains_waiting_rows() {
    let durable = PrivateSettlementOwner::with_capacity(5);
    let mut fixture = prepared_ordered_fixture_with_store(XServerFrontendClientId::from_raw(9769), durable.clone());
    install_prepared_keyboard_freeze(&fixture);
    fixture.ingress.submit(&fixture.keeper.lease(), key_service_route(fixture.surface, 997690, 42, false)).unwrap();
    fixture.runner.execute_accounted_step().unwrap();
    let authority = Arc::clone(&fixture.runner.frontend().broker.registry.input_authority);
    authority.lock().unwrap().ungrab_keyboard(fixture.namespace, fixture.client.raw());
    install_prepared_keyboard_freeze(&fixture);
    authority.lock().unwrap().allow_events(fixture.namespace, fixture.client.raw(), 3).unwrap();
    fixture.runner.execute_accounted_step().unwrap();
    let Some(PrivateOrderedItem::Refused { refusal: PrivateExecutionRefusal::Native(private_native::Refusal::FreezeInvalidated), custody, .. }) = fixture.runner.frontend().terminal.turn.first() else {
        panic!("a replacement thaw must refuse the original request");
    };
    assert_eq!(custody.phase.get(), PrivateRequestPhase::Settled);
    assert!(fixture.runner.frontend().terminal.holds.is_empty());
    assert_eq!(fixture.runner.frontend().terminal.next_event_order, 0);

    let later_ingress = fixture.runner.ingress_for(&fixture.keeper.lease(), fixture.client, DeviceId::from_raw(2)).unwrap();
    install_prepared_keyboard_freeze(&fixture);
    later_ingress.submit(&fixture.keeper.lease(), key_service_route(fixture.surface, 997691, 30, false)).unwrap();
    fixture.runner.execute_accounted_step().unwrap();
    assert_eq!(fixture.runner.frontend().terminal.frozen.len(), 1);
    assert!(fixture.runner.frontend().terminal.frozen.capacity() >= 5);
    let settlement = fixture.runner.shutdown();
    drop(settlement);
    assert_eq!(durable.reserved(), Some(2));
    let store = durable.inner.lock().unwrap();
    assert_eq!(store.terminal.len(), 1);
    assert_eq!(store.terminal[0].frozen.len(), 1);
    assert!(store.terminal[0].frozen[0].custody.observe().unwrap().is_none());
    assert!(!store.terminal[0].is_empty());
}
