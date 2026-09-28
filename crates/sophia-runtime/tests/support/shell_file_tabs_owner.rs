//! Native tab exchange through the SDK and production export. Actual tab
//! projection/commit and an independent client remain separate evidence.
use super::owner::{drive, observe, submit};
use super::*;

fn connected() -> (Fixture, ShellConnection) {
    let mut f = Fixture::new();
    f.start(ShellContentAdmissionPolicy::Unavailable);
    let c = f.connect(8, BASE | 4);
    (f, c)
}
fn snapshot(generation: u64) -> ShellTabSnapshot {
    ShellTabSnapshot {
        connection_epoch: EPOCH,
        generation,
        groups: [11, 12]
            .into_iter()
            .map(|slot| ShellTabGroup {
                slot,
                output: OutputId::from_raw(3),
                focused: slot == 11,
                selected_slot: None,
                entries: vec![],
            })
            .collect(),
    }
}
fn candidate(generation: u64) -> ShellTabCandidate {
    ShellTabCandidate {
        connection_epoch: EPOCH,
        snapshot_generation: generation,
        candidate_generation: generation,
        groups: vec![11, 12],
    }
}
fn publish(f: &mut Fixture, c: &mut ShellConnection, generation: u64) {
    let tx = TransactionId::from_raw(generation);
    let value = snapshot(generation);
    f.transport.publish_tabs(&mut f.epochs, tx, &value).unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: tx,
            record: ShellDescriptorRecord::Tabs(value)
        }
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
}
fn accept(f: &mut Fixture, c: &mut ShellConnection, generation: u64) {
    let value = candidate(generation);
    submit(
        f,
        c,
        generation,
        ShellDescriptorRecord::TabsCandidate(value.clone()),
    );
    assert_eq!(
        drive(f, c, |f, _| f
            .transport
            .poll_tabs_candidate(&mut f.epochs)
            .unwrap()),
        (TransactionId::from_raw(generation), value)
    );
}
fn outcome(generation: u64, kind: ShellV1CandidateOutcomeKind) -> ShellV1CandidateOutcome {
    ShellV1CandidateOutcome {
        connection_epoch: EPOCH,
        candidate_generation: generation,
        presentation_epoch: if kind == ShellV1CandidateOutcomeKind::Presented {
            17
        } else {
            0
        },
        kind,
    }
}
fn finish(
    f: &mut Fixture,
    c: &mut ShellConnection,
    generation: u64,
    kind: ShellV1CandidateOutcomeKind,
) {
    let value = outcome(generation, kind);
    let tx = TransactionId::from_raw(generation);
    f.transport
        .send_tabs_outcome(&mut f.epochs, tx, value)
        .unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: tx,
            record: ShellDescriptorRecord::DescriptorOutcome(value)
        }
    );
}

#[test]
fn tabs_require_preparation_then_exactly_one_terminal_outcome() {
    let (mut f, mut c) = connected();
    publish(&mut f, &mut c, 1);
    accept(&mut f, &mut c, 1);
    assert_eq!(
        f.transport.send_tabs_outcome(
            &mut f.epochs,
            TransactionId::from_raw(1),
            outcome(1, ShellV1CandidateOutcomeKind::Presented)
        ),
        Err(ShellTransportError::WrongCandidate)
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    finish(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Prepared);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        1
    );
    for (tx, generation, kind) in [
        (1, 1, ShellV1CandidateOutcomeKind::Prepared),
        (2, 1, ShellV1CandidateOutcomeKind::Presented),
        (1, 2, ShellV1CandidateOutcomeKind::Presented),
    ] {
        assert_eq!(
            f.transport.send_tabs_outcome(
                &mut f.epochs,
                TransactionId::from_raw(tx),
                outcome(generation, kind)
            ),
            Err(ShellTransportError::WrongCandidate)
        );
        assert_eq!(
            f.transport.content_accounting(&f.epochs).response_records,
            1
        );
    }
    finish(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Presented);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    assert_eq!(
        f.transport.send_tabs_outcome(
            &mut f.epochs,
            TransactionId::from_raw(1),
            outcome(1, ShellV1CandidateOutcomeKind::Superseded)
        ),
        Err(ShellTransportError::WrongCandidate)
    );
}

