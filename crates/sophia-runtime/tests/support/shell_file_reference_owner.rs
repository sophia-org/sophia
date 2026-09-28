//! Real SDK/file-export reference ownership, without a renderer or Session
//! projection. Refusals must preserve unrelated response obligations.
use super::owner::{drive, observe, submit};
use super::*;

fn connected() -> (Fixture, ShellConnection) {
    let mut f = Fixture::new();
    f.start(ShellContentAdmissionPolicy::Unavailable);
    let c = f.connect(8, BASE | (1 << 3) | (1 << 4));
    (f, c)
}
fn catalog() -> ShellShortcutCatalog {
    ShellShortcutCatalog {
        connection_epoch: EPOCH,
        generation: 1,
        entries: vec![ShellShortcut {
            slot: 1,
            chord: "Super+q".into(),
            action: "session:quit".into(),
            label: None,
            group: None,
        }],
    }
}
fn request(generation: u64) -> ShellReferenceRequest {
    ShellReferenceRequest {
        connection_epoch: EPOCH,
        catalog_generation: 1,
        request_generation: generation,
        output: OutputId::from_raw(3),
        output_generation: 4,
        presentation_epoch: 0,
        operation: ShellReferenceOperation::Toggle,
    }
}
fn candidate(generation: u64) -> ShellReferenceCandidate {
    ShellReferenceCandidate {
        connection_epoch: EPOCH,
        catalog_generation: 1,
        request_generation: generation,
        candidate_generation: generation,
        output: OutputId::from_raw(3),
        visible: true,
        page: 0,
        style: ShellReferenceStyle {
            body_size: 12,
            title_size: 16,
            padding: 4,
            row_gap: 2,
            key_gap: 4,
            column_gap: 4,
            border: 1,
            margin: 4,
            columns: 1,
            colors: [0xff000000; 6],
            title: "Shortcuts".into(),
        },
        entries: vec![ShellReferenceEntry {
            slot: 1,
            key: "Super+q".into(),
            label: "Quit".into(),
        }],
    }
}
fn outcome(generation: u64, kind: ShellV1CandidateOutcomeKind) -> ShellReferenceOutcome {
    ShellReferenceOutcome {
        connection_epoch: EPOCH,
        catalog_generation: 1,
        request_generation: generation,
        candidate_generation: generation,
        presentation_epoch: if kind == ShellV1CandidateOutcomeKind::Presented {
            9
        } else {
            0
        },
        page: 0,
        pages: 1,
        kind,
    }
}
fn publish(f: &mut Fixture, c: &mut ShellConnection) {
    f.transport
        .publish_shortcuts(&mut f.epochs, TransactionId::from_raw(1), &catalog())
        .unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(1),
            record: ShellDescriptorRecord::Shortcuts(catalog()),
        }
    );
}
fn begin(f: &mut Fixture, c: &mut ShellConnection, generation: u64) {
    f.transport
        .begin_reference_request(
            &mut f.epochs,
            TransactionId::from_raw(generation),
            request(generation),
        )
        .unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(generation),
            record: ShellDescriptorRecord::ReferenceRequest(request(generation)),
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
        ShellDescriptorRecord::ReferenceCandidate(candidate(generation)),
    );
    let event = drive(f, c, |f, _| {
        f.transport.poll_reference_candidate(&mut f.epochs).unwrap()
    });
    let ShellReferenceCandidateEvent::Candidate(tx, value) = event else {
        panic!("valid candidate refused")
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
        .send_reference_outcome(
            &mut f.epochs,
            TransactionId::from_raw(generation),
            outcome(generation, kind),
        )
        .unwrap();
    assert_eq!(
        observe(f, c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(generation),
            record: ShellDescriptorRecord::ReferenceOutcome(outcome(generation, kind)),
        }
    );
}

#[test]
fn reference_requires_catalog_and_exact_prepared_then_terminal() {
    let (mut f, mut c) = connected();
    assert_eq!(
        f.transport
            .begin_reference_request(&mut f.epochs, TransactionId::from_raw(2), request(2)),
        Err(ShellTransportError::WrongCandidate)
    );
    publish(&mut f, &mut c);
    begin(&mut f, &mut c, 2);
    assert_eq!(
        f.transport
            .publish_shortcuts(&mut f.epochs, TransactionId::from_raw(2), &catalog()),
        Err(ShellTransportError::WrongCandidate)
    );
    accept(&mut f, &mut c, 2);
    for (tx, value) in [
        (2, outcome(2, ShellV1CandidateOutcomeKind::Presented)),
        (3, outcome(2, ShellV1CandidateOutcomeKind::Prepared)),
        (2, outcome(3, ShellV1CandidateOutcomeKind::Prepared)),
    ] {
        assert_eq!(
            f.transport
                .send_reference_outcome(&mut f.epochs, TransactionId::from_raw(tx), value),
            Err(ShellTransportError::WrongCandidate)
        );
        assert_eq!(
            f.transport.content_accounting(&f.epochs).response_records,
            2
        );
    }
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Prepared);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        1
    );
    assert_eq!(
        f.transport.send_reference_outcome(
            &mut f.epochs,
            TransactionId::from_raw(2),
            outcome(2, ShellV1CandidateOutcomeKind::Prepared)
        ),
        Err(ShellTransportError::WrongCandidate)
    );
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Presented);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    assert_eq!(
        f.transport.send_reference_outcome(
            &mut f.epochs,
            TransactionId::from_raw(2),
            outcome(2, ShellV1CandidateOutcomeKind::Superseded)
        ),
        Err(ShellTransportError::WrongCandidate)
    );
}

