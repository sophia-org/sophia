use super::*;

#[test]
fn backpressure_tail_and_binding_fence_preserve_the_cursor() {
    let mut files = export("descriptor", BASE | 4);
    files.uploads[0] = Some(Binding {
        qid: 99,
        transaction: TransactionId::from_raw(1),
        grant: ContentGrant {
            connection_epoch: EPOCH,
            content_grant_epoch: 1,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        layout: ContentResourceLayout {
            row_bytes: 4,
            rows_per_chunk: 1,
            chunk_count: 4,
            total_bytes: 16,
        },
        admitted: true,
        closing: false,
        writer: Some(1),
        passed: 0,
        ordinal: 0,
        scratch: Vec::with_capacity(4),
    });
    for _ in 0..INBOUND_RECORDS {
        files
            .inbound
            .push_back(Inbound::Descriptor(Box::new(candidate())));
    }
    let pixel = [0, 0, 0, 255];
    assert_eq!(files.write_upload(0, 99, 0, &pixel), Err(Errno::EAGAIN));
    for offset in [4, 8, 12] {
        assert_eq!(
            files.write_upload(0, 99, offset, &pixel),
            Err(Errno::EINVAL)
        );
    }
    assert_eq!(files.uploads[0].as_ref().unwrap().cursor(), 0);
    assert_eq!(files.inbound.len(), INBOUND_RECORDS);

    files.inbound.clear();
    assert_eq!(files.write_upload(0, 99, 0, &pixel), Ok(4));
    assert_eq!(files.inbound.len(), 1);
    // A terminal binding wins over the stale-offset rule for queued tails.
    files.uploads[0].as_mut().unwrap().closing = true;
    for offset in [4, 8, 12] {
        assert_eq!(
            files.write_upload(0, 99, offset, &pixel),
            Err(Errno::ESTALE)
        );
    }
    assert_eq!(files.uploads[0].as_ref().unwrap().cursor(), 4);
    assert_eq!(files.inbound.len(), 1);
}
