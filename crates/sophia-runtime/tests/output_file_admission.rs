use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::output_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

fn snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 4,
        primary_output: OutputId::from_raw(1),
        heads: vec![OutputHeadDescriptor {
            head: DisplayHeadId::from_raw(1),
            generation: 2,
            label: "Display 1".into(),
            connected: true,
            enabled: true,
            current_mode: Some(DisplayModeId::from_raw(10)),
            transforms: OutputTransformSet::ALL,
            vrr_capable: false,
            modes: vec![OutputModeDescriptor {
                mode: DisplayModeId::from_raw(10),
                pixel_size: Size {
                    width: 800,
                    height: 600,
                },
                refresh_millihz: 60_000,
                preferred: true,
            }],
        }],
        groups: vec![OutputLogicalGroupState {
            output: OutputId::from_raw(1),
            generation: 1,
            logical: Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(1),
                mapping: OutputHeadMapping::Exact,
            }],
        }],
    }
}

fn publication() -> OutputFilePublication {
    OutputFilePublication {
        topology_epoch: 4,
        qid_path: 90,
    }
}

fn record(kind: OutputFileKind, submission_id: u64, body: &[u8]) -> Vec<u8> {
    encode_output_file_record(
        OutputFileHeader {
            kind,
            connection_epoch: 7,
            submission_id,
            sequence: 0,
        },
        body,
    )
    .unwrap()
}

fn hello(capabilities: u64) -> Vec<u8> {
    record(
        OutputFileKind::Negotiate,
        1,
        &encode_output_file_negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities,
        }),
    )
}

fn proposal(submission: u64, transaction: u64, valid: bool) -> Vec<u8> {
    let facts = snapshot();
    let message = OutputV1Proposal {
        connection_epoch: 7,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: 4,
            intent: OutputTopologyIntent::Apply,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(1),
                head_generation: 2,
                mode: DisplayModeId::from_raw(10),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Disabled,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: OutputId::from_raw(1),
                logical: Rect {
                    x: if valid { 0 } else { -1 },
                    ..facts.groups[0].logical
                },
                members: facts.groups[0].members.clone(),
            }],
        },
    };
    record(
        OutputFileKind::Proposal,
        submission,
        &encode_output_file_proposal(TransactionId::from_raw(transaction), &message).unwrap(),
    )
}

fn submit(state: &mut OutputFileAdmission, bytes: &[u8]) -> Result<OutputFileSubmission, Errno> {
    state.submit(bytes, &snapshot(), publication())
}

fn ready(limits: OutputFileLimits) -> OutputFileAdmission {
    let mut state = OutputFileAdmission::new(7, limits).unwrap();
    assert!(matches!(
        submit(&mut state, &hello(3)),
        Ok(OutputFileSubmission::Negotiated(_))
    ));
    state
}

fn acknowledge_all(state: &mut OutputFileAdmission) {
    let sequence = state.journal().position().next_sequence - 1;
    state
        .acknowledge(OutputFileAck {
            connection_epoch: 7,
            sequence,
        })
        .unwrap();
}

fn outcome(epoch: u64) -> OutputV1Outcome {
    OutputV1Outcome {
        connection_epoch: epoch,
        topology_epoch: 4,
        kind: OutputV1OutcomeKind::Committed,
        reason: 0,
    }
}

#[test]
fn exact_replay_never_repeats_owner_delivery_or_journal_records() {
    let mut state = ready(OutputFileLimits::default());
    let before = state.journal().position();
    assert_eq!(
        submit(&mut state, &hello(3)),
        Ok(OutputFileSubmission::Replayed)
    );
    assert_eq!(state.journal().position(), before);
    assert_eq!(submit(&mut state, &hello(1)), Err(Errno::EINVAL));
    let candidate = proposal(2, 99, true);
    assert!(matches!(
        submit(&mut state, &candidate),
        Ok(OutputFileSubmission::Proposal { .. })
    ));
    let before = state.journal().position();
    assert_eq!(
        submit(&mut state, &candidate),
        Ok(OutputFileSubmission::Replayed)
    );
    assert_eq!(state.journal().position(), before);
    assert_eq!(state.connection().used_transaction_count(), 1);
    assert_eq!(submit(&mut state, &hello(3)), Err(Errno::ESTALE));
    assert_eq!(
        submit(&mut state, &proposal(2, 98, true)),
        Err(Errno::EINVAL)
    );
    let mut wrong_epoch = candidate;
    wrong_epoch[8..16].copy_from_slice(&6u64.to_le_bytes());
    assert_eq!(submit(&mut state, &wrong_epoch), Err(Errno::ESTALE));
}

