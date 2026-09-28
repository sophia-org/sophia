//! Descriptor launcher ownership over the actual SDK and file export.
//! These checks do not launch applications or grant Session admission.
use super::owner::{drive, observe, submit};
use super::*;
use sophia_shell_client::DescriptorObservation;

fn connected() -> (Fixture, ShellConnection) {
    let mut f = Fixture::new();
    f.start(ShellContentAdmissionPolicy::Unavailable);
    let c = f.connect(8, BASE | (1 << 5) | (1 << 6));
    (f, c)
}
fn catalog() -> ShellApplicationCatalog {
    ShellApplicationCatalog {
        connection_epoch: EPOCH,
        generation: 1,
        entries: vec![ShellApplicationDescriptor {
            slot: 1,
            available: true,
            label: "Editor".into(),
            keywords: "text".into(),
        }],
    }
}
fn request(generation: u64) -> ShellLauncherRequest {
    ShellLauncherRequest {
        connection_epoch: EPOCH,
        catalog_generation: 1,
        request_generation: generation,
        output: OutputId::from_raw(3),
        output_generation: 4,
        presentation_epoch: 0,
        operation: ShellLauncherOperation::Open,
        query: String::new(),
    }
}
fn candidate(generation: u64) -> ShellLauncherCandidate {
    ShellLauncherCandidate {
        connection_epoch: EPOCH,
        catalog_generation: 1,
        request_generation: generation,
        candidate_generation: generation,
        output: OutputId::from_raw(3),
        visible: true,
        selected: 1,
        entries: vec![1],
        font_size: 12,
        colors: [0xff000000; 4],
    }
}
fn outcome(generation: u64, kind: ShellV1CandidateOutcomeKind) -> ShellLauncherOutcome {
    ShellLauncherOutcome {
        connection_epoch: EPOCH,
        request_generation: generation,
        candidate_generation: generation,
        presentation_epoch: if kind == ShellV1CandidateOutcomeKind::Presented {
            9
        } else {
            0
        },
        kind,
    }
}
fn activation(generation: u64) -> ShellLauncherActivation {
    ShellLauncherActivation {
        connection_epoch: EPOCH,
        catalog_generation: 1,
        request_generation: generation,
        candidate_generation: generation,
        presentation_epoch: 9,
        activation: 5,
        slot: 1,
    }
}
fn publish(f: &mut Fixture, c: &mut ShellConnection) {
    f.transport
        .publish_launcher_catalog(&f.epochs, TransactionId::from_raw(1), &catalog())
        .unwrap();
    match drive(f, c, |_, c| c.take_descriptor_observation().unwrap()) {
        DescriptorObservation::Catalog(tx, value) => {
            assert_eq!(tx, TransactionId::from_raw(1));
            assert_eq!(value, catalog());
        }
        other => panic!("expected catalog, got {other:?}"),
    }
}
fn begin(f: &mut Fixture, c: &mut ShellConnection, generation: u64) {
    f.transport
        .begin_launcher_request(
            &mut f.epochs,
            TransactionId::from_raw(generation),
            &request(generation),
        )
        .unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(generation),
            record: ShellDescriptorRecord::LauncherRequest(request(generation))
        }
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
}
fn accept(f: &mut Fixture, c: &mut ShellConnection, generation: u64) {
    submit(
        f,
        c,
        generation,
        ShellDescriptorRecord::LauncherCandidate(candidate(generation)),
    );
    let event = drive(f, c, |f, _| {
        f.transport.poll_launcher_candidate(&mut f.epochs).unwrap()
    });
    let ShellLauncherCandidateEvent::Candidate(tx, value) = event else {
        panic!("valid launcher refused")
    };
    assert_eq!(tx, TransactionId::from_raw(generation));
    assert_eq!(value, candidate(generation));
}
fn finish(
    f: &mut Fixture,
    c: &mut ShellConnection,
    generation: u64,
    kind: ShellV1CandidateOutcomeKind,
) {
    f.transport
        .send_launcher_outcome(
            &mut f.epochs,
            TransactionId::from_raw(generation),
            outcome(generation, kind),
        )
        .unwrap();
    assert_eq!(
        observe(f, c).record,
        ShellDescriptorRecord::LauncherOutcome(outcome(generation, kind))
    );
}
fn present(f: &mut Fixture, c: &mut ShellConnection) {
    publish(f, c);
    begin(f, c, 2);
    accept(f, c, 2);
    finish(f, c, 2, ShellV1CandidateOutcomeKind::Prepared);
    finish(f, c, 2, ShellV1CandidateOutcomeKind::Presented);
}
fn queue(f: &mut Fixture, c: &mut ShellConnection) {
    f.transport
        .queue_launcher_activation(&mut f.epochs, TransactionId::from_raw(5), activation(2))
        .unwrap();
    assert_eq!(
        observe(f, c).record,
        ShellDescriptorRecord::LauncherActivation(activation(2))
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        1
    );
}
fn ack(f: &mut Fixture, c: &mut ShellConnection, consumed: bool) {
    let ack = ShellLauncherActivationAck {
        activation: activation(2),
        consumed,
    };
    submit(f, c, 5, ShellDescriptorRecord::LauncherActivationAck(ack));
    assert_eq!(
        drive(f, c, |f, _| f
            .transport
            .poll_launcher_activation_ack(&mut f.epochs)
            .unwrap()),
        (TransactionId::from_raw(5), ack)
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        1
    );
}
fn launch(f: &mut Fixture, status: ShellLaunchStatus) -> Result<(), ShellTransportError> {
    f.transport.send_launch_outcome(
        &mut f.epochs,
        TransactionId::from_raw(5),
        ShellLaunchOutcome {
            activation: activation(2),
            status,
        },
    )
}

