//! Production file export plus the public SDK. These tests drive transport
//! ownership; Engine work-area commits and an independent encoder are separate.
use super::*;
use sophia_shell_client::{Custody, DescriptorObservation};

fn action() -> ToplevelActionCapabilityRef {
    ToplevelActionCapabilityRef {
        token: 3,
        issuer_epoch: 4,
        issuer_revocation_epoch: 5,
        recipient_epoch: EPOCH,
        target_slot: 1,
        target_generation: 6,
    }
}

pub(super) fn snapshot(generation: u64) -> ShellV1DescriptorSnapshot {
    ShellV1DescriptorSnapshot {
        connection_epoch: EPOCH,
        snapshot_generation: generation,
        output: OutputId::from_raw(8),
        output_generation: 9,
        broker_epoch: 4,
        broker_revocation_epoch: 5,
        descriptors: vec![ShellV1Descriptor {
            slot: 1,
            generation: 6,
            label: None,
            trust_level: TrustLevel::Trusted,
            attention: AttentionState::None,
            action: action(),
        }],
    }
}

fn candidate(generation: u64) -> ShellV1Candidate {
    ShellV1Candidate {
        connection_epoch: EPOCH,
        snapshot_generation: generation,
        candidate_generation: generation,
        output: OutputId::from_raw(8),
        visible: true,
        selected_slot: Some(1),
        reservation: None,
        entries: vec![ShellV1CandidateEntry {
            slot: 1,
            generation: 6,
        }],
    }
}

pub(super) fn activation(generation: u64) -> ShellV1Activation {
    ShellV1Activation {
        connection_epoch: EPOCH,
        candidate_generation: generation,
        presentation_epoch: 10,
        activation: 11,
        action: action(),
    }
}

fn connected() -> (Fixture, ShellConnection) {
    let mut f = Fixture::new();
    f.start(ShellContentAdmissionPolicy::Unavailable);
    let c = f.connect(8, BASE);
    (f, c)
}

pub(super) fn drive<T>(
    f: &mut Fixture,
    c: &mut ShellConnection,
    mut poll: impl FnMut(&mut Fixture, &mut ShellConnection) -> Option<T>,
) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        f.transport.poll_io(&mut f.epochs).unwrap();
        c.poll_io().unwrap();
        if let Some(value) = poll(f, c) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "descriptor owner exchange stalled"
        );
        std::thread::yield_now();
    }
}

pub(super) fn observe(f: &mut Fixture, c: &mut ShellConnection) -> ShellFileDescriptorRecord {
    match drive(f, c, |_, c| c.take_descriptor_observation().unwrap()) {
        DescriptorObservation::Record(value) => value,
        other => panic!("unexpected {other:?}"),
    }
}

pub(super) fn request(f: &mut Fixture, c: &mut ShellConnection, generation: u64) {
    let value = snapshot(generation);
    f.transport
        .begin_candidate_request(&mut f.epochs, TransactionId::from_raw(generation), &value)
        .unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(generation),
            record: ShellDescriptorRecord::Descriptors(value)
        }
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
}

pub(super) fn submit(
    f: &mut Fixture,
    c: &mut ShellConnection,
    transaction: u64,
    record: ShellDescriptorRecord,
) {
    let ticket = c
        .enqueue_descriptor_tracked(&ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(transaction),
            record,
        })
        .unwrap()
        .first;
    drive(f, c, |_, c| {
        (c.custody(ticket) == Some(Custody::Submitted)).then_some(())
    });
}

pub(super) fn accept(f: &mut Fixture, c: &mut ShellConnection, generation: u64) {
    let value = candidate(generation);
    submit(
        f,
        c,
        generation,
        ShellDescriptorRecord::DescriptorCandidate(value.clone()),
    );
    assert_eq!(
        drive(f, c, |f, _| f
            .transport
            .poll_candidate(&mut f.epochs)
            .unwrap()),
        value
    );
}

pub(super) fn outcome(
    f: &mut Fixture,
    c: &mut ShellConnection,
    generation: u64,
    kind: ShellV1CandidateOutcomeKind,
) {
    let value = ShellV1CandidateOutcome {
        connection_epoch: EPOCH,
        candidate_generation: generation,
        presentation_epoch: if kind == ShellV1CandidateOutcomeKind::Presented {
            10
        } else {
            0
        },
        kind,
    };
    f.transport
        .send_candidate_outcome(&mut f.epochs, TransactionId::from_raw(generation), value)
        .unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(generation),
            record: ShellDescriptorRecord::DescriptorOutcome(value)
        }
    );
}