#[test]
fn journal_pressure_spends_no_identity_for_valid_or_rejected_candidates() {
    for valid in [true, false] {
        let mut state = ready(OutputFileLimits {
            journal_records: 8,
            ..OutputFileLimits::default()
        });
        for _ in 0..5 {
            state.publish(publication()).unwrap();
        }
        let before = state.journal().position();
        let candidate = proposal(2, 99, valid);
        assert_eq!(submit(&mut state, &candidate), Err(Errno::EAGAIN));
        assert_eq!(state.journal().position(), before);
        assert_eq!(state.connection().used_transaction_count(), 0);
        assert_eq!(state.journal().reserved_outcomes(), 0);
        acknowledge_all(&mut state);
        let result = submit(&mut state, &candidate).unwrap();
        assert_eq!(
            matches!(result, OutputFileSubmission::Proposal { .. }),
            valid
        );
        assert_eq!(state.connection().used_transaction_count(), 1);
        assert_eq!(state.journal().reserved_outcomes(), usize::from(valid));
    }
}

#[test]
fn queued_replacement_and_terminal_credit_move_together() {
    let mut state = ready(OutputFileLimits::default());
    let start = state.journal().position().tail;
    acknowledge_all(&mut state);
    for (submission, transaction) in [(2, 99), (3, 3), (4, 17)] {
        submit(&mut state, &proposal(submission, transaction, true)).unwrap();
    }
    assert_eq!(state.journal().reserved_outcomes(), 2);
    let ReadOutcome::Ready(bytes) = state.journal().read(start, 4096).unwrap() else {
        panic!("events")
    };
    let mut records = bytes.as_slice();
    let mut stale = Vec::new();
    while !records.is_empty() {
        let size = u32::from_le_bytes(records[..4].try_into().unwrap()) as usize;
        let event = decode_output_file_record(&records[..size], OutputFileClass::Event).unwrap();
        if event.header.kind == OutputFileKind::Outcome {
            stale.push(decode_output_file_outcome(event.body, 7).unwrap());
        }
        records = &records[size..];
    }
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].0, TransactionId::from_raw(3));
    assert_eq!(stale[0].1.kind, OutputV1OutcomeKind::Stale);
    let before = state.journal().position();
    assert_eq!(
        state.settle(TransactionId::from_raw(3), outcome(7)),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        state.settle(TransactionId::from_raw(99), outcome(6)),
        Err(Errno::ESTALE)
    );
    assert_eq!(state.journal().position(), before);
    let promoted = state
        .settle(TransactionId::from_raw(99), outcome(7))
        .unwrap()
        .unwrap();
    assert_eq!(promoted.transaction, TransactionId::from_raw(17));
    assert_eq!(state.journal().reserved_outcomes(), 1);
    assert_eq!(state.settle(promoted.transaction, outcome(7)), Ok(None));
    assert_eq!(state.journal().reserved_outcomes(), 0);
}

#[test]
fn domain_history_exhaustion_preserves_an_accepted_terminal_outcome() {
    let mut state = ready(OutputFileLimits {
        max_domain_transactions: 1,
        ..OutputFileLimits::default()
    });
    submit(&mut state, &proposal(2, 99, true)).unwrap();
    acknowledge_all(&mut state);
    assert_eq!(
        submit(&mut state, &proposal(3, 3, true)),
        Err(Errno::ENOSPC)
    );
    assert_eq!(state.journal().reserved_outcomes(), 1);
    state
        .settle(TransactionId::from_raw(99), outcome(7))
        .unwrap();
    assert_eq!(state.journal().reserved_outcomes(), 0);
}

#[test]
fn refused_negotiation_keeps_its_terminal_records_and_exact_retry() {
    let mut state = OutputFileAdmission::new(7, OutputFileLimits::default()).unwrap();
    assert_eq!(
        submit(&mut state, &hello(0)),
        Ok(OutputFileSubmission::Refused(
            OutputFileRefusal::ObservationRequired
        ))
    );
    let before = state.journal().position();
    assert_eq!(before.records, 2);
    assert_eq!(
        submit(&mut state, &hello(0)),
        Ok(OutputFileSubmission::Replayed)
    );
    assert_eq!(
        submit(&mut state, &proposal(2, 1, true)),
        Err(Errno::EACCES)
    );
    assert_eq!(state.journal().position(), before);
}
