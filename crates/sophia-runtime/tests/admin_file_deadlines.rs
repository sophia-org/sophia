#[path = "support/admin_files.rs"]
mod support;
use support::*;

#[test]
fn indeterminate_results_keep_execution_capacity_until_owner_settlement() {
    let f = Fixture::new();
    let mut peers = Vec::new();
    let mut tickets = Vec::new();
    for _ in 0..16 {
        let mut peer = f.peer();
        assert_eq!(peer.invoke(1, 7)[4], 119);
        let ticket = f.ticket();
        claim(&ticket);
        tickets.push(ticket);
        peers.push(peer);
    }
    let mut extra = f.peer();
    assert_eq!(errno(&extra.invoke(1, 7)), 11);
    assert_eq!(
        extra.status(),
        AdminStatus::Ready {
            connection: extra.info.connection
        }
    );
    // Exercise the advertised deadline, not a test-only clock or ticket phase.
    let deadline = tickets.last().unwrap().received + Duration::from_secs(10);
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    for peer in &mut peers {
        assert_eq!(peer.outcome(ControlOutcome::Indeterminate).request, 1);
        assert_eq!(errno(&peer.invoke(2, 7)), 11);
        assert_eq!(peer.outcome(ControlOutcome::Indeterminate).request, 1);
    }
    assert_eq!(errno(&extra.invoke(1, 7)), 11);
    assert!(f.service.try_request().is_none());
    // Settling another connection permits a successor, but not a late result
    // from the first execution to overwrite that successor's pending status.
    tickets[1].finish(ControlOutcome::Committed);
    until(|| peers[0].invoke(2, 7)[4] == 119);
    let successor = f.ticket();
    tickets[0].finish(ControlOutcome::Committed);
    assert_eq!(
        peers[0].status(),
        AdminStatus::Pending {
            connection: peers[0].info.connection,
            request: 2,
            generation: 7,
        }
    );
    claim(&successor);
    successor.finish(ControlOutcome::Unchanged);
    assert_eq!(peers[0].outcome(ControlOutcome::Unchanged).request, 2);
    for ticket in &tickets[2..] {
        ticket.finish(ControlOutcome::Completed);
    }
}

#[test]
fn queued_deadline_cancels_claim_and_releases_capacity() {
    let f = Fixture::new();
    let mut peer = f.peer();
    assert_eq!(peer.invoke(1, 7)[4], 119);
    let ticket = f.ticket();
    std::thread::sleep(
        (ticket.received + Duration::from_secs(10)).saturating_duration_since(Instant::now()),
    );
    assert_eq!(peer.outcome(ControlOutcome::TimedOut).request, 1);
    assert!(ticket.cancelled());
    assert!(!ticket.claim());
    assert_eq!(peer.invoke(2, 7)[4], 119);
    f.ticket().finish(ControlOutcome::Completed);
    assert_eq!(peer.outcome(ControlOutcome::Completed).request, 2);
}
