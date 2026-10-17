use super::*;

#[test]
fn snapshot_fetch_waits_for_exact_request_or_automatic_resumption() {
    for automatic in [false, true] {
        let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
        connection.set_automatic_snapshot_fetch(false).unwrap();
        publish(
            &mut peer,
            "indicators",
            5,
            indicators_object(EPOCH, 1),
            usize::MAX,
        );
        let event = peer.published(ShellFileKind::Indicators, 1, 5);
        peer.push(event);
        drive(&mut connection, &mut peer, |c, _| {
            c.pending_snapshot().is_some()
        })
        .unwrap();
        let request = connection.pending_snapshot().unwrap();
        for _ in 0..4 {
            connection.poll_io().unwrap();
            peer.pump();
        }
        assert_eq!(peer.walk_count("indicators"), 0);
        assert!(connection.journal_progress().object_pending);
        assert_eq!(connection.journal_progress().ack_limit, 2);
        assert!(
            connection
                .request_snapshot(ShellFileObjectPublished {
                    generation: 2,
                    ..request
                })
                .is_err()
        );
        assert_eq!(connection.pending_snapshot(), Some(request));
        if automatic {
            connection.set_automatic_snapshot_fetch(true).unwrap();
        } else {
            assert!(connection.request_snapshot(request).unwrap());
            assert!(!connection.request_snapshot(request).unwrap());
        }
        peer.pump();
        assert_eq!(peer.walk_count("indicators"), 0, "request performed I/O");
        let (_, snapshot) = next(&mut connection, &mut peer, ShellConnection::take_indicators);
        assert_eq!(snapshot, indicator_snapshot(1));
        assert!(connection.pending_snapshot().is_none());
        assert!(connection.request_snapshot(request).is_err());
        connection.close();
        assert!(connection.pending_snapshot().is_none());
        assert!(connection.set_automatic_snapshot_fetch(true).is_err());
    }
}

#[test]
fn manual_ack_cannot_skip_an_unverified_object() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    connection.set_automatic_journal_ack(false).unwrap();
    publish(
        &mut peer,
        "indicators",
        5,
        indicators_object(EPOCH, 1),
        usize::MAX,
    );
    peer.hold_object_reads = true;
    let event = peer.published(ShellFileKind::Indicators, 1, 5);
    peer.push(event);
    drive(&mut connection, &mut peer, |_, p| !p.opens.is_empty()).unwrap();
    for _ in 0..4 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    let progress = connection.journal_progress();
    assert!(progress.object_pending);
    assert_eq!((progress.handled, progress.ack_limit), (3, 2));
    assert!(!connection.acknowledge_journal().unwrap());
    assert!(peer.acks.is_empty());
    peer.release_object_reads();
    let (_, snapshot) = next(&mut connection, &mut peer, ShellConnection::take_indicators);
    assert_eq!(snapshot, indicator_snapshot(1));
    assert!(!connection.journal_progress().object_pending);
    assert!(connection.acknowledge_journal().unwrap());
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 3
    })
    .unwrap();
}

#[test]
fn manual_ack_waits_for_the_caller_and_distinguishes_queued_from_confirmed() {
    let (mut connection, mut peer) = connect(bar(0));
    connection.set_automatic_journal_ack(false).unwrap();
    assert_eq!(connection.journal_progress().acknowledged, 2);
    let event = permit(&mut peer);
    peer.push(event);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().handled == 3
    })
    .unwrap();
    assert!(peer.acks.is_empty());
    assert_eq!(connection.journal_progress().ack_limit, 3);
    assert!(connection.acknowledge_journal().unwrap());
    assert!(!connection.acknowledge_journal().unwrap());
    peer.pump();
    assert!(peer.acks.is_empty(), "queueing performed socket I/O");
    assert_eq!(connection.journal_progress().queued, 3);
    assert_eq!(connection.journal_progress().acknowledged, 2);
    peer.hold_ack_replies = true;
    drive(&mut connection, &mut peer, |_, p| {
        !p.held_replies.is_empty()
    })
    .unwrap();
    assert!(connection.journal_progress().ack_pending);
    assert_eq!(connection.journal_progress().acknowledged, 2);
    peer.release_replies();
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 3
    })
    .unwrap();
    assert!(!connection.journal_progress().ack_pending);
    let event = permit(&mut peer);
    peer.push(event);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().handled == 4
    })
    .unwrap();
    assert_eq!(peer.acks.len(), 1);
    peer.hold_ack_replies = false;
    connection.set_automatic_journal_ack(true).unwrap();
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 4
    })
    .unwrap();
    connection.close();
    assert!(connection.acknowledge_journal().is_err());
    assert!(connection.set_automatic_journal_ack(true).is_err());
    assert_eq!(connection.journal_progress().acknowledged, 4);
    assert!(!connection.journal_progress().ack_pending);
}