#[test]
fn launcher_requires_exact_preparation_and_presented_identity_for_activation() {
    let (mut f, mut c) = connected();
    assert_eq!(
        f.transport
            .begin_launcher_request(&mut f.epochs, TransactionId::from_raw(2), &request(2)),
        Err(ShellTransportError::WrongCandidate)
    );
    publish(&mut f, &mut c);
    begin(&mut f, &mut c, 2);
    accept(&mut f, &mut c, 2);
    assert_eq!(
        f.transport.send_launcher_outcome(
            &mut f.epochs,
            TransactionId::from_raw(2),
            outcome(2, ShellV1CandidateOutcomeKind::Presented)
        ),
        Err(ShellTransportError::WrongCandidate)
    );
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Prepared);
    assert_eq!(
        f.transport.queue_launcher_activation(
            &mut f.epochs,
            TransactionId::from_raw(5),
            activation(2)
        ),
        Err(ShellTransportError::WrongActivation)
    );
    assert_eq!(
        f.transport.send_launcher_outcome(
            &mut f.epochs,
            TransactionId::from_raw(2),
            outcome(3, ShellV1CandidateOutcomeKind::Presented)
        ),
        Err(ShellTransportError::WrongCandidate)
    );
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Presented);
    for change in 0..5 {
        let mut value = activation(2);
        match change {
            0 => value.catalog_generation += 1,
            1 => value.request_generation += 1,
            2 => value.candidate_generation += 1,
            3 => value.presentation_epoch += 1,
            _ => value.slot = 2,
        }
        assert_eq!(
            f.transport
                .queue_launcher_activation(&mut f.epochs, TransactionId::from_raw(5), value),
            Err(ShellTransportError::WrongActivation)
        );
    }
    queue(&mut f, &mut c);
    assert_eq!(
        launch(&mut f, ShellLaunchStatus::Started),
        Err(ShellTransportError::WrongActivation)
    );
    ack(&mut f, &mut c, true);
    launch(&mut f, ShellLaunchStatus::Started).unwrap();
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::LaunchOutcome(ShellLaunchOutcome {
            activation: activation(2),
            status: ShellLaunchStatus::Started
        })
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    assert_eq!(
        launch(&mut f, ShellLaunchStatus::Started),
        Err(ShellTransportError::WrongActivation)
    );
}