#[test]
fn a_new_snapshot_supersedes_unanswered_work_without_spending_its_credits_twice() {
    let (mut f, mut c) = connected();
    publish(&mut f, &mut c, 1);
    publish(&mut f, &mut c, 2);
    submit(
        &mut f,
        &mut c,
        1,
        ShellDescriptorRecord::TabsCandidate(candidate(1)),
    );
    assert_eq!(
        f.transport.poll_tabs_candidate(&mut f.epochs).unwrap(),
        None
    );
    assert_eq!(
        observe(&mut f, &mut c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(1),
            record: ShellDescriptorRecord::DescriptorOutcome(outcome(
                1,
                ShellV1CandidateOutcomeKind::Superseded
            ))
        }
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    accept(&mut f, &mut c, 2);
    assert_eq!(
        f.transport
            .publish_tabs(&mut f.epochs, TransactionId::from_raw(3), &snapshot(3)),
        Err(ShellTransportError::WrongCandidate)
    );
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Superseded);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    publish(&mut f, &mut c, 3);
}

#[test]
fn stale_snapshot_order_generation_and_reserved_high_bit_are_superseded() {
    for case in 0..4 {
        let (mut f, mut c) = connected();
        publish(&mut f, &mut c, 1);
        accept(&mut f, &mut c, 1);
        finish(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Rejected);
        publish(&mut f, &mut c, 2);
        let mut value = candidate(2);
        match case {
            0 => value.snapshot_generation = 1,
            1 => value.groups.reverse(),
            2 => value.candidate_generation = 1,
            3 => value.candidate_generation = 1 << 63,
            _ => unreachable!(),
        }
        let generation = value.candidate_generation;
        submit(
            &mut f,
            &mut c,
            2,
            ShellDescriptorRecord::TabsCandidate(value),
        );
        assert_eq!(
            f.transport.poll_tabs_candidate(&mut f.epochs).unwrap(),
            None
        );
        assert_eq!(
            observe(&mut f, &mut c),
            ShellFileDescriptorRecord {
                transaction: TransactionId::from_raw(2),
                record: ShellDescriptorRecord::DescriptorOutcome(outcome(
                    generation,
                    ShellV1CandidateOutcomeKind::Superseded
                ))
            }
        );
        assert_eq!(
            f.transport.content_accounting(&f.epochs).response_records,
            0
        );
        publish(&mut f, &mut c, 3);
        accept(&mut f, &mut c, 3);
    }
}

#[test]
fn tab_credits_share_content_capacity_and_disconnect_releases_them() {
    let mut f = Fixture::new();
    let mut limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: 7,
    });
    limits.max_control_records = 1;
    f.transport.reserve_content(&mut f.epochs, limits).unwrap();
    f.start(ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    });
    let _c = f.connect(8, BASE | 4 | (1 << 7));
    assert_eq!(
        f.transport
            .publish_tabs(&mut f.epochs, TransactionId::from_raw(1), &snapshot(1)),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(
        f.transport
            .content_accounting(&f.epochs)
            .snapshot_retained_bytes,
        0
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    let (mut f, mut c) = connected();
    publish(&mut f, &mut c, 1);
    f.transport.disconnect(&mut f.epochs).unwrap();
    assert!(f.transport.content_accounting(&f.epochs).quiescent());
}

#[test]
fn descriptor_requests_cannot_spend_credits_already_reserved_by_tabs() {
    let mut f = Fixture::new();
    let mut limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: 7,
    });
    limits.max_control_records = 3;
    f.transport.reserve_content(&mut f.epochs, limits).unwrap();
    f.start(ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    });
    let mut c = f.connect(8, BASE | 4 | (1 << 7));
    publish(&mut f, &mut c, 10);
    assert_eq!(
        f.transport.begin_candidate_request(
            &mut f.epochs,
            TransactionId::from_raw(1),
            &owner::snapshot(1)
        ),
        Err(ShellTransportError::ActivationQueueSaturated)
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    accept(&mut f, &mut c, 10);
    finish(&mut f, &mut c, 10, ShellV1CandidateOutcomeKind::Superseded);
    owner::request(&mut f, &mut c, 1);
}

