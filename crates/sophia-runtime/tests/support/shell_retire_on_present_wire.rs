//! W2 prototype on the file wire: a candidate's retire intent is admitted
//! only on a connection negotiated at allocated revision 12 with capability
//! bit 17 granted, rides with its whole candidate to the exact End, and is
//! reserved there by `end_with_retire`. Negotiation is test-only setup; no
//! default policy advertises W2.
use super::*;
use sophia_protocol::shell::content::{
    ContentCandidateRetire, SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT as W2,
};

const LEGACY: Option<crate::ContentStoreProfile> = Some(crate::ContentStoreProfile::Legacy);

fn tx(raw: u64) -> TransactionId {
    TransactionId::from_raw(raw)
}

/// A connection negotiated at `revision` with `capabilities` added, resource
/// 1 accepted and permit 1 granted on output 1; staging output is dropped.
fn connect(
    revision: u16,
    capabilities: u64,
) -> (
    Fixture,
    Peer,
    ContentGrant,
    ContentOutputId,
    ContentResourceId,
) {
    let mut fixture = Fixture::new(16);
    let transport = &mut fixture.transport;
    transport.state.capabilities |= capabilities;
    let peer = Peer::attach_at(&mut transport.state, LEGACY, revision);
    let (grant, output, predecessor) = stage_predecessor(transport);
    drop_output(transport);
    (fixture, peer, grant, output, predecessor)
}

/// Drops pending resource statuses and queued output, which the peer would
/// otherwise have to read; the assertions here are on store state.
fn drop_output(transport: &mut ShellSessionTransport) {
    let resources = transport
        .content_epochs
        .resources_mut(transport.state.store_grant)
        .unwrap();
    while resources.take_event().is_some() {}
    while transport.state.output.front().is_some() {
        transport.state.output.pop_front();
    }
}

fn candidate(
    transaction: u64,
    generation: u64,
    permit: u64,
    grant: ContentGrant,
    output: ContentOutputId,
) -> ShellFileCandidate {
    ShellFileCandidate {
        transaction: tx(transaction),
        candidate: ContentCandidate {
            grant,
            candidate_generation: generation,
            output,
            facts_generation: 1,
            pacing_permit: permit,
            interaction_generation: 1,
            surfaces: Vec::new(),
            placements: Vec::new(),
            targets: Vec::new(),
        },
    }
}

fn intent(retire_transaction: u64, id: u64) -> ContentCandidateRetire {
    ContentCandidateRetire {
        retire_transaction: tx(retire_transaction),
        resource: ContentResourceId { id, generation: 1 },
    }
}

fn submit(
    peer: &mut Peer,
    transport: &mut ShellSessionTransport,
    value: &ShellFileCandidate,
    retire: Option<ContentCandidateRetire>,
) -> bool {
    peer.try_submit(&mut transport.state, ShellFileKind::Candidate, |header| {
        match &retire {
            Some(retire) => encode_shell_file_candidate_with_retire(header, value, retire),
            None => encode_shell_file_candidate(header, value),
        }
        .unwrap()
    })
}

fn service(transport: &mut ShellSessionTransport, output: ContentOutputId) -> usize {
    let context = crate::ContentCandidateContext {
        output,
        facts_generation: 1,
        interaction_generation: 1,
        allocations: &[],
    };
    transport
        .state
        .service_content_candidates(&mut transport.content_epochs, &[context], 1)
        .unwrap()
}

fn reserved(transport: &ShellSessionTransport) -> u64 {
    transport
        .content_epochs
        .resources(transport.state.store_grant)
        .unwrap()
        .reserved_retiring()
}

fn leases_refused(transport: &ShellSessionTransport, grant: ContentGrant, id: u64) -> bool {
    transport
        .content_epochs
        .resources(transport.state.store_grant)
        .unwrap()
        .lease(grant, ContentResourceId { id, generation: 1 })
        .is_err()
}

