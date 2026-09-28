//! Output admission, replacement, settlement and epoch rules without socket codecs.
use sophia_protocol::output_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

fn head(head: u64, generation: u64, width: i32) -> OutputHeadDescriptor {
    OutputHeadDescriptor {
        head: DisplayHeadId::from_raw(head),
        generation,
        label: format!("DP-{head}"),
        connected: true,
        enabled: true,
        current_mode: Some(DisplayModeId::from_raw(head * 10)),
        transforms: OutputTransformSet::ALL,
        vrr_capable: false,
        modes: vec![OutputModeDescriptor {
            mode: DisplayModeId::from_raw(head * 10),
            pixel_size: Size {
                width,
                height: 1080,
            },
            refresh_millihz: 60_000,
            preferred: true,
        }],
    }
}

fn snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 4,
        primary_output: OutputId::from_raw(1),
        heads: vec![head(1, 2, 1920), head(2, 3, 1280)],
        groups: vec![OutputLogicalGroupState {
            output: OutputId::from_raw(1),
            generation: 6,
            logical: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            members: vec![
                OutputGroupMember {
                    head: DisplayHeadId::from_raw(1),
                    mapping: OutputHeadMapping::Exact,
                },
                OutputGroupMember {
                    head: DisplayHeadId::from_raw(2),
                    mapping: OutputHeadMapping::Fit,
                },
            ],
        }],
    }
}

fn proposal(epoch: u64, first_output: OutputId) -> OutputV1Proposal {
    OutputV1Proposal {
        connection_epoch: epoch,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: 4,
            intent: OutputTopologyIntent::Apply,
            primary_group_index: 0,
            heads: vec![
                OutputHeadTargetProposal {
                    head: DisplayHeadId::from_raw(1),
                    head_generation: 2,
                    mode: DisplayModeId::from_raw(10),
                    transform: OutputTransform::Normal,
                    vrr: OutputVrrPolicy::Disabled,
                },
                OutputHeadTargetProposal {
                    head: DisplayHeadId::from_raw(2),
                    head_generation: 3,
                    mode: DisplayModeId::from_raw(20),
                    transform: OutputTransform::Normal,
                    vrr: OutputVrrPolicy::Disabled,
                },
            ],
            groups: vec![OutputLogicalGroupProposal {
                output: first_output,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                members: vec![
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(1),
                        mapping: OutputHeadMapping::Exact,
                    },
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(2),
                        mapping: OutputHeadMapping::Fit,
                    },
                ],
            }],
        },
    }
}

fn negotiated(epoch: u64) -> OutputConnectionState {
    let mut state = OutputConnectionState::default();
    state.connect(epoch).unwrap();
    state
        .negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
        })
        .unwrap();
    state
}

#[test]
fn output_connection_grants_only_supported_capabilities() {
    let mut state = OutputConnectionState::default();
    state.connect(7).unwrap();
    let welcome = state
        .negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE
                | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE
                | (1 << 63),
        })
        .unwrap();
    assert_eq!(
        welcome.capabilities,
        SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE
    );
    assert_eq!(welcome.connection_epoch, 7);
    assert_eq!(welcome.max_heads, 16);
    assert_eq!(welcome.max_heads_per_group, 4);
}

#[test]
fn output_connection_requires_observation_before_configuration() {
    let mut state = OutputConnectionState::default();
    state.connect(7).unwrap();
    assert_eq!(
        state.negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
        }),
        Err(OutputTransferError::UnsupportedCapability)
    );
    assert_eq!(
        state.require_observe(),
        Err(OutputTransferError::NotNegotiated)
    );
}

