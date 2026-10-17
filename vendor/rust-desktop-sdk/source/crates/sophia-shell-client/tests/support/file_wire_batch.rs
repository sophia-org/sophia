use super::*;

#[test]
fn all_ack_batches_keep_control_headroom_but_mixed_batches_cannot_spend_it() {
    let (mut connection, _) = connect(bar(0));
    for id in 1..=32 {
        enqueue_demand(&mut connection, id);
    }
    let ack = ShellContentRecord::ActionAck(ack(&action()));
    let demand = demand(33);
    assert_eq!(
        connection.enqueue_content_batch(&[
            ContentBatchItem::Record(tx(40), &ack),
            ContentBatchItem::Record(tx(41), &demand)
        ]),
        Err(ShellClientError::QueueSaturated)
    );
    assert_eq!(connection.custody(Ticket(33)), None);
    let admission = connection
        .enqueue_content_batch(&[ContentBatchItem::Record(tx(40), &ack); 2])
        .unwrap();
    assert_eq!(
        admission,
        Admission {
            first: Ticket(33),
            count: 2
        }
    );
}

#[test]
fn batch_total_bytes_are_bounded_before_admission() {
    let (mut connection, mut peer) = connect(bar(0));
    let chunk = ShellContentRecord::ResourceChunk(ContentResourceChunk {
        grant: grant(),
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        ordinal: 0,
        offset: 0,
        bytes: vec![1; 1024 * 1024 + 1],
    });
    assert_eq!(
        connection.enqueue_content_batch(&[ContentBatchItem::Record(tx(1), &chunk); 2]),
        Err(ShellClientError::QueueSaturated)
    );
    assert_eq!(connection.custody(Ticket(1)), None);
    let ticket = enqueue_demand(&mut connection, 2);
    assert_eq!(ticket, Ticket(1));
    let bytes = commit(&mut connection, &mut peer, ticket);
    assert_eq!(kind_and_id(&bytes).0, ShellFileKind::FrameDemand);
}

#[test]
fn batch_preserves_operation_order_transactions_and_independent_tickets() {
    let (mut connection, mut peer) = connect(bar(0));
    let first = demand(1);
    let last = demand(3);
    let candidate = admission::withdrawal();
    let items = [
        ContentBatchItem::Record(tx(10), &first),
        ContentBatchItem::Candidate(tx(20), &candidate),
        ContentBatchItem::Record(tx(30), &last),
    ];
    let admitted = connection.enqueue_content_batch(&items).unwrap();
    assert_eq!(
        admitted,
        Admission {
            first: Ticket(1),
            count: 3
        }
    );
    peer.pump();
    assert_eq!(peer.walk_count("transaction"), 0);
    let bytes = commit(&mut connection, &mut peer, Ticket(1));
    assert_eq!(
        decode_shell_file_transaction(&bytes, ShellFileKind::FrameDemand)
            .unwrap()
            .transaction,
        tx(10)
    );
    let bytes = commit(&mut connection, &mut peer, Ticket(2));
    assert_eq!(
        decode_shell_file_candidate(&bytes).unwrap().transaction,
        tx(20)
    );
    let bytes = commit(&mut connection, &mut peer, Ticket(3));
    assert_eq!(
        decode_shell_file_transaction(&bytes, ShellFileKind::FrameDemand)
            .unwrap()
            .transaction,
        tx(30)
    );
}

#[test]
fn invalid_middle_or_full_queue_admits_none_and_reserved_capacity_survives() {
    let (mut connection, mut peer) = connect(bar(0));
    let record = demand(1);
    let invalid = permit_record();
    let good = [ContentBatchItem::Record(tx(1), &record); 3];
    let bad = [good[0], ContentBatchItem::Record(tx(2), &invalid), good[2]];
    let token = connection.reserve_capacity(3, 4096).unwrap();
    assert!(
        connection
            .commit_reserved_content_batch(&token, &bad)
            .is_err()
    );
    assert_eq!(connection.custody(Ticket(1)), None);
    let admitted = connection
        .commit_reserved_content_batch(&token, &good)
        .unwrap();
    assert_eq!(admitted.count, 3);
    for ticket in admitted.tickets() {
        commit(&mut connection, &mut peer, ticket);
    }
    for id in 0..30 {
        enqueue_demand(&mut connection, id + 10);
    }
    assert_eq!(
        connection.enqueue_content_batch(&good),
        Err(ShellClientError::QueueSaturated)
    );
    assert_eq!(connection.custody(Ticket(34)), None);
}

