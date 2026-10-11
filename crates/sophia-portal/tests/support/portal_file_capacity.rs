use super::*;

fn pressure(records: bool) {
    let mut limits = PortalFileLimits::defaults(1);
    if records {
        limits.journal_records = 4;
    } else {
        limits.journal_bytes = 280;
    }
    // After ACKing only the first Submitted, one 88-byte Decision remains.
    // Another request needs two records/136 bytes and raises outstanding
    // terminal reservations to two records/96 bytes. Only one bound is tight.
    let needed_records = 1 + 2 + 2;
    let needed_bytes = 88 + 136 + 96;
    assert_eq!(needed_records > limits.journal_records, records);
    assert_eq!(needed_bytes > limits.journal_bytes, !records);
    let mut f = Fixture::new(limits);
    f.negotiate();
    f.send(request(1)).unwrap();
    let first = f.events(false);
    assert!(matches!(
        first.as_slice(),
        [
            (_, PortalFileEvent::Submitted(_)),
            (
                _,
                PortalFileEvent::Decision(PortalBrokerResponsePacket {
                    decision: PortalBrokerResponseDecision::Allowed(_),
                    ..
                })
            )
        ]
    ));
    f.ack(first[0].0).unwrap();
    let submission = f.next;
    assert_eq!(
        f.send(request(2)),
        Err(Errno::EAGAIN),
        "journal {} limit must reserve terminal capacity",
        if records { "record" } else { "byte" },
    );
    assert_eq!(f.next, submission);
    assert!(
        f.events(false).is_empty(),
        "refused request publishes no partial batch"
    );
    f.export.authority_mut().rights = false;
    f.export.expire(Instant::now());
    f.export.authority_mut().rights = true;
    let terminal = f.events(true);
    assert!(
        matches!(terminal.as_slice(), [(_, PortalFileEvent::TransferOutcome(PortalTransferOutcome { transfer, state: PortalGrantState::Revoked }))] if *transfer == PortalTransferId::from_raw(1)),
        "reserved terminal outcome survives pressure"
    );
    assert!(
        matches!(f.request(2), PortalBrokerResponseDecision::Allowed(_)),
        "pressure refusal must not spend the transfer identity"
    );
}

#[test]
fn record_credit_alone_preserves_terminal_outcome_and_retry_identity() {
    pressure(true);
}

#[test]
fn byte_credit_alone_preserves_terminal_outcome_and_retry_identity() {
    pressure(false);
}

#[test]
fn active_limit_denial_is_distinct_from_journal_and_history_exhaustion() {
    let mut limits = PortalFileLimits::defaults(1);
    limits.max_transfers = 1;
    assert_eq!(limits.max_retained_transfers, 4096);
    let mut f = Fixture::new(limits);
    f.negotiate();
    assert!(matches!(
        f.request(1),
        PortalBrokerResponseDecision::Allowed(_)
    ));
    // Every previous event is ACKed; neither journal bound nor retained history
    // is near capacity. Saturation is a Decision, not transport backpressure.
    assert_eq!(
        f.send(request(2)),
        Ok(()),
        "active saturation must return a correlated denial"
    );
    let denied = f.events(true);
    assert!(
        matches!(denied.as_slice(), [(_, PortalFileEvent::Submitted(_)), (_, PortalFileEvent::Decision(PortalBrokerResponsePacket { transfer, decision: PortalBrokerResponseDecision::Denied }))] if *transfer == PortalTransferId::from_raw(2)),
        "active limit must deny the new transfer without disturbing the old grant"
    );
    f.send(PortalFileCandidate::TransferCancel(
        PortalTransferId::from_raw(1),
    ))
    .unwrap();
    let cancelled = f.events(true);
    assert!(matches!(
        cancelled.last().unwrap().1,
        PortalFileEvent::TransferOutcome(PortalTransferOutcome {
            state: PortalGrantState::Revoked,
            ..
        })
    ));
    assert!(
        matches!(f.request(2), PortalBrokerResponseDecision::Allowed(_)),
        "active-limit denial must not retain an unadmitted identity"
    );
    assert_eq!(
        f.request(1),
        PortalBrokerResponseDecision::Denied,
        "settled identity remains spent independently of active capacity"
    );
}