#[test]
fn output_connection_keeps_one_active_and_one_replaceable_latest_candidate() {
    let snapshot = snapshot();
    let mut state = negotiated(7);
    assert_eq!(
        state.admit_proposal(
            TransactionId::from_raw(1),
            proposal(7, OutputId::from_raw(1)),
            &snapshot,
        ),
        Ok(OutputProposalAdmission::Active)
    );
    assert_eq!(
        state.admit_proposal(
            TransactionId::from_raw(2),
            proposal(7, OutputId::from_raw(1)),
            &snapshot,
        ),
        Ok(OutputProposalAdmission::Queued { replaced: None })
    );
    let admission = state
        .admit_proposal(
            TransactionId::from_raw(3),
            proposal(7, OutputId::from_raw(1)),
            &snapshot,
        )
        .unwrap();
    assert!(matches!(
        admission,
        OutputProposalAdmission::Queued { replaced: Some(old) }
            if old.transaction == TransactionId::from_raw(2)
    ));

    assert_eq!(
        state.active().unwrap().transaction,
        TransactionId::from_raw(1)
    );
    assert_eq!(
        state
            .settle_active(TransactionId::from_raw(1))
            .unwrap()
            .unwrap()
            .transaction,
        TransactionId::from_raw(3)
    );
}

#[test]
fn output_connection_rejects_stale_topology_and_reused_transaction() {
    let snapshot = snapshot();
    let mut state = negotiated(7);
    let transaction = TransactionId::from_raw(5);
    let mut stale = proposal(7, OutputId::from_raw(1));
    stale.candidate.base_topology_epoch = 3;
    assert_eq!(
        state.admit_proposal(transaction, stale, &snapshot),
        Err(OutputTransferError::InvalidCandidate(
            OutputTopologyCandidateError::StaleTopology
        ))
    );
    assert_eq!(
        state.admit_proposal(transaction, proposal(7, OutputId::from_raw(1)), &snapshot,),
        Err(OutputTransferError::ReusedTransaction)
    );
}

#[test]
fn output_disconnect_returns_every_unsettled_identity_and_keeps_last_good_external() {
    let snapshot = snapshot();
    let mut state = negotiated(7);
    for transaction in [8, 9] {
        state
            .admit_proposal(
                TransactionId::from_raw(transaction),
                proposal(7, OutputId::from_raw(1)),
                &snapshot,
            )
            .unwrap();
    }
    let abandoned = state.disconnect().unwrap();
    assert_eq!(
        abandoned
            .iter()
            .map(|proposal| proposal.transaction.raw())
            .collect::<Vec<_>>(),
        vec![8, 9]
    );
    assert!(state.active().is_none());
}

#[test]
fn native_file_negotiation_keeps_the_existing_capability_and_revision_rules() {
    for (minimum_revision, maximum_revision, capabilities, expected) in [
        (1, 1, 3 | (1 << 63), Ok(3)),
        (1, 1, 2, Err(OutputTransferError::UnsupportedCapability)),
        (2, 3, 3, Err(OutputTransferError::UnsupportedRevision)),
        (2, 1, 3, Err(OutputTransferError::UnsupportedRevision)),
    ] {
        let body = encode_output_file_negotiate(OutputV1ClientHello {
            minimum_revision,
            maximum_revision,
            capabilities,
        });
        let hello = decode_output_file_negotiate(&body).unwrap();
        let mut state = OutputConnectionState::default();
        state.connect(7).unwrap();
        assert_eq!(
            state.negotiate(hello).map(|welcome| welcome.capabilities),
            expected
        );
    }
}