#[test]
fn prepared_does_not_authorize_activation_and_presented_does() {
    let (mut f, mut c) = connected();
    request(&mut f, &mut c, 1);
    accept(&mut f, &mut c, 1);
    let tx = TransactionId::from_raw(20);
    let value = activation(1);
    assert_eq!(
        f.transport.queue_activation(&mut f.epochs, tx, value),
        Err(ShellTransportError::WrongActivation)
    );
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Prepared);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        1
    );
    assert_eq!(
        f.transport.queue_activation(&mut f.epochs, tx, value),
        Err(ShellTransportError::WrongActivation)
    );
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Presented);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    f.transport
        .queue_activation(&mut f.epochs, tx, value)
        .unwrap();
    assert_eq!(
        observe(&mut f, &mut c),
        ShellFileDescriptorRecord {
            transaction: tx,
            record: ShellDescriptorRecord::DescriptorActivation(value)
        }
    );
    let ack = ShellV1ActivationAck {
        connection_epoch: EPOCH,
        activation: 11,
        disposition: ShellV1ActivationDisposition::Consumed,
    };
    // A stale ack is consumed once, without discharging the exact live one.
    submit(
        &mut f,
        &mut c,
        19,
        ShellDescriptorRecord::DescriptorActivationAck(ack),
    );
    assert_eq!(
        f.transport.poll_activation_ack(&mut f.epochs).unwrap(),
        None
    );
    assert_eq!(f.transport.descriptor_unmatched_acks(), 1);
    submit(
        &mut f,
        &mut c,
        20,
        ShellDescriptorRecord::DescriptorActivationAck(ack),
    );
    assert_eq!(
        drive(&mut f, &mut c, |f, _| f
            .transport
            .poll_activation_ack(&mut f.epochs)
            .unwrap()),
        ack
    );
    assert_eq!(f.transport.descriptor_unmatched_acks(), 1);
}

#[test]
fn rejection_releases_both_credits_and_stale_transaction_preserves_request() {
    let (mut f, mut c) = connected();
    request(&mut f, &mut c, 2);
    submit(
        &mut f,
        &mut c,
        1,
        ShellDescriptorRecord::DescriptorCandidate(candidate(1)),
    );
    assert_eq!(f.transport.poll_candidate(&mut f.epochs).unwrap(), None);
    let rejected = observe(&mut f, &mut c);
    assert_eq!(rejected.transaction, TransactionId::from_raw(1));
    assert!(matches!(
        rejected.record,
        ShellDescriptorRecord::DescriptorOutcome(ShellV1CandidateOutcome {
            kind: ShellV1CandidateOutcomeKind::Rejected,
            ..
        })
    ));
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    accept(&mut f, &mut c, 2);
    outcome(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Rejected);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    assert_eq!(
        f.transport
            .queue_activation(&mut f.epochs, TransactionId::from_raw(20), activation(2)),
        Err(ShellTransportError::WrongActivation)
    );
    request(&mut f, &mut c, 3);
    // Exact transaction, wrong snapshot: terminal rejection ends this request.
    submit(
        &mut f,
        &mut c,
        3,
        ShellDescriptorRecord::DescriptorCandidate(candidate(2)),
    );
    assert_eq!(f.transport.poll_candidate(&mut f.epochs).unwrap(), None);
    assert!(matches!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::DescriptorOutcome(ShellV1CandidateOutcome {
            kind: ShellV1CandidateOutcomeKind::Rejected,
            ..
        })
    ));
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    request(&mut f, &mut c, 4);
    accept(&mut f, &mut c, 4);
    outcome(&mut f, &mut c, 4, ShellV1CandidateOutcomeKind::Prepared);
    outcome(&mut f, &mut c, 4, ShellV1CandidateOutcomeKind::Superseded);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
}

#[test]
fn disconnect_releases_an_unanswered_descriptor_request() {
    let (mut f, mut c) = connected();
    request(&mut f, &mut c, 1);
    f.transport.disconnect(&mut f.epochs).unwrap();
    assert!(
        f.transport
            .collect_content_accounting(&mut f.epochs)
            .quiescent()
    );
}