#[test]
fn an_unpermitted_connection_refuses_a_flagged_candidate_before_custody() {
    // r11 is the draw lane's revision and bit 16 its capability: neither is
    // W2. Only allocated r12 with bit 17 granted may carry the intent.
    for (revision, capabilities) in [
        (8, W2),
        (10, W2),
        (11, W2),
        (12, 0),
        (12, 1 << 16),
        (13, W2),
    ] {
        let (mut fixture, mut peer, grant, output, _) = connect(revision, capabilities);
        let transport = &mut fixture.transport;
        let value = candidate(2, 1, 1, grant, output);
        let errno = peer.refused_submit(&mut transport.state, ShellFileKind::Candidate, |h| {
            encode_shell_file_candidate_with_retire(h, &value, &intent(50, 1)).unwrap()
        });
        assert_eq!(errno, 22, "r{revision} caps {capabilities:#x}");
        peer.quiet(&mut transport.state);
        assert!(transport.state.inbound_idle(), "no custody");
        assert_eq!(service(transport, output), 0);
        assert_eq!(reserved(transport), 0);
        // The same connection still takes the unflagged candidate.
        assert!(submit(&mut peer, transport, &value, None));
        assert!(service(transport, output) > 0);
        assert_eq!(reserved(transport), 0);
        assert!(!leases_refused(transport, grant, 1));
    }
}

#[test]
fn a_permitted_connection_reserves_at_end_and_an_unflagged_candidate_never() {
    let revision = 12;
    let (mut fixture, mut peer, grant, output, _) = connect(revision, W2);
    let transport = &mut fixture.transport;
    // Control: no intent, no reservation.
    assert!(submit(
        &mut peer,
        transport,
        &candidate(2, 1, 1, grant, output),
        None
    ));
    assert!(service(transport, output) > 0);
    assert_eq!(reserved(transport), 0);
    transport
        .grant_content_permit(tx(3), output, 2, 2, 1)
        .unwrap();
    drop_output(transport);
    let value = candidate(4, 2, 2, grant, output);
    assert!(submit(&mut peer, transport, &value, Some(intent(50, 1))));
    // Decoding reserves nothing: the reservation is End's.
    assert_eq!(reserved(transport), 0);
    assert!(!leases_refused(transport, grant, 1));
    assert!(service(transport, output) > 0);
    assert_eq!(reserved(transport), 8, "r{revision}");
    assert!(leases_refused(transport, grant, 1));
    assert!(transport.state.inbound_idle());
}

/// The peer reads the rejection of candidate `generation`, its only outcome.
fn rejected(peer: &mut Peer, transport: &mut ShellSessionTransport, generation: u64) {
    let event = peer.take_event(&mut transport.state);
    let value = decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
    let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
        panic!("candidate outcome");
    };
    assert_eq!(outcome.candidate_generation, generation);
    assert_ne!(outcome.kind, 2, "not presented");
    peer.quiet(&mut transport.state);
}

#[test]
fn a_rejected_end_takes_its_intent_and_a_reused_transaction_starts_clean() {
    let (mut fixture, mut peer, grant, output, _) = connect(12, W2);
    let transport = &mut fixture.transport;
    // A names a resource that was never accepted: End rejects the whole
    // candidate, and its intent goes with it.
    let a = candidate(2, 1, 1, grant, output);
    assert!(submit(&mut peer, transport, &a, Some(intent(50, 7))));
    assert!(service(transport, output) > 0);
    rejected(&mut peer, transport, 1);
    assert_eq!(reserved(transport), 0);
    assert!(transport.state.inbound_idle());
    // B reuses A's transaction without an intent and inherits nothing.
    transport
        .grant_content_permit(tx(3), output, 2, 2, 1)
        .unwrap();
    drop_output(transport);
    let b = candidate(2, 2, 2, grant, output);
    assert!(submit(&mut peer, transport, &b, None));
    assert!(service(transport, output) > 0);
    assert_eq!(reserved(transport), 0);
    assert!(!leases_refused(transport, grant, 1));
    // C reuses the transaction and A's retire ID, now on resource 1.
    transport
        .grant_content_permit(tx(4), output, 3, 3, 1)
        .unwrap();
    drop_output(transport);
    let c = candidate(2, 3, 3, grant, output);
    assert!(submit(&mut peer, transport, &c, Some(intent(50, 1))));
    assert!(service(transport, output) > 0);
    assert_eq!(reserved(transport), 8);
}

