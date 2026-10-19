//! Public FileWire observations when input is deliberately left unread.
//! Zero read credit makes the boundary deterministic in both feature modes;
//! these controls do not claim to force a post-write deferral inside a drive.

use super::*;

#[test]
fn unread_final_submitted_and_eof_settle_only_when_input_is_serviced() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);
    assert!(!connection.io_interest().unwrap().writable);
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    peer.flush_events();
    peer.close();

    let now = Instant::now();
    connection.dispatch(now, 0).unwrap();
    assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
    assert!(connection.io_interest().is_ok());
    assert_eq!(connection.take_content().unwrap(), None);

    // An EOF intake itself succeeds: closure is exposed by the receive and
    // interest APIs after the final valid event has settled custody.
    connection.dispatch(now, 65536).unwrap();
    assert_eq!(connection.custody(ticket), Some(Custody::Submitted));
    assert_eq!(connection.take_content(), Err(ShellClientError::PeerClosed));
    assert_eq!(connection.io_interest(), Err(ShellClientError::PeerClosed));
    connection.close();
    assert_eq!(connection.custody(ticket), Some(Custody::Submitted));
}

#[test]
fn unread_submit_refusal_starts_backoff_at_observation_time() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    let held = submit(&mut connection, &mut peer);
    peer.answer_error(held.tag, EAGAIN);
    let queued_at = Instant::now();
    connection.dispatch(queued_at, 0).unwrap();
    assert_eq!(connection.wake_deadline(), None);
    assert_eq!(connection.custody(ticket), Some(Custody::InFlight));

    let observed_at = queued_at + Duration::from_millis(100);
    connection.dispatch(observed_at, 65536).unwrap();
    let retry_at = observed_at + RETRY_FIRST;
    assert_eq!(connection.wake_deadline(), Some(retry_at));
    peer.pump();
    assert!(peer.submits.is_empty());
    connection
        .dispatch(retry_at - Duration::from_millis(1), 65536)
        .unwrap();
    peer.pump();
    assert!(peer.submits.is_empty());
    connection.dispatch(retry_at, 65536).unwrap();
    peer.pump();
    assert_eq!(peer.submits.pop_front().unwrap().data, held.data);
    assert_eq!(peer.transactions.len(), 1);
}

#[test]
fn unread_ack_reply_keeps_queued_and_confirmed_sequences_distinct() {
    let (mut connection, mut peer) = connect(bar(0));
    connection.set_automatic_journal_ack(false).unwrap();
    let event = permit(&mut peer);
    peer.push(event);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().handled == 3
    })
    .unwrap();
    peer.hold_ack_replies = true;
    assert!(connection.acknowledge_journal().unwrap());
    drive(&mut connection, &mut peer, |_, p| {
        !p.held_replies.is_empty()
    })
    .unwrap();
    peer.release_replies();

    let now = Instant::now();
    connection.dispatch(now, 0).unwrap();
    let pending = connection.journal_progress();
    assert_eq!((pending.queued, pending.acknowledged), (3, 2));
    assert!(pending.ack_pending);
    connection.dispatch(now, 65536).unwrap();
    let confirmed = connection.journal_progress();
    assert_eq!((confirmed.queued, confirmed.acknowledged), (3, 3));
    assert!(!confirmed.ack_pending);
    peer.pump();
    assert_eq!(
        peer.acks.iter().map(|ack| ack.sequence).collect::<Vec<_>>(),
        [3]
    );
}
