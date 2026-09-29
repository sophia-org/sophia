//! Remaining socket-owner controls, expressed through the descriptor file
//! profile. Other owner tests retain preparation, presentation and exact ACKs.
use super::*;

#[test]
fn activation_limit_revokes_even_when_the_peer_drains_every_journal_event() {
    let (mut f, mut c) = connected();
    request(&mut f, &mut c, 1);
    accept(&mut f, &mut c, 1);
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Prepared);
    outcome(&mut f, &mut c, 1, ShellV1CandidateOutcomeKind::Presented);
    for id in 1..=SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS as u64 {
        let value = ShellV1Activation {
            activation: id,
            ..activation(1)
        };
        let transaction = TransactionId::from_raw(id);
        f.transport
            .queue_activation(&mut f.epochs, transaction, value)
            .unwrap();
        assert_eq!(
            observe(&mut f, &mut c),
            ShellFileDescriptorRecord {
                transaction,
                record: ShellDescriptorRecord::DescriptorActivation(value),
            }
        );
    }
    // Journal ACKs release retention, but cannot discharge domain activations.
    let id = SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS as u64 + 1;
    assert_eq!(
        f.transport.queue_activation(
            &mut f.epochs,
            TransactionId::from_raw(id),
            ShellV1Activation {
                activation: id,
                ..activation(1)
            }
        ),
        Err(ShellTransportError::ActivationQueueSaturated)
    );
    assert!(f.transport.content_accounting(&f.epochs).quiescent());
    assert_eq!(
        f.transport.poll_io(&mut f.epochs),
        Err(ShellTransportError::NotConnected)
    );
}

#[test]
fn snapshot_refusal_preserves_the_next_candidate_and_its_exact_reservation() {
    let (mut f, mut c) = connected();
    request(&mut f, &mut c, 1);
    let mut stale = candidate(1);
    stale.snapshot_generation += 1;
    submit(
        &mut f,
        &mut c,
        1,
        ShellDescriptorRecord::DescriptorCandidate(stale),
    );
    assert_eq!(f.transport.poll_candidate(&mut f.epochs).unwrap(), None);
    assert_eq!(
        observe(&mut f, &mut c),
        ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(1),
            record: ShellDescriptorRecord::DescriptorOutcome(ShellV1CandidateOutcome {
                connection_epoch: EPOCH,
                candidate_generation: 1,
                presentation_epoch: 0,
                kind: ShellV1CandidateOutcomeKind::Rejected,
            }),
        }
    );
    assert_eq!(
        f.transport.content_accounting(&f.epochs).response_records,
        0
    );
    request(&mut f, &mut c, 2);
    let mut value = candidate(2);
    value.reservation = Some(ShellV1WorkAreaReservation {
        edge: ShellV1ReservationEdge::Bottom,
        thickness_px: 28,
    });
    submit(
        &mut f,
        &mut c,
        2,
        ShellDescriptorRecord::DescriptorCandidate(value.clone()),
    );
    assert_eq!(
        f.transport.poll_candidate(&mut f.epochs).unwrap(),
        Some(value)
    );
    outcome(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Prepared);
    outcome(&mut f, &mut c, 2, ShellV1CandidateOutcomeKind::Presented);
}

#[test]
fn reconnect_burns_the_old_epoch_and_negotiates_a_fresh_one() {
    let (mut f, old) = connected();
    assert_eq!(old.connection_epoch(), EPOCH);
    f.transport.disconnect(&mut f.epochs).unwrap();
    assert!(
        f.transport
            .collect_content_accounting(&mut f.epochs)
            .quiescent()
    );
    assert_eq!(
        f.transport.begin_descriptor_file_negotiation(
            &f.epochs,
            EPOCH,
            WAIT,
            ShellContentAdmissionPolicy::Unavailable
        ),
        Err(ShellTransportError::InvalidConnectionEpoch)
    );
    f.transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    f.transport
        .begin_descriptor_file_negotiation(
            &f.epochs,
            EPOCH + 1,
            WAIT,
            ShellContentAdmissionPolicy::Unavailable,
        )
        .unwrap();
    let new = f.connect_at(8, BASE, EPOCH + 1);
    assert_eq!(new.connection_epoch(), EPOCH + 1);
    assert_eq!(f.transport.connection_epoch(), EPOCH + 1);
    assert_eq!(
        f.transport.begin_descriptor_file_negotiation(
            &f.epochs,
            EPOCH,
            Duration::ZERO,
            ShellContentAdmissionPolicy::Unavailable
        ),
        Err(ShellTransportError::InvalidConnectionEpoch)
    );
}
