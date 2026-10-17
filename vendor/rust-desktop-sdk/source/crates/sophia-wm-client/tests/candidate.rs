use sophia_wm_client::{
    Candidate,
    protocol::{wm_files::*, *},
};

fn dirty() -> Candidate {
    Candidate::Dirty(PolicyDirtyRequest {
        connection_epoch: 7,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(9)],
    })
}

#[test]
fn owned_bytes_survive_policy_mutation_and_match_literal_record() {
    let mut candidate = dirty();
    let encoded = candidate
        .encode(7, 11, SOPHIA_WM_CAPABILITY_POLICY_DIRTY)
        .unwrap();
    if let Candidate::Dirty(value) = &mut candidate {
        value.policy_generation = 4;
        value.affected_outputs.clear();
    }
    let mut literal = Vec::new();
    // Independent file-envelope/body layout, not a second call to the encoder.
    literal.extend(56u32.to_le_bytes());
    literal.extend(1u16.to_le_bytes());
    literal.extend(261u16.to_le_bytes());
    literal.extend(7u64.to_le_bytes());
    literal.extend(11u64.to_le_bytes());
    literal.extend(0u64.to_le_bytes());
    literal.extend(3u64.to_le_bytes());
    literal.extend(1u16.to_le_bytes());
    literal.extend([0; 6]);
    literal.extend(9u64.to_le_bytes());
    assert_eq!(encoded.bytes(), literal);
    assert_eq!(encoded.header().sequence, 0);
    assert_eq!(encoded.domain_transaction(), None);
}

#[test]
fn invalid_identity_and_missing_capability_are_refused() {
    assert!(
        dirty()
            .encode(0, 1, SOPHIA_WM_CAPABILITY_POLICY_DIRTY)
            .is_err()
    );
    assert!(
        dirty()
            .encode(7, 0, SOPHIA_WM_CAPABILITY_POLICY_DIRTY)
            .is_err()
    );
    assert!(
        dirty()
            .encode(8, 1, SOPHIA_WM_CAPABILITY_POLICY_DIRTY)
            .is_err()
    );
    assert!(dirty().encode(7, 1, 0).is_err());
}

#[test]
fn profile_transactions_are_echoes_not_domain_allocations() {
    let value = PolicyProfileCompletion {
        transaction: TransactionId::from_raw(19),
        identity: PolicyProfileIdentity::new(7, 4, [3; 32]).unwrap(),
        outcome: PolicyProfileOutcome::Accepted,
    };
    for candidate in [
        Candidate::ProfilePrepared(value),
        Candidate::ProfileActive(value),
        Candidate::ProfileRolledBack(value),
    ] {
        let encoded = candidate
            .encode(7, 11, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION)
            .unwrap();
        assert_eq!(encoded.domain_transaction(), None);
        assert_eq!(
            decode_wm_file_profile_completion(
                encoded.bytes(),
                candidate.kind(),
                SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION
            )
            .unwrap(),
            value
        );
    }
}

#[test]
fn session_operation_keeps_domain_and_submission_ids_distinct() {
    let value = WmFileSessionOperation {
        transaction: TransactionId::from_raw(19),
        request: PolicySessionOperationRequest {
            connection_epoch: 7,
            request_id: 31,
            operation: 1,
            target: None,
        },
    };
    let encoded = Candidate::SessionOperation(value)
        .encode(7, 11, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)
        .unwrap();
    assert_eq!(encoded.header().submission_id, 11);
    assert_eq!(
        encoded.domain_transaction(),
        Some(TransactionId::from_raw(19))
    );
    assert_eq!(
        decode_wm_file_session_operation(encoded.bytes(), SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)
            .unwrap(),
        value
    );
}

#[test]
fn configuration_and_projection_preserve_domain_values() {
    let config = WmFileConfiguration {
        transaction: TransactionId::from_raw(19),
        configuration: PolicyConfiguration {
            connection_epoch: 7,
            generation: 3,
            actions: vec![],
            chrome: WmChromePolicy::default(),
            action_lifecycles: vec![],
        },
    };
    let encoded = Candidate::Configuration(config.clone())
        .encode(
            7,
            11,
            SOPHIA_WM_CAPABILITY_CONFIGURATION | SOPHIA_WM_CAPABILITY_CHROME,
        )
        .unwrap();
    assert_eq!(encoded.domain_transaction(), Some(config.transaction));
    assert_eq!(
        decode_wm_file_configuration(
            encoded.bytes(),
            SOPHIA_WM_CAPABILITY_CONFIGURATION | SOPHIA_WM_CAPABILITY_CHROME
        )
        .unwrap(),
        config
    );
    let projection = PolicyProjectionProposal {
        transaction: TransactionId::from_raw(20),
        connection_epoch: 7,
        request_id: 5,
        base_generation: 3,
        active_output: OutputId::from_raw(9),
        outputs: vec![PolicyOutputProjection {
            output: OutputId::from_raw(9),
            placements: vec![],
            focus: None,
        }],
        presentation: None,
        output_launch_contexts: vec![],
        launch_contexts: vec![],
        translation_groups: vec![],
        tab_groups: vec![],
        indicators: vec![],
        output_statuses: vec![],
    };
    let encoded = Candidate::Projection(Box::new(projection.clone()))
        .encode(7, 12, 0)
        .unwrap();
    assert_eq!(encoded.domain_transaction(), Some(projection.transaction));
    assert_eq!(
        decode_wm_file_projection(encoded.bytes(), 0).unwrap(),
        projection
    );
}