#[test]
fn native_file_proposals_keep_semantic_refusals_and_consume_the_domain_identity() {
    let snapshot = snapshot();
    let mut state = negotiated(7);
    let base = proposal(7, OutputId::from_raw(1));
    for (index, expected) in [
        OutputTopologyCandidateError::StaleTopology,
        OutputTopologyCandidateError::InvalidPrimaryGroup,
        OutputTopologyCandidateError::InvalidGroup(0),
        OutputTopologyCandidateError::InvalidHead(DisplayHeadId::INVALID),
        OutputTopologyCandidateError::UnknownMode(DisplayHeadId::from_raw(1)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut invalid = base.clone();
        match index {
            0 => invalid.candidate.base_topology_epoch -= 1,
            1 => invalid.candidate.primary_group_index = u16::MAX,
            2 => invalid.candidate.groups[0].logical.x = -1,
            3 => invalid.candidate.heads[0].head = DisplayHeadId::INVALID,
            _ => invalid.candidate.heads[0].mode = DisplayModeId::from_raw(999),
        }
        let tx = TransactionId::from_raw(index as u64 + 1);
        let body = encode_output_file_proposal(tx, &invalid).unwrap();
        let (decoded_tx, decoded) = decode_output_file_proposal(&body, 7).unwrap();
        assert_eq!(
            state.admit_proposal(decoded_tx, decoded, &snapshot),
            Err(OutputTransferError::InvalidCandidate(expected))
        );
        assert!(state.active().is_none());
        assert_eq!(
            state.admit_proposal(tx, base.clone(), &snapshot),
            Err(OutputTransferError::ReusedTransaction)
        );
    }
    let tx = TransactionId::from_raw(8);
    let body = encode_output_file_proposal(tx, &base).unwrap();
    let (decoded_tx, decoded) = decode_output_file_proposal(&body, 7).unwrap();
    assert_eq!(
        state.admit_proposal(decoded_tx, decoded, &snapshot),
        Ok(OutputProposalAdmission::Active)
    );
}

#[test]
fn bounded_domain_history_keeps_arbitrary_ids_and_survives_settlement_until_reconnect() {
    let mut state = OutputConnectionState::with_transaction_limit(3);
    state.connect(7).unwrap();
    let hello = OutputV1ClientHello {
        minimum_revision: 1,
        maximum_revision: 1,
        capabilities: 3,
    };
    state.negotiate(hello).unwrap();
    let snapshot = snapshot();
    for id in [99, 2] {
        let tx = TransactionId::from_raw(id);
        state
            .admit_proposal(tx, proposal(7, OutputId::from_raw(1)), &snapshot)
            .unwrap();
        assert_eq!(state.settle_active(tx), Ok(None));
    }
    let mut invalid = proposal(7, OutputId::from_raw(1));
    invalid.candidate.base_topology_epoch -= 1;
    assert!(matches!(
        state.admit_proposal(TransactionId::from_raw(50), invalid, &snapshot),
        Err(OutputTransferError::InvalidCandidate(
            OutputTopologyCandidateError::StaleTopology
        ))
    ));
    assert_eq!(state.used_transaction_count(), 3);
    for id in [99, 2, 50] {
        assert_eq!(
            state.admit_proposal(
                TransactionId::from_raw(id),
                proposal(7, OutputId::from_raw(1)),
                &snapshot
            ),
            Err(OutputTransferError::ReusedTransaction)
        );
    }
    assert_eq!(
        state.admit_proposal(
            TransactionId::from_raw(51),
            proposal(7, OutputId::from_raw(1)),
            &snapshot
        ),
        Err(OutputTransferError::TransactionCapacityExceeded)
    );
    assert_eq!(state.used_transaction_count(), 3);
    assert!(state.active().is_none());
    assert!(state.disconnect().unwrap().is_empty());
    assert_eq!(
        state.connect(7),
        Err(OutputTransferError::InvalidConnectionEpoch)
    );
    state.connect(8).unwrap();
    state.negotiate(hello).unwrap();
    assert_eq!(state.used_transaction_count(), 0);
    assert_eq!(
        state.admit_proposal(
            TransactionId::from_raw(99),
            proposal(8, OutputId::from_raw(1)),
            &snapshot
        ),
        Ok(OutputProposalAdmission::Active)
    );
}

#[test]
fn history_exhaustion_preserves_both_existing_proposals_for_settlement() {
    let mut state = OutputConnectionState::with_transaction_limit(2);
    state.connect(7).unwrap();
    state
        .negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: 3,
        })
        .unwrap();
    let snapshot = snapshot();
    for id in [8, 3] {
        state
            .admit_proposal(
                TransactionId::from_raw(id),
                proposal(7, OutputId::from_raw(1)),
                &snapshot,
            )
            .unwrap();
    }
    assert_eq!(
        state.admit_proposal(
            TransactionId::from_raw(4),
            proposal(7, OutputId::from_raw(1)),
            &snapshot
        ),
        Err(OutputTransferError::TransactionCapacityExceeded)
    );
    assert_eq!(state.active().unwrap().transaction.raw(), 8);
    assert_eq!(
        state
            .settle_active(TransactionId::from_raw(8))
            .unwrap()
            .unwrap()
            .transaction
            .raw(),
        3
    );
    assert_eq!(state.settle_active(TransactionId::from_raw(3)), Ok(None));
    assert_eq!(state.used_transaction_count(), 2);
}

