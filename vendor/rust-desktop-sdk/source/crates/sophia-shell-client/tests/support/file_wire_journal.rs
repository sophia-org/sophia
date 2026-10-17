use super::*;

#[test]
fn retained_heads_hold_delivery_and_ack_until_explicit_consumption() {
    let (mut connection, mut peer) = connect(bar(0));
    connection.set_automatic_event_delivery(false).unwrap();
    for _ in 0..2 {
        let event = permit(&mut peer);
        peer.push(event);
    }
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_event().is_some()
    })
    .unwrap();
    for _ in 0..8 {
        connection.poll_io().unwrap();
        peer.pump();
        let event = connection.journal_event().unwrap();
        assert_eq!(event.sequence, 3);
        assert!(matches!(
            event.record,
            sophia_shell_client::JournalRecord::Content(_, ShellContentRecord::FramePermit(_))
        ));
        assert_eq!(connection.journal_progress().ack_limit, 2);
    }
    assert!(connection.set_automatic_event_delivery(true).is_err());
    assert!(connection.take_content().unwrap().is_none());
    assert!(connection.consume_journal_event().unwrap());
    assert!(!connection.consume_journal_event().unwrap());
    assert!(connection.journal_event().is_none());
    assert_eq!(connection.journal_progress().ack_limit, 2);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 3 && c.journal_event().is_some()
    })
    .unwrap();
    assert_eq!(connection.journal_event().unwrap().sequence, 4);
    connection.close();
    assert_eq!(connection.journal_event().unwrap().sequence, 4);
    assert!(connection.consume_journal_event().is_err());
}

#[test]
fn retained_publication_can_be_fetched_before_consumption_without_ack() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    connection.set_automatic_event_delivery(false).unwrap();
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
        c.journal_event().is_some()
    })
    .unwrap();
    let sophia_shell_client::JournalRecord::ObjectPublished(request) =
        connection.journal_event().unwrap().record
    else {
        panic!("expected publication")
    };
    let request = *request;
    assert!(connection.pending_snapshot().is_none());
    connection.prefetch_snapshot(request).unwrap();
    next(&mut connection, &mut peer, ShellConnection::take_indicators);
    assert_eq!(connection.journal_event().unwrap().sequence, 3);
    assert_eq!(connection.journal_progress().ack_limit, 2);
    connection.consume_journal_event().unwrap();
    connection.set_automatic_event_delivery(true).unwrap();
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 3
    })
    .unwrap();
    assert!(!connection.journal_progress().object_pending);
    assert_eq!(peer.walk_count("indicators"), 1);
    let event = permit(&mut peer);
    peer.push(event);
    next(&mut connection, &mut peer, ShellConnection::take_content);
    assert!(connection.journal_event().is_none());
}

#[test]
fn explicit_fetch_failures_retain_identity_and_allow_retry_without_acknowledging() {
    for fault in 0..8 {
        let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
        connection.set_automatic_snapshot_fetch(false).unwrap();
        let request = ShellFileObjectPublished {
            object: ShellFileKind::Indicators,
            generation: 1,
            qid: 5,
        };
        let mut bytes = indicators_object(
            if fault == 2 { EPOCH + 1 } else { EPOCH },
            if fault == 1 { 2 } else { 1 },
        );
        if fault == 3 {
            bytes.push(0);
        }
        if fault == 4 {
            bytes.resize(SHELL_FILE_INDICATORS_MAX_BYTES + 1, 0);
        }
        peer.object_read_error = match fault {
            5 => Some(5),   // EIO
            6 => Some(11),  // EAGAIN exhausts bounded restarts
            7 => Some(116), // ESTALE exhausts bounded restarts
            _ => None,
        };
        publish(
            &mut peer,
            "indicators",
            if fault == 0 { 6 } else { 5 },
            bytes,
            17,
        );
        let event = peer.published(request.object, request.generation, request.qid);
        peer.push(event);
        drive(&mut connection, &mut peer, |c, _| {
            c.pending_snapshot().is_some()
        })
        .unwrap();
        connection.request_snapshot(request).unwrap();
        drive(&mut connection, &mut peer, |c, _| {
            c.snapshot_failure().is_some()
        })
        .unwrap();
        assert_eq!(connection.snapshot_failure().unwrap().request, request);
        assert_eq!(connection.journal_progress().ack_limit, 2);
        assert_eq!(
            connection.request_snapshot(request),
            Err(ShellClientError::QueueSaturated)
        );
        assert!(connection.take_indicators().unwrap().is_none());
        assert!(connection.take_snapshot_failure().is_some());
        assert!(connection.take_snapshot_failure().is_none());
        peer.object_read_error = None;
        publish(&mut peer, "indicators", 5, indicators_object(EPOCH, 1), 17);
        connection.request_snapshot(request).unwrap();
        let (_, value) = next(&mut connection, &mut peer, ShellConnection::take_indicators);
        assert_eq!(value, indicator_snapshot(1));
        drive(&mut connection, &mut peer, |c, _| {
            c.journal_progress().acknowledged == 3
        })
        .unwrap();
        assert!(connection.snapshot_failure().is_none());
    }
}

