//! Output admission, replacement, settlement and epoch rules without wire codecs.
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