#[test]
fn batch_count_and_reservation_size_are_checked_before_admission() {
    let (mut connection, _) = connect(bar(0));
    let record = demand(1);
    let items = [ContentBatchItem::Record(tx(1), &record); 65];
    assert!(matches!(
        connection.enqueue_content_batch(&[]),
        Err(ShellClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        connection.enqueue_content_batch(&items),
        Err(ShellClientError::InvalidArgument(_))
    ));
    let token = connection.reserve_capacity(1, 4096).unwrap();
    assert!(matches!(
        connection.commit_reserved_content_batch(&token, &items[..2]),
        Err(ShellClientError::InvalidArgument(_))
    ));
    assert_eq!(connection.custody(Ticket(1)), None);
    assert_eq!(
        connection
            .commit_reserved_content_batch(&token, &items[..1])
            .unwrap()
            .first,
        Ticket(1)
    );
}

fn begin(id: u64) -> ShellContentRecord {
    ShellContentRecord::ResourceBegin(ContentResourceBegin {
        grant: grant(),
        resource: ContentResourceId { id, generation: 1 },
        width_px: 4,
        height_px: 4,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 64,
    })
}

#[test]
fn upload_slots_are_distinct_and_roll_back_on_failure_or_cancellation() {
    let (mut connection, mut peer) = connect(Profile {
        limits_published: true,
        ..bar(0)
    });
    let _ = next(&mut connection, &mut peer, ShellConnection::take_content);
    let first = begin(1);
    let second = begin(2);
    let invalid = permit_record();
    let good = [
        ContentBatchItem::Record(tx(1), &first),
        ContentBatchItem::Record(tx(2), &second),
    ];
    let bad = [good[0], good[1], ContentBatchItem::Record(tx(3), &invalid)];
    for _ in 0..8 {
        assert!(connection.enqueue_content_batch(&bad).is_err());
        assert!(
            connection
                .enqueue_content_batch(&[good[0], good[0]])
                .is_err()
        );
        drop(connection.reserve_content_batch(&good).unwrap());
    }
    let admission = connection.enqueue_content_batch(&good).unwrap();
    assert_eq!(
        admission,
        Admission {
            first: Ticket(1),
            count: 2
        }
    );
    let a = commit(&mut connection, &mut peer, Ticket(1));
    let b = commit(&mut connection, &mut peer, Ticket(2));
    assert_eq!(decode_shell_file_resource_begin(&a).unwrap().slot, 0);
    assert_eq!(decode_shell_file_resource_begin(&b).unwrap().slot, 1);
}

#[test]
fn a_batch_can_have_submitted_unknown_and_unsent_outcomes() {
    let (mut connection, mut peer) = connect(bar(0));
    let record = demand(1);
    connection
        .enqueue_content_batch(&[ContentBatchItem::Record(tx(1), &record); 3])
        .unwrap();
    commit(&mut connection, &mut peer, Ticket(1));
    let _ = submit(&mut connection, &mut peer);
    peer.close();
    let _ = connection.poll_io();
    assert_eq!(connection.custody(Ticket(1)), Some(Custody::Submitted));
    assert_eq!(connection.custody(Ticket(2)), Some(Custody::Unknown));
    assert_eq!(connection.custody(Ticket(3)), Some(Custody::DroppedUnsent));
    assert_eq!(connection.custody_result(Ticket(1)), Ok(Custody::Submitted));
    assert_eq!(connection.custody_result(Ticket(2)), Ok(Custody::Unknown));
    assert_eq!(
        connection.custody_result(Ticket(3)),
        Ok(Custody::DroppedUnsent)
    );
    assert_eq!(
        connection.custody_result(Ticket(4)),
        Err(sophia_shell_client::CustodyLookupError::Unissued)
    );
}