#[test]
fn journal_reservation_and_domain_admission_preserve_custody_on_refusal_and_replacement() {
    let mut state = OutputConnectionState::with_transaction_limit(3);
    state.connect(7).unwrap();
    state
        .negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: 3,
        })
        .unwrap();
    let snapshot = snapshot();
    let mut journal = OutputFileJournal::new(
        7,
        sophia_9p::journal::JournalBounds {
            records: 4,
            bytes: 216,
        },
    )
    .unwrap();
    for id in [1, 2] {
        let tx = TransactionId::from_raw(id);
        let prepared = journal.prepare_proposal(id, tx, None).unwrap();
        state
            .admit_proposal(tx, proposal(7, OutputId::from_raw(1)), &snapshot)
            .unwrap();
        prepared.commit();
    }
    let replacement = OutputFileReplacement {
        transaction: TransactionId::from_raw(2),
        topology_epoch: 4,
    };
    assert_eq!(
        journal
            .prepare_proposal(3, TransactionId::from_raw(3), Some(replacement))
            .err(),
        Some(sophia_9p::Errno::EAGAIN)
    );
    assert_eq!(state.used_transaction_count(), 2);
    journal
        .acknowledge(OutputFileAck {
            connection_epoch: 7,
            sequence: 2,
        })
        .unwrap();
    let prepared = journal
        .prepare_proposal(3, TransactionId::from_raw(3), Some(replacement))
        .unwrap();
    let admission = state
        .admit_proposal(
            TransactionId::from_raw(3),
            proposal(7, OutputId::from_raw(1)),
            &snapshot,
        )
        .unwrap();
    assert!(
        matches!(admission, OutputProposalAdmission::Queued { replaced: Some(old) } if old.transaction == replacement.transaction)
    );
    prepared.commit();
    for id in [1, 3] {
        let tx = TransactionId::from_raw(id);
        assert_eq!(state.active().unwrap().transaction, tx);
        journal
            .finish(
                tx,
                OutputV1Outcome {
                    connection_epoch: 7,
                    topology_epoch: 4,
                    kind: OutputV1OutcomeKind::Validated,
                    reason: 0,
                },
            )
            .unwrap();
        state.settle_active(tx).unwrap();
    }
    journal
        .acknowledge(OutputFileAck {
            connection_epoch: 7,
            sequence: 6,
        })
        .unwrap();
    let before = journal.position();
    let prepared = journal
        .prepare_proposal(4, TransactionId::from_raw(4), None)
        .unwrap();
    assert_eq!(
        state.admit_proposal(
            TransactionId::from_raw(4),
            proposal(7, OutputId::from_raw(1)),
            &snapshot
        ),
        Err(OutputTransferError::TransactionCapacityExceeded)
    );
    drop(prepared);
    assert_eq!(journal.position(), before);
    assert_eq!(journal.reserved_outcomes(), 0);
    assert_eq!(state.used_transaction_count(), 3);
}
