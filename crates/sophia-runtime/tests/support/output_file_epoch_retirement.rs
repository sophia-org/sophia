//! Neutral owner-error and late-settlement assertions formerly on the socket worker.
use super::*;

fn submit(worker: &mut Worker, epoch: u64, submission: u64, kind: OutputFileKind, body: &[u8]) {
    let bytes = encode_output_file_record(
        OutputFileHeader {
            kind,
            connection_epoch: epoch,
            submission_id: submission,
            sequence: 0,
        },
        body,
    )
    .unwrap();
    let export = worker.transport.export_mut().unwrap();
    let mut staging = export.open(&Node::Transaction, OpenFlags(2)).unwrap();
    export
        .write(&Node::Transaction, &mut staging, 0, &bytes)
        .unwrap();
    let mut control = export.open(&Node::Submit, OpenFlags(1)).unwrap();
    export
        .write(
            &Node::Submit,
            &mut control,
            0,
            &encode_output_file_submit(OutputFileSubmit {
                connection_epoch: epoch,
                submission_id: submission,
                candidate_bytes: bytes.len() as u32,
            })
            .unwrap(),
        )
        .unwrap();
    export.release(Node::Transaction, Some(staging));
    assert!(
        export.take_delivery().is_some(),
        "owner consumes each bounded receipt"
    );
    // A receipt retains the staging slot until the peer has read and acked it.
    let mut events = export.open(&Node::Events, OpenFlags(0)).unwrap();
    let offset = if kind == OutputFileKind::Negotiate {
        0
    } else {
        160
    };
    let length = export.describe(&Node::Events, None).size - offset;
    export
        .read(&Node::Events, &mut events, offset, length as u32)
        .unwrap();
    if kind == OutputFileKind::Negotiate {
        let mut topology = export.open(&Node::Topology, OpenFlags(0)).unwrap();
        let length = export.describe(&Node::Topology, Some(&topology)).size;
        export
            .read(&Node::Topology, &mut topology, 0, length as u32)
            .unwrap();
        export.release(Node::Topology, Some(topology));
    }
    let mut ack = export.open(&Node::Ack, OpenFlags(1)).unwrap();
    export
        .write(
            &Node::Ack,
            &mut ack,
            0,
            &encode_output_file_ack(OutputFileAck {
                connection_epoch: epoch,
                sequence: if kind == OutputFileKind::Negotiate {
                    3
                } else {
                    4
                },
            })
            .unwrap(),
        )
        .unwrap();
}

fn active(worker: &mut Worker, epoch: u64) -> UnixStream {
    let peer = UnixStream::connect(worker.transport.socket_path()).unwrap();
    assert!(worker.transport.poll_accept(&worker.snapshot).unwrap());
    submit(
        worker,
        epoch,
        1,
        OutputFileKind::Negotiate,
        &encode_output_file_negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
        }),
    );
    let facts = &worker.snapshot;
    let proposal = OutputV1Proposal {
        connection_epoch: epoch,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: facts.topology_epoch,
            intent: OutputTopologyIntent::ValidateOnly,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: facts.heads[0].head,
                head_generation: 1,
                mode: facts.heads[0].current_mode.unwrap(),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Disabled,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: facts.groups[0].output,
                logical: facts.groups[0].logical,
                members: facts.groups[0].members.clone(),
            }],
        },
    };
    submit(
        worker,
        epoch,
        2,
        OutputFileKind::Proposal,
        &encode_output_file_proposal(TransactionId::from_raw(90), &proposal).unwrap(),
    );
    assert_eq!(
        worker
            .transport
            .export()
            .unwrap()
            .admission()
            .connection()
            .active()
            .unwrap()
            .transaction
            .raw(),
        90
    );
    peer
}

fn settle(epoch: u64) -> OutputFileServiceCommand {
    OutputFileServiceCommand::Settle {
        transaction: TransactionId::from_raw(90),
        outcome: OutputV1Outcome {
            connection_epoch: epoch,
            topology_epoch: 4,
            kind: OutputV1OutcomeKind::Validated,
            reason: 0,
        },
    }
}

#[test]
fn late_settlement_cannot_settle_a_replacements_reused_transaction() {
    let mut worker = worker_with_limits("late-settlement", OutputFileLimits::default());
    let old = active(&mut worker, 7);
    let abandoned = worker.transport.disconnect().unwrap();
    assert_eq!(abandoned.len(), 1);
    drop(old);
    let _replacement = active(&mut worker, 8);
    worker.command(settle(7)).unwrap();
    let connection = worker.transport.export().unwrap().admission().connection();
    assert_eq!(connection.connection_epoch(), 8);
    assert_eq!(connection.active().unwrap().transaction.raw(), 90);
    assert!(worker.pending.is_empty());
    worker.command(settle(8)).unwrap();
    assert!(
        worker
            .transport
            .export()
            .unwrap()
            .admission()
            .connection()
            .active()
            .is_none()
    );
}

#[test]
fn zero_and_future_settlement_epochs_are_owner_errors_without_retiring_the_peer() {
    for epoch in [0, 8] {
        let mut worker = worker_with_limits(
            &format!("invalid-settlement-{epoch}"),
            OutputFileLimits::default(),
        );
        let _peer = active(&mut worker, 7);
        assert!(worker.command(settle(epoch)).is_err());
        assert!(!worker.transport.export().unwrap().is_revoked());
        assert_eq!(
            worker
                .transport
                .export()
                .unwrap()
                .admission()
                .connection()
                .active()
                .unwrap()
                .transaction
                .raw(),
            90
        );
        assert!(worker.pending.is_empty());
        worker.command(settle(7)).unwrap();
        assert!(
            worker
                .transport
                .export()
                .unwrap()
                .admission()
                .connection()
                .active()
                .is_none()
        );
    }
}
