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
