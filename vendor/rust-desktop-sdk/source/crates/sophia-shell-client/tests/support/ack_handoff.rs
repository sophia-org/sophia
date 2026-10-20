use super::*;

fn ready() -> (ShellConnection, Peer) {
    let (connection, mut peer) = transaction_reuse::reusable(None);
    let mut connection = connection.unwrap();
    drive(&mut connection, &mut peer, |c, p| {
        p.events_read.is_some() && !c.journal_progress().ack_pending
    })
    .unwrap();
    while connection.take_content().unwrap().is_some() {}
    peer.reset_logs();
    (connection, peer)
}

fn deliver(connection: &mut ShellConnection, peer: &mut Peer) -> u64 {
    let event = permit(peer);
    peer.push(event);
    peer.flush_events();
    connection.poll_io().unwrap();
    peer.pump();
    let state = connection.journal_progress();
    assert!(state.handled > state.queued, "ACK was not handed off");
    assert!(peer.acks.is_empty(), "ACK already on the wire");
    assert!(connection.io_interest().unwrap().immediate);
    assert!(!state.ack_pending);
    state.handled
}

#[test]
fn handed_off_ack_precedes_next_record_without_waiting_for_ack_reply() {
    let (mut connection, mut peer) = ready();
    let sequence = deliver(&mut connection, &mut peer);
    assert!(connection.take_content().unwrap().is_some());
    assert!(
        connection.io_interest().unwrap().immediate,
        "debt remains after inbox is drained"
    );
    peer.hold_ack_replies = true;
    peer.hold_transactions = true;
    let ticket = enqueue_demand(&mut connection, 7);
    connection.poll_io().unwrap();
    peer.pump();
    assert_eq!(peer.acks.len(), 1);
    assert_eq!(peer.acks[0].sequence, sequence);
    assert_eq!(peer.held_replies.len(), 1, "ACK reply remains held");
    assert_eq!(peer.held_transactions.len(), 1, "next record already sent");
    assert_eq!(peer.submits.len(), 1, "submit remains pipelined");
    assert_eq!(peer.write_nodes, ["ack", "transaction", "submit"]);
    assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
    assert!(connection.journal_progress().ack_pending);
}

#[test]
fn a_second_drive_cannot_defer_again_even_with_another_event() {
    let (mut connection, mut peer) = ready();
    deliver(&mut connection, &mut peer);
    let event = permit(&mut peer);
    peer.push(event);
    peer.flush_events();
    connection.poll_io().unwrap();
    peer.pump();
    assert_eq!(peer.acks.len(), 1);
    assert_eq!(peer.acks[0].sequence, connection.journal_progress().handled);
    while connection.take_content().unwrap().is_some() {}
    assert!(!connection.io_interest().unwrap().immediate);
}

#[test]
fn zero_budget_retains_debt_and_wait_services_it_before_returning() {
    let (mut connection, mut peer) = ready();
    let sequence = deliver(&mut connection, &mut peer);
    connection.dispatch(Instant::now(), 0).unwrap();
    peer.pump();
    assert!(peer.acks.is_empty());
    assert!(connection.io_interest().unwrap().immediate);
    // The retained inbox prevents blocking, but the leading drive must flush debt.
    connection.wait_for_io(Duration::from_secs(30)).unwrap();
    peer.pump();
    assert_eq!(peer.acks.len(), 1);
    assert_eq!(peer.acks[0].sequence, sequence);
}

#[test]
fn explicit_ack_and_manual_mode_respect_the_handoff() {
    let (mut connection, mut peer) = ready();
    deliver(&mut connection, &mut peer);
    connection.set_automatic_journal_ack(false).unwrap();
    connection.poll_io().unwrap();
    peer.pump();
    assert!(peer.acks.is_empty());
    assert!(connection.acknowledge_journal().unwrap());
    connection.poll_io().unwrap();
    peer.pump();
    assert_eq!(peer.acks.len(), 1);
}

#[test]
fn close_with_debt_preserves_the_retained_event_and_reports_closed() {
    let (mut connection, mut peer) = ready();
    deliver(&mut connection, &mut peer);
    peer.close();
    connection.poll_io().unwrap();
    assert!(connection.take_content().unwrap().is_some());
    assert!(matches!(
        connection.take_content(),
        Err(ShellClientError::PeerClosed)
    ));
    assert!(matches!(
        connection.io_interest(),
        Err(ShellClientError::PeerClosed)
    ));
    assert!(!connection.journal_progress().ack_pending);
}

#[test]
fn tiny_budgets_keep_partial_ack_output_owned_and_immediate() {
    let (mut connection, mut peer) = ready();
    let sequence = deliver(&mut connection, &mut peer);
    while connection.take_content().unwrap().is_some() {}
    connection.dispatch(Instant::now(), 1).unwrap();
    peer.pump();
    assert!(
        peer.acks.is_empty(),
        "one byte cannot complete an ACK request"
    );
    assert!(connection.io_interest().unwrap().immediate);
    for _ in 0..128 {
        if !peer.acks.is_empty() {
            break;
        }
        connection.dispatch(Instant::now(), 1).unwrap();
        peer.pump();
    }
    assert_eq!(peer.acks.len(), 1);
    assert_eq!(peer.acks[0].sequence, sequence);
}

#[test]
fn round_exhaustion_queues_ack_instead_of_handing_it_off() {
    let (mut connection, mut peer) = ready();
    let before = connection.journal_progress().queued;
    for _ in 0..40 {
        let event = permit(&mut peer);
        peer.push(event);
    }
    peer.flush_events();
    connection.poll_io().unwrap();
    let state = connection.journal_progress();
    assert!(state.queued > before);
    assert_eq!(state.queued, state.handled);
    assert!(
        state.ack_pending,
        "round exhaustion queues ACK before returning"
    );
    assert!(connection.io_interest().unwrap().immediate);
}

#[test]
fn ack_refusal_is_observed_after_handed_off_write_without_losing_event() {
    let (mut connection, mut peer) = ready();
    deliver(&mut connection, &mut peer);
    peer.ack_reply = Some(EINVAL);
    connection.poll_io().unwrap();
    peer.pump();
    assert_eq!(peer.acks.len(), 1);
    assert!(connection.poll_io().is_err());
    assert!(connection.take_content().unwrap().is_some());
}
