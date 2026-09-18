#[test]
fn exact_input_claim_preserves_original_cancellation_across_no_effect_deferral() {
    let id = XAuthorityInputDeliveryId::from_raw(9761);
    let (recovery, receipts) = claim_fixture(id);
    let original = recovery.completion_for(id).unwrap().unwrap();
    assert_eq!(
        recovery.claim_execution_for_held(id, &original),
        Ok(ExecutionClaim::Claimed)
    );
    assert!(
        recovery
            .recover(std::time::Instant::now(), true)
            .unwrap()
            .is_empty()
    );
    assert!(original.answer().is_none());
    recovery.resolve_claim_for_held(id, &original, false);
    let receipt = receipts.try_recv().unwrap();
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked
    );
    assert_eq!(original.answer(), Some(receipt));
    assert_eq!(
        recovery.claim_execution_for_held(id, &original),
        Ok(ExecutionClaim::Ended)
    );
    assert!(recovery.observe(receipt));
    assert!(recovery.ticket(id).is_none());
    recovery
        .admit_typed(
            &button_to(SurfaceId::new(1, 1), id, 272, true),
            1,
            std::time::Instant::now(),
        )
        .unwrap();
    let replacement = recovery.completion_for(id).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&original, &replacement));
    assert_eq!(
        recovery.claim_execution_for_held(id, &original),
        Ok(ExecutionClaim::Ended)
    );
    assert_eq!(
        recovery.claim_execution_for_held(id, &replacement),
        Ok(ExecutionClaim::Claimed)
    );
    recovery.resolve_claim_for_held(id, &replacement, false);
}

#[test]
fn missing_or_replaced_unanswered_input_completion_cannot_be_claimed_or_released() {
    let id = XAuthorityInputDeliveryId::from_raw(9762);
    let (recovery, _receipts) = claim_fixture(id);
    let original = recovery.completion_for(id).unwrap().unwrap();
    assert_eq!(
        recovery.claim_execution_for_held(id, &original),
        Ok(ExecutionClaim::Claimed)
    );
    // Exercise an original cell lost without a terminal answer, then the
    // real ledger admission path reusing its id. Neither is an old receipt.
    recovery.abort_enqueue(Some(id));
    assert_eq!(
        recovery.claim_execution_for_held(id, &original),
        Err(PrivateCompletionMismatch::Missing)
    );
    recovery
        .admit_typed(
            &button_to(SurfaceId::new(1, 1), id, 272, true),
            1,
            std::time::Instant::now(),
        )
        .unwrap();
    let replacement = recovery.completion_for(id).unwrap().unwrap();
    assert_eq!(
        recovery.claim_execution_for_held(id, &original),
        Err(PrivateCompletionMismatch::Replaced)
    );
    assert_eq!(
        recovery.claim_execution_for_held(id, &replacement),
        Ok(ExecutionClaim::Claimed)
    );
    recovery.resolve_claim_for_held(id, &original, true);
    assert_eq!(
        recovery.claim_execution_for_held(id, &replacement),
        Ok(ExecutionClaim::Contended)
    );
    assert!(
        !recovery
            .state
            .lock()
            .unwrap()
            .tickets
            .get(&id)
            .unwrap()
            .may_have_applied
    );
    recovery.resolve_claim_for_held(id, &replacement, false);
    assert_eq!(
        recovery.claim_execution_for_held(id, &replacement),
        Ok(ExecutionClaim::Claimed)
    );
    recovery.resolve_claim_for_held(id, &replacement, false);
    assert!(original.answer().is_none());
}

#[test]
fn prepared_producer_retains_the_original_completion_through_ordered_execution() {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId::from_raw(9763));
    let id = XAuthorityInputDeliveryId::from_raw(99763);
    fixture.ingress.submit(&fixture.keeper.lease(), button_to(fixture.surface, id, 272, false)).unwrap();
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut fixture.runner;
    let private = frontend.as_mut().unwrap();
    let original = private.broker.registry.input_recovery.completion_for(id).unwrap().unwrap();
    private.step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap()).unwrap();
    let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.first() else {
        panic!("the actual producer's no-held release completes");
    };
    let held = custody.input_completion().expect("captured before queue publication");
    assert_eq!(held.delivery, id);
    assert!(Arc::ptr_eq(&held.cell, &original));
    assert_eq!(custody.phase.get(), PrivateRequestPhase::Settled);
    assert!(matches!(private.deliver_one(&mut |_, _| Ok(())).unwrap(),
        PrivateDeliveryStep::Advanced { report: Some(_), .. }));
}

#[test]
fn prepared_producer_never_reclaims_a_missing_or_replaced_accepted_completion() {
    for (client, replace, expected) in [
        (9764, false, PrivateCompletionMismatch::Missing),
        (9765, true, PrivateCompletionMismatch::Replaced),
    ] {
        let mut fixture = prepared_ordered_fixture(XServerFrontendClientId::from_raw(client));
        let id = XAuthorityInputDeliveryId::from_raw(99000 + client);
        let route = button_to(fixture.surface, id, 272, true);
        fixture.ingress.submit(&fixture.keeper.lease(), route.clone()).unwrap();
        let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut fixture.runner;
        let private = frontend.as_mut().unwrap();
        let recovery = &private.broker.registry.input_recovery;
        let original = recovery.completion_for(id).unwrap().unwrap();
        // Fault injection removes the real ledger entry after queue
        // acceptance. A replacement is minted by actual recovery admission.
        recovery.abort_enqueue(Some(id));
        let replacement = replace.then(|| {
            recovery.admit_typed(&route, 1, std::time::Instant::now()).unwrap();
            recovery.completion_for(id).unwrap().unwrap()
        });
        private.step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap()).unwrap();
        let Some(PrivateOrderedItem::Refused { custody, refusal, .. }) = private.terminal.turn.first() else {
            panic!("missing original completion refuses before the common/native effect");
        };
        assert!(matches!(refusal, PrivateExecutionRefusal::CompletionMismatch(cause) if *cause == expected));
        assert!(Arc::ptr_eq(&custody.input_completion().unwrap().cell, &original));
        assert_eq!(custody.phase.get(), PrivateRequestPhase::Unused);
        assert!(custody.observe().unwrap().is_none());
        assert!(private.terminal.holds.is_empty());
        assert!(private.terminal.settling.is_empty());
        assert!(original.answer().is_none());
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
        assert_eq!(private.terminal.undelivered.len(), 1);
        if let Some(replacement) = replacement {
            assert!(replacement.answer().is_none());
            let recovery = &private.broker.registry.input_recovery;
            assert_eq!(recovery.claim_execution_for_held(id, &replacement), Ok(ExecutionClaim::Claimed));
            recovery.resolve_claim_for_held(id, &replacement, false);
            assert!(!recovery.state.lock().unwrap().tickets[&id].may_have_applied);
        }
    }
}