#[test]
fn tab_activation_waits_for_presentation_and_acks_cannot_cross_owners() {
    let (mut f, mut c) = connected();
    owner::request(&mut f, &mut c, 1);
    owner::accept(&mut f, &mut c, 1);
    owner::outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Prepared);
    owner::outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Presented);
    publish(&mut f, &mut c, 10);
    accept(&mut f, &mut c, 10);
    let tab_tx = TransactionId::from_raw(21);
    let mut tab_event = owner::activation(10);
    tab_event.presentation_epoch = 17;
    tab_event.activation = 12;
    assert_eq!(
        f.transport
            .queue_tab_activation(&mut f.epochs, tab_tx, tab_event),
        Err(ShellTransportError::WrongActivation)
    );
    finish(&mut f, &mut c, 10, ShellV1CandidateOutcomeKind::Prepared);
    assert_eq!(
        f.transport
            .queue_tab_activation(&mut f.epochs, tab_tx, tab_event),
        Err(ShellTransportError::WrongActivation)
    );
    finish(&mut f, &mut c, 10, ShellV1CandidateOutcomeKind::Presented);
    f.transport
        .queue_tab_activation(&mut f.epochs, tab_tx, tab_event)
        .unwrap();
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::DescriptorActivation(tab_event)
    );
    let base_tx = TransactionId::from_raw(20);
    let base_event = owner::activation(1);
    f.transport
        .queue_activation(&mut f.epochs, base_tx, base_event)
        .unwrap();
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::DescriptorActivation(base_event)
    );
    let tab_ack = ShellV1ActivationAck {
        connection_epoch: EPOCH,
        activation: 12,
        disposition: ShellV1ActivationDisposition::Consumed,
    };
    let base_ack = ShellV1ActivationAck {
        activation: 11,
        ..tab_ack
    };
    submit(
        &mut f,
        &mut c,
        21,
        ShellDescriptorRecord::DescriptorActivationAck(ShellV1ActivationAck {
            activation: 99,
            ..tab_ack
        }),
    );
    assert_eq!(
        f.transport
            .poll_tab_activation_ack(&mut f.epochs, tab_tx)
            .unwrap(),
        None
    );
    assert_eq!(f.transport.descriptor_unmatched_acks(), 1);
    submit(
        &mut f,
        &mut c,
        21,
        ShellDescriptorRecord::DescriptorActivationAck(tab_ack),
    );
    assert_eq!(
        f.transport.poll_activation_ack(&mut f.epochs).unwrap(),
        None,
        "base poll keeps the tab ack"
    );
    assert_eq!(f.transport.descriptor_unmatched_acks(), 1);
    submit(
        &mut f,
        &mut c,
        20,
        ShellDescriptorRecord::DescriptorActivationAck(base_ack),
    );
    assert_eq!(
        f.transport
            .poll_tab_activation_ack(&mut f.epochs, TransactionId::from_raw(22))
            .unwrap(),
        None
    );
    assert_eq!(
        f.transport
            .poll_tab_activation_ack(&mut f.epochs, tab_tx)
            .unwrap(),
        Some(tab_ack)
    );
    assert_eq!(
        f.transport.poll_activation_ack(&mut f.epochs).unwrap(),
        Some(base_ack)
    );
    assert_eq!(f.transport.descriptor_unmatched_acks(), 1);
    publish(&mut f, &mut c, 11);
    assert_eq!(
        f.transport
            .queue_tab_activation(&mut f.epochs, tab_tx, tab_event),
        Err(ShellTransportError::WrongActivation),
        "new tab snapshot revokes the old interaction"
    );
}