#[test]
fn prefetched_identity_discharges_only_the_matching_publication() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    connection.set_automatic_snapshot_fetch(false).unwrap();
    publish(
        &mut peer,
        "indicators",
        5,
        indicators_object(EPOCH, 1),
        usize::MAX,
    );
    let request = ShellFileObjectPublished {
        object: ShellFileKind::Indicators,
        generation: 1,
        qid: 5,
    };
    assert!(
        connection
            .prefetch_snapshot(ShellFileObjectPublished {
                object: ShellFileKind::Descriptors,
                ..request
            })
            .is_err()
    );
    assert!(connection.prefetch_snapshot(request).unwrap());
    assert!(!connection.prefetch_snapshot(request).unwrap());
    assert_eq!(
        connection.prefetch_snapshot(ShellFileObjectPublished { qid: 6, ..request }),
        Err(ShellClientError::QueueSaturated)
    );
    let (_, value) = next(&mut connection, &mut peer, ShellConnection::take_indicators);
    assert_eq!(value, indicator_snapshot(1));
    let event = peer.published(ShellFileKind::Indicators, 1, 5);
    peer.push(event);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 3
    })
    .unwrap();
    assert_eq!(peer.walk_count("indicators"), 1);
    assert!(connection.pending_snapshot().is_none());
    let event = peer.published(ShellFileKind::Indicators, 1, 6);
    peer.push(event);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().handled == 4
    })
    .unwrap();
    assert_eq!(connection.journal_progress().ack_limit, 3);
    assert_eq!(connection.pending_snapshot().unwrap().qid, 6);
}

#[test]
fn choosing_a_later_feed_does_not_acknowledge_the_earlier_unfetched_feed() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    connection.set_automatic_snapshot_fetch(false).unwrap();
    let output_bytes = encode_shell_file_outputs(
        ShellFileHeader {
            kind: ShellFileKind::Outputs,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: 0,
        },
        &ShellFileTransactionRecord {
            transaction: tx(1),
            record: ShellContentRecord::OutputFacts(ContentOutputFacts {
                grant: grant(),
                facts_generation: 1,
                outputs: Vec::new(),
            }),
        },
    )
    .unwrap();
    publish(&mut peer, "outputs", 4, output_bytes, usize::MAX);
    publish(
        &mut peer,
        "indicators",
        5,
        indicators_object(EPOCH, 1),
        usize::MAX,
    );
    for (kind, qid) in [(ShellFileKind::Outputs, 4), (ShellFileKind::Indicators, 5)] {
        let event = peer.published(kind, 1, qid);
        peer.push(event);
    }
    drive(&mut connection, &mut peer, |c, _| {
        c.pending_snapshots().count() == 2
    })
    .unwrap();
    let requests: Vec<_> = connection.pending_snapshots().collect();
    let indicators = requests
        .iter()
        .find(|r| r.object == ShellFileKind::Indicators)
        .copied()
        .unwrap();
    let outputs = requests
        .iter()
        .find(|r| r.object == ShellFileKind::Outputs)
        .copied()
        .unwrap();
    connection.request_snapshot(indicators).unwrap();
    next(&mut connection, &mut peer, ShellConnection::take_indicators);
    assert_eq!(connection.journal_progress().ack_limit, 2);
    assert_eq!(peer.walk_count("outputs"), 0);
    connection.request_snapshot(outputs).unwrap();
    next(&mut connection, &mut peer, ShellConnection::take_content);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 4
    })
    .unwrap();
    assert_eq!(connection.pending_snapshots().count(), 0);
}

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