#[test]
fn launcher_stale_identity_visibility_slots_and_generation_are_superseded() {
    for change in 0..6 {
        let (mut f, mut c) = connected();
        present(&mut f, &mut c);
        begin(&mut f, &mut c, 3);
        let mut value = candidate(3);
        match change {
            0 => value.catalog_generation = 2,
            1 => value.request_generation = 2,
            2 => value.output = OutputId::from_raw(99),
            3 => value.candidate_generation = 2,
            4 => value.visible = false,
            _ => {
                value.entries = vec![2];
                value.selected = 2
            }
        }
        submit(
            &mut f,
            &mut c,
            3,
            ShellDescriptorRecord::LauncherCandidate(value.clone()),
        );
        assert!(
            matches!(drive(&mut f,&mut c,|f,_|f.transport.poll_launcher_candidate(&mut f.epochs).unwrap()),ShellLauncherCandidateEvent::Refused(tx) if tx.raw()==3)
        );
        let mut expected = outcome(3, ShellV1CandidateOutcomeKind::Superseded);
        expected.request_generation = value.request_generation;
        expected.candidate_generation = value.candidate_generation;
        assert_eq!(
            observe(&mut f, &mut c).record,
            ShellDescriptorRecord::LauncherOutcome(expected)
        );
        assert_eq!(
            f.transport.content_accounting(&f.epochs).response_records,
            0
        );
        begin(&mut f, &mut c, 4);
        accept(&mut f, &mut c, 4);
    }
}

#[test]
fn launcher_wrong_transaction_preserves_request_and_cancel_preserves_terminal_credit() {
    let (mut f, mut c) = connected();
    publish(&mut f, &mut c);
    begin(&mut f, &mut c, 2);
    submit(
        &mut f,
        &mut c,
        1,
        ShellDescriptorRecord::LauncherCandidate(candidate(1)),
    );
    assert!(
        matches!(drive(&mut f,&mut c,|f,_|f.transport.poll_launcher_candidate(&mut f.epochs).unwrap()),ShellLauncherCandidateEvent::Refused(tx) if tx.raw()==1)
    );
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::LauncherOutcome(outcome(1, ShellV1CandidateOutcomeKind::Superseded))
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    accept(&mut f, &mut c, 2);
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Prepared);
    f.transport.revoke_launcher();
    assert_eq!(
        f.transport.send_launcher_outcome(
            &mut f.epochs,
            TransactionId::from_raw(2),
            outcome(2, ShellV1CandidateOutcomeKind::Presented)
        ),
        Err(ShellTransportError::WrongCandidate)
    );
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Superseded);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    begin(&mut f, &mut c, 3);
    f.transport.revoke_launcher();
    submit(
        &mut f,
        &mut c,
        3,
        ShellDescriptorRecord::LauncherCandidate(candidate(3)),
    );
    assert!(
        matches!(drive(&mut f,&mut c,|f,_|f.transport.poll_launcher_candidate(&mut f.epochs).unwrap()),ShellLauncherCandidateEvent::Refused(tx) if tx.raw()==3)
    );
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::LauncherOutcome(outcome(3, ShellV1CandidateOutcomeKind::Superseded))
    );
}

#[test]
fn launcher_ack_must_echo_the_whole_grant_and_cannot_release_launch_credit() {
    let (mut f, mut c) = connected();
    present(&mut f, &mut c);
    queue(&mut f, &mut c);
    for change in 0..7 {
        let mut value = activation(2);
        let mut tx = 5;
        match change {
            0 => tx = 6,
            1 => value.catalog_generation += 1,
            2 => value.request_generation += 1,
            3 => value.candidate_generation += 1,
            4 => value.presentation_epoch += 1,
            5 => value.activation += 1,
            _ => value.slot = 2,
        }
        submit(
            &mut f,
            &mut c,
            tx,
            ShellDescriptorRecord::LauncherActivationAck(ShellLauncherActivationAck {
                activation: value,
                consumed: true,
            }),
        );
        assert_eq!(
            f.transport
                .poll_launcher_activation_ack(&mut f.epochs)
                .unwrap(),
            None
        );
        assert_eq!(f.transport.launcher_unmatched_acks(), change + 1);
        assert_eq!(
            f.transport.content_accounting(&f.epochs).response_records,
            1
        );
    }
    ack(&mut f, &mut c, true);
    submit(
        &mut f,
        &mut c,
        5,
        ShellDescriptorRecord::LauncherActivationAck(ShellLauncherActivationAck {
            activation: activation(2),
            consumed: true,
        }),
    );
    assert_eq!(
        f.transport
            .poll_launcher_activation_ack(&mut f.epochs)
            .unwrap(),
        None
    );
    assert_eq!(f.transport.launcher_unmatched_acks(), 8);
    launch(&mut f, ShellLaunchStatus::Failed).unwrap();
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::LaunchOutcome(ShellLaunchOutcome {
            activation: activation(2),
            status: ShellLaunchStatus::Failed
        })
    );
}

