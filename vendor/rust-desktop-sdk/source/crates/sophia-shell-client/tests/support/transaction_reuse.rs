use super::*;

pub(super) fn reusable(error: Option<u32>) -> (Result<ShellConnection, ShellClientError>, Peer) {
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let profile = bar(0);
    let thread = std::thread::spawn(move || Peer::reusable_handshake(listener, profile, error));
    let connection = ShellConnection::connect_files(&path, options(&profile));
    let peer = thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (connection, peer)
}

#[test]
fn retained_writer_rearms_after_ack_and_refusal_opens_a_fresh_writer() {
    let (connection, mut peer) = reusable(None);
    let mut connection = connection.unwrap();
    assert_eq!(peer.walk_count("transaction-reuse"), 1);
    peer.reset_logs();
    for id in 1..=3 {
        let ticket = enqueue_demand(&mut connection, id);
        commit(&mut connection, &mut peer, ticket);
    }
    assert_eq!(peer.walk_count("transaction"), 0);
    assert_eq!(peer.walk_count("transaction-reuse"), 0);
    let refused = enqueue_demand(&mut connection, 4);
    let held = submit(&mut connection, &mut peer);
    peer.answer_error(held.tag, EINVAL);
    assert_eq!(
        settle(&mut connection, &mut peer, refused),
        (Custody::Refused(EINVAL), None)
    );
    let next = enqueue_demand(&mut connection, 5);
    commit(&mut connection, &mut peer, next);
    assert_eq!(peer.walk_count("transaction-reuse"), 1);
    assert_eq!(peer.walk_count("transaction"), 0);
}

#[test]
fn retained_record_is_outstanding_before_covering_ack_reply() {
    let (connection, mut peer) = reusable(None);
    let mut connection = connection.unwrap();
    let first = enqueue_demand(&mut connection, 1);
    let second = enqueue_demand(&mut connection, 2);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);
    let covering = peer.next_sequence();
    peer.hold_ack_replies = true;
    peer.hold_transactions = true;
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    drive(&mut connection, &mut peer, |_, peer| {
        !peer.held_transactions.is_empty()
    })
    .unwrap();
    assert_eq!(connection.custody(first), Some(Custody::Submitted));
    assert_eq!(peer.acks.last().unwrap().sequence, covering);
    assert_eq!(
        peer.held_replies.len(),
        1,
        "covering ACK remains unanswered"
    );
    assert_eq!(
        peer.held_transactions.len(),
        1,
        "record is also outstanding"
    );
    assert_eq!(
        peer.submits.len(),
        1,
        "submit shares the ACK and record flush"
    );
    let record = peer.held_transactions.pop_front().unwrap();
    peer.release_replies();
    peer.answer_write(record.tag, record.data.len() as u32);
    peer.hold_ack_replies = false;
    peer.hold_transactions = false;
    commit(&mut connection, &mut peer, second);
}

#[test]
fn only_missing_reusable_node_selects_legacy() {
    for error in [5, 13, 116] {
        let (connection, peer) = reusable(Some(error));
        assert!(connection.is_err());
        assert_eq!(
            peer.walk_count("transaction"),
            1,
            "only negotiation uses legacy"
        );
        assert_eq!(peer.walk_count("transaction-reuse"), 1);
    }
}

#[test]
fn paired_submit_is_issued_before_record_reply_and_reconciles_reverse_order() {
    for reuse in [false, true] {
        let (mut connection, mut peer) = if reuse {
            let (connection, peer) = reusable(None);
            (connection.unwrap(), peer)
        } else {
            connect(bar(0))
        };
        peer.hold_transactions = true;
        let ticket = enqueue_demand(&mut connection, 1);
        let submit = submit(&mut connection, &mut peer);
        let record = peer
            .held_transactions
            .pop_front()
            .expect("record reply remains held");
        let (id, kind) = staged(&peer, &submit);
        peer.answer_write(submit.tag, submit.data.len() as u32);
        connection.poll_io().unwrap();
        assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
        peer.answer_write(record.tag, record.data.len() as u32);
        let event = peer.submitted(id, kind);
        peer.push(event);
        assert_eq!(
            settle(&mut connection, &mut peer, ticket),
            (Custody::Submitted, None)
        );
    }
}

#[test]
fn failed_paired_record_drains_submit_without_retry_and_preserves_acceptance() {
    for reuse in [false, true] {
        for errno in [0, EAGAIN, EINVAL, 116, EALREADY] {
            let (mut connection, mut peer) = if reuse {
                let (connection, peer) = reusable(None);
                (connection.unwrap(), peer)
            } else {
                connect(bar(0))
            };
            peer.hold_transactions = true;
            let ticket = enqueue_demand(&mut connection, 1);
            let held = submit(&mut connection, &mut peer);
            let record = peer.held_transactions.pop_front().unwrap();
            peer.answer_write(record.tag, record.data.len() as u32 - 1);
            connection.poll_io().unwrap();
            assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
            if errno == 0 {
                peer.answer_write(held.tag, held.data.len() as u32);
            } else {
                peer.answer_error(held.tag, errno);
            }
            let (custody, error) = settle(&mut connection, &mut peer, ticket);
            assert!(error.is_some());
            assert_eq!(
                custody,
                match errno {
                    0 => Custody::Submitted,
                    EALREADY => Custody::Unknown,
                    _ => Custody::DroppedUnsent,
                }
            );
            assert!(
                peer.submits.is_empty(),
                "a failed record must never retry submit"
            );
        }
    }
}

#[test]
fn early_submitted_survives_a_failed_pair() {
    for reuse in [false, true] {
        let (mut connection, mut peer) = if reuse {
            let (connection, peer) = reusable(None);
            (connection.unwrap(), peer)
        } else {
            connect(bar(0))
        };
        peer.hold_transactions = true;
        let ticket = enqueue_demand(&mut connection, 1);
        let held = submit(&mut connection, &mut peer);
        let record = peer.held_transactions.pop_front().unwrap();
        let (id, kind) = staged(&peer, &held);
        peer.begin_batch();
        peer.answer_error(record.tag, EINVAL);
        peer.answer_error(held.tag, EAGAIN);
        let event = peer.submitted(id, kind);
        peer.push(event);
        peer.end_batch();
        let (custody, error) = settle(&mut connection, &mut peer, ticket);
        assert_eq!(custody, Custody::Submitted);
        assert!(error.is_some());
        assert!(peer.submits.is_empty());
    }
}

#[test]
fn accepted_submit_survives_disconnect_without_record_reply_or_journal() {
    for reuse in [false, true] {
        for record_replied in [false, true] {
            let (mut connection, mut peer) = if reuse {
                let (connection, peer) = reusable(None);
                (connection.unwrap(), peer)
            } else {
                connect(bar(0))
            };
            peer.hold_transactions = true;
            let ticket = enqueue_demand(&mut connection, 1);
            let held = submit(&mut connection, &mut peer);
            let record = peer.held_transactions.pop_front().unwrap();
            if record_replied {
                peer.answer_write(record.tag, record.data.len() as u32);
            }
            peer.answer_write(held.tag, held.data.len() as u32);
            connection.poll_io().unwrap();
            assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
            peer.close();
            assert_eq!(
                settle(&mut connection, &mut peer, ticket).0,
                Custody::Submitted
            );
        }
    }
}