#[test]
fn cancellation_supersedes_even_a_stale_reply_and_releases_both_credits() {
    let (mut f, mut c) = connected();
    publish(&mut f, &mut c);
    begin(&mut f, &mut c, 2);
    f.transport.cancel_reference_request();
    let mut value = candidate(2);
    value.output = OutputId::from_raw(99);
    submit(
        &mut f,
        &mut c,
        2,
        ShellDescriptorRecord::ReferenceCandidate(value),
    );
    let event = drive(&mut f, &mut c, |f, _| {
        f.transport.poll_reference_candidate(&mut f.epochs).unwrap()
    });
    assert!(matches!(event, ShellReferenceCandidateEvent::Refused(tx) if tx.raw() == 2));
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::ReferenceOutcome(outcome(
            2,
            ShellV1CandidateOutcomeKind::Superseded
        ))
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    begin(&mut f, &mut c, 3);
    accept(&mut f, &mut c, 3);
}

#[test]
fn wrong_transaction_is_rejected_without_cancelling_the_current_request() {
    let (mut f, mut c) = connected();
    publish(&mut f, &mut c);
    begin(&mut f, &mut c, 2);
    submit(
        &mut f,
        &mut c,
        1,
        ShellDescriptorRecord::ReferenceCandidate(candidate(1)),
    );
    assert!(
        matches!(drive(&mut f, &mut c, |f, _| f.transport.poll_reference_candidate(&mut f.epochs).unwrap()), ShellReferenceCandidateEvent::Refused(tx) if tx.raw() == 1)
    );
    assert_eq!(
        observe(&mut f, &mut c).record,
        ShellDescriptorRecord::ReferenceOutcome(outcome(1, ShellV1CandidateOutcomeKind::Rejected))
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    accept(&mut f, &mut c, 2);
    // Session can reject a projection before Prepared, releasing both credits.
    finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Rejected);
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
}

#[test]
fn stale_reference_identity_or_generation_is_rejected_without_presentation() {
    for change in 0..4 {
        let (mut f, mut c) = connected();
        publish(&mut f, &mut c);
        begin(&mut f, &mut c, 2);
        accept(&mut f, &mut c, 2);
        finish(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Rejected);
        begin(&mut f, &mut c, 3);
        let mut value = candidate(3);
        match change {
            0 => value.catalog_generation = 2,
            1 => value.request_generation = 2,
            2 => value.output = OutputId::from_raw(99),
            _ => value.candidate_generation = 2,
        }
        submit(
            &mut f,
            &mut c,
            3,
            ShellDescriptorRecord::ReferenceCandidate(value.clone()),
        );
        assert!(
            matches!(drive(&mut f, &mut c, |f, _| f.transport.poll_reference_candidate(&mut f.epochs).unwrap()), ShellReferenceCandidateEvent::Refused(tx) if tx.raw() == 3)
        );
        let mut expected = outcome(3, ShellV1CandidateOutcomeKind::Rejected);
        expected.catalog_generation = value.catalog_generation;
        expected.request_generation = value.request_generation;
        expected.candidate_generation = value.candidate_generation;
        assert_eq!(
            observe(&mut f, &mut c).record,
            ShellDescriptorRecord::ReferenceOutcome(expected)
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
fn request_and_responses_reserve_capacity_together_before_transfer() {
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
    let mut c = f.connect(8, BASE | (1 << 3) | (1 << 4) | (1 << 7));
    publish(&mut f, &mut c);
    let before = f.transport.content_accounting(&f.epochs);
    assert_eq!(
        f.transport
            .begin_reference_request(&mut f.epochs, TransactionId::from_raw(2), request(2)),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(f.transport.content_accounting(&f.epochs), before);
    f.transport.disconnect(&mut f.epochs).unwrap();
    assert!(f.transport.content_accounting(&f.epochs).quiescent());
}

#[test]
fn descriptor_requests_cannot_spend_reference_response_credits() {
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
    let mut c = f.connect(8, BASE | (1 << 3) | (1 << 4) | (1 << 7));
    publish(&mut f, &mut c);
    begin(&mut f, &mut c, 2);
    assert_eq!(
        f.transport.begin_candidate_request(
            &mut f.epochs,
            TransactionId::from_raw(3),
            &super::owner::snapshot(3),
        ),
        Err(ShellTransportError::ActivationQueueSaturated)
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        2
    );
    f.transport.disconnect(&mut f.epochs).unwrap();
    assert!(f.transport.content_accounting(&f.epochs).quiescent());
}