#[test]
fn replacement_preserves_the_presented_generation_until_its_terminal_outcome() {
    let (mut f, mut c) = connected();
    request(&mut f, &mut c, 1);
    accept(&mut f, &mut c, 1);
    // Reject an out-of-order outcome without consuming either response credit.
    assert_eq!(
        f.transport.send_candidate_outcome(
            &mut f.epochs,
            TransactionId::from_raw(1),
            ShellV1CandidateOutcome {
                connection_epoch: EPOCH,
                candidate_generation: 1,
                presentation_epoch: 10,
                kind: ShellV1CandidateOutcomeKind::Presented,
            }
        ),
        Err(ShellTransportError::WrongCandidate)
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Prepared);
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Presented);
    request(&mut f, &mut c, 2);
    let mut withdrawal = candidate(2);
    withdrawal.visible = false;
    withdrawal.selected_slot = None;
    withdrawal.entries.clear();
    submit(
        &mut f,
        &mut c,
        2,
        ShellDescriptorRecord::DescriptorCandidate(withdrawal.clone()),
    );
    assert_eq!(
        drive(&mut f, &mut c, |f, _| f
            .transport
            .poll_candidate(&mut f.epochs)
            .unwrap()),
        withdrawal
    );
    outcome(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Prepared);
    let tx = TransactionId::from_raw(20);
    f.transport
        .queue_activation(&mut f.epochs, tx, activation(1))
        .unwrap();
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::DescriptorActivation(activation(1))
    );
    assert_eq!(
        f.transport
            .queue_activation(&mut f.epochs, tx, activation(2)),
        Err(ShellTransportError::WrongActivation)
    );
    outcome(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Presented);
    for generation in [1, 2] {
        assert_eq!(
            f.transport
                .queue_activation(&mut f.epochs, tx, activation(generation)),
            Err(ShellTransportError::WrongActivation)
        );
    }
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
}

#[test]
fn combined_content_accounts_for_descriptor_obligations_in_the_same_budget() {
    let mut f = Fixture::new();
    let mut limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: 7,
    });
    limits.max_control_records = 2;
    f.transport.reserve_content(&mut f.epochs, limits).unwrap();
    f.start(ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    });
    let mut c = f.connect(8, BASE | (1 << 7));
    request(&mut f, &mut c, 1);
    accept(&mut f, &mut c, 1);
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Prepared);
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Presented);
    request(&mut f, &mut c, 2);
    // Both control slots belong to the replacement; bulk cannot steal them.
    assert_eq!(
        f.transport
            .queue_activation(&mut f.epochs, TransactionId::from_raw(20), activation(1)),
        Err(ShellTransportError::ActivationQueueSaturated)
    );
    accept(&mut f, &mut c, 2);
    outcome(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Rejected);
    f.transport
        .queue_activation(&mut f.epochs, TransactionId::from_raw(20), activation(1))
        .unwrap();
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::DescriptorActivation(activation(1))
    );
}

#[test]
fn semantic_refusals_end_the_exact_request_without_authorizing_an_action() {
    for case in 0..3 {
        let (mut f, mut c) = connected();
        request(&mut f, &mut c, 1);
        accept(&mut f, &mut c, 1);
        outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Rejected);
        request(&mut f, &mut c, 2);
        let mut value = candidate(2);
        match case {
            0 => value.candidate_generation = 1,
            1 => value.output = OutputId::from_raw(9),
            2 => value.entries[0].generation = 7,
            _ => unreachable!(),
        }
        let generation = value.candidate_generation;
        submit(
            &mut f,
            &mut c,
            2,
            ShellDescriptorRecord::DescriptorCandidate(value),
        );
        assert_eq!(f.transport.poll_candidate(&mut f.epochs).unwrap(), None);
        assert_eq!(
            observe(&mut f, &mut c),
            ShellFileDescriptorRecord {
                transaction: TransactionId::from_raw(2),
                record: ShellDescriptorRecord::DescriptorOutcome(ShellV1CandidateOutcome {
                    connection_epoch: EPOCH,
                    candidate_generation: generation,
                    presentation_epoch: 0,
                    kind: ShellV1CandidateOutcomeKind::Rejected,
                }),
            }
        );
        assert_eq!(
            f.transport.content_accounting(&f.epochs).response_records,
            0
        );
        assert_eq!(
            f.transport
                .queue_activation(&mut f.epochs, TransactionId::from_raw(20), activation(2)),
            Err(ShellTransportError::WrongActivation)
        );
        request(&mut f, &mut c, 3);
        accept(&mut f, &mut c, 3);
    }
}