#[test]
fn a_rejected_begin_discards_the_end_and_its_intent() {
    let (mut fixture, mut peer, grant, output, _) = connect(12, W2);
    let transport = &mut fixture.transport;
    // A sets generation 5; B then names stale generation 3, so its Begin is
    // rejected with an outcome and the discarded rest includes the End that
    // carried the intent.
    assert!(submit(
        &mut peer,
        transport,
        &candidate(2, 5, 1, grant, output),
        None
    ));
    assert!(service(transport, output) > 0);
    transport
        .grant_content_permit(tx(3), output, 2, 2, 1)
        .unwrap();
    drop_output(transport);
    let b = candidate(4, 3, 2, grant, output);
    assert!(submit(&mut peer, transport, &b, Some(intent(50, 1))));
    assert_eq!(service(transport, output), 1, "only Begin is serviced");
    rejected(&mut peer, transport, 3);
    assert!(transport.state.inbound_idle());
    assert_eq!(reserved(transport), 0);
    assert!(!leases_refused(transport, grant, 1));
    // The next candidate reuses B's transaction without an intent.
    transport
        .grant_content_permit(tx(5), output, 3, 3, 1)
        .unwrap();
    drop_output(transport);
    assert!(submit(
        &mut peer,
        transport,
        &candidate(4, 6, 3, grant, output),
        None
    ));
    assert!(service(transport, output) > 0);
    assert_eq!(reserved(transport), 0);
    assert!(!leases_refused(transport, grant, 1));
}

#[test]
fn an_identical_retry_is_one_candidate_and_one_reservation() {
    let (mut fixture, mut peer, grant, output, _) = connect(12, W2);
    let transport = &mut fixture.transport;
    let value = candidate(2, 1, 1, grant, output);
    peer.submit_twice(&mut transport.state, ShellFileKind::Candidate, |h| {
        encode_shell_file_candidate_with_retire(h, &value, &intent(50, 1)).unwrap()
    });
    assert!(service(transport, output) > 0);
    assert!(transport.state.inbound_idle());
    assert_eq!(service(transport, output), 0);
    assert_eq!(reserved(transport), 8);
}

#[test]
fn disconnect_before_service_drops_the_queued_intent() {
    let (mut fixture, mut peer, grant, output, _) = connect(12, W2);
    let transport = &mut fixture.transport;
    assert!(submit(
        &mut peer,
        transport,
        &candidate(2, 1, 1, grant, output),
        Some(intent(50, 1)),
    ));
    let store = transport.state.store_grant;
    transport.disconnect().unwrap();
    assert!(transport.state.inbound_idle());
    assert!(
        transport
            .content_epochs
            .resources(store)
            .is_none_or(|resources| resources.reserved_retiring() == 0)
    );
}

#[test]
fn a_wire_intent_releases_after_presented_and_waits_for_a_held_lease() {
    let (mut fixture, mut peer, grant, output, predecessor) = connect(12, W2);
    let transport = &mut fixture.transport;
    // The renderer still holds the predecessor from an earlier frame.
    let held = transport
        .content_epochs
        .resources(transport.state.store_grant)
        .unwrap()
        .lease(grant, predecessor)
        .unwrap();
    assert!(submit(
        &mut peer,
        transport,
        &candidate(2, 1, 1, grant, output),
        Some(intent(50, 1)),
    ));
    assert!(service(transport, output) > 0);
    assert_eq!(reserved(transport), 8);
    let _bundle = transport.begin_content_submission(output, 1, 2).unwrap();
    transport
        .content_prepared(grant, output, 1, 1, 1, 2)
        .unwrap();
    transport
        .content_epochs
        .candidates_mut(grant)
        .unwrap()
        .presented(output, 1, 1, 1, 1)
        .unwrap();
    let flush = |transport: &mut ShellSessionTransport| {
        transport
            .state
            .flush_content_candidate_events(&mut transport.content_epochs)
            .unwrap();
        transport.content_epochs.collect();
        transport
            .state
            .flush_content_resource_events(&mut transport.content_epochs)
            .unwrap();
    };
    let order = |transport: &mut ShellSessionTransport| {
        let mut order = Vec::new();
        while transport.state.output.front().is_some() {
            if let OutboundRecord::Content(transaction, record) =
                transport.state.output.pop_front().record
            {
                match record {
                    ShellContentRecord::CandidateOutcome(value) if value.kind == 2 => {
                        order.push(("presented", transaction))
                    }
                    ShellContentRecord::ResourceReleased(value) => {
                        assert_eq!(value.resource, predecessor);
                        order.push(("released", transaction))
                    }
                    _ => {}
                }
            }
        }
        order
    };
    flush(transport);
    assert_eq!(reserved(transport), 0, "committed at transfer");
    assert_eq!(
        order(transport),
        [("presented", tx(2))],
        "the lease holds it"
    );
    drop(held);
    flush(transport);
    assert_eq!(order(transport), [("released", tx(50))]);
}