#[test]
fn refusal_or_revocation_cannot_be_reported_as_a_started_launch() {
    for revoke in [false, true] {
        let (mut f, mut c) = connected();
        present(&mut f, &mut c);
        queue(&mut f, &mut c);
        ack(&mut f, &mut c, revoke);
        if revoke {
            f.transport.revoke_launcher();
        }
        assert_eq!(
            launch(&mut f, ShellLaunchStatus::Started),
            Err(ShellTransportError::WrongActivation)
        );
        assert_eq!(
            launch(&mut f, ShellLaunchStatus::Failed),
            Err(ShellTransportError::WrongActivation)
        );
        launch(&mut f, ShellLaunchStatus::Rejected).unwrap();
        assert_eq!(
            observe(&mut f, &mut c).record,
            ShellDescriptorRecord::LaunchOutcome(ShellLaunchOutcome {
                activation: activation(2),
                status: ShellLaunchStatus::Rejected
            })
        );
        assert_eq!(
            f.transport.content_accounting(&f.epochs).response_records,
            0
        );
    }
}

#[test]
fn launcher_request_and_responses_fit_or_nothing_is_transferred() {
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
    let mut c = f.connect(8, BASE | (1 << 5) | (1 << 6) | (1 << 7));
    publish(&mut f, &mut c);
    let before = f.transport.content_accounting(&f.epochs);
    assert_eq!(
        f.transport
            .begin_launcher_request(&mut f.epochs, TransactionId::from_raw(2), &request(2)),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(f.transport.content_accounting(&f.epochs), before);
}

#[test]
fn launch_and_descriptor_credits_share_capacity_and_disconnect_releases_them() {
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
    let mut c = f.connect(8, BASE | (1 << 5) | (1 << 6) | (1 << 7));
    present(&mut f, &mut c);
    super::owner::request(&mut f, &mut c, 7);
    assert_eq!(
        f.transport.queue_launcher_activation(
            &mut f.epochs,
            TransactionId::from_raw(5),
            activation(2)
        ),
        Err(ShellTransportError::ActivationQueueSaturated)
    );
    super::owner::accept(&mut f, &mut c, 7);
    super::owner::outcome(&mut f, &mut c, 7, ShellV1CandidateOutcomeKind::Rejected);
    queue(&mut f, &mut c);
    f.transport
        .begin_candidate_request(
            &mut f.epochs,
            TransactionId::from_raw(8),
            &super::owner::snapshot(8),
        )
        .unwrap();
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::Descriptors(super::owner::snapshot(8))
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        3
    );
    // The active grant holds capacity and excludes a replacement request/catalog.
    assert_eq!(
        f.transport
            .begin_launcher_request(&mut f.epochs, TransactionId::from_raw(3), &request(3)),
        Err(ShellTransportError::WrongCandidate)
    );
    assert_eq!(
        f.transport
            .publish_launcher_catalog(&f.epochs, TransactionId::from_raw(3), &catalog()),
        Err(ShellTransportError::WrongCandidate)
    );
    f.transport.disconnect(&mut f.epochs).unwrap();
    assert!(f.transport.content_accounting(&f.epochs).quiescent());
}
