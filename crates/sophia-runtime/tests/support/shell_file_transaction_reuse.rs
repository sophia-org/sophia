use super::*;

fn assembly(id: u64) -> (Vec<u8>, Vec<u8>) {
    let value = candidate();
    let bytes = encode_shell_file_descriptor(
        ShellFileHeader {
            kind: shell_file_descriptor_kind(&value.record),
            connection_epoch: EPOCH,
            submission_id: id,
            sequence: 0,
        },
        &value,
    )
    .unwrap();
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: EPOCH,
        submission_id: id,
        candidate_bytes: bytes.len() as u32,
    })
    .unwrap();
    (bytes, submit)
}

fn ack(files: &mut ShellFiles, sequence: u64) {
    files
        .write(
            &Node::Ack,
            &mut Handle::Plain,
            0,
            &encode_shell_file_ack(ShellFileAck {
                connection_epoch: EPOCH,
                sequence,
            })
            .unwrap(),
        )
        .unwrap();
}

#[test]
fn discovery_mixed_writers_and_cancellation_preserve_one_assembly() {
    let mut files = export("descriptor", BASE | 4);
    assert_eq!(
        files.lookup(&Node::Root, WalkName::Child(b"transaction-reuse")),
        Ok(Node::TransactionReuse)
    );
    let legacy = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    assert!(matches!(
        files.open(&Node::TransactionReuse, OpenFlags(2)),
        Err(EBUSY)
    ));
    files.release(Node::Transaction, Some(legacy));
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let (bytes, _) = assembly(1);
    files
        .write(&Node::TransactionReuse, &mut writer, 0, &bytes)
        .unwrap();
    files.release(Node::TransactionReuse, Some(writer));
    assert!(files.staging.is_none() && files.accepted.is_none() && files.inbound.is_empty());
    assert!(files.open(&Node::Transaction, OpenFlags(2)).is_ok());
}

#[test]
fn backpressure_keeps_reusable_assembly_retryable() {
    let mut files = export("descriptor", BASE | 4);
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let (bytes, submit) = assembly(1);
    files
        .write(&Node::TransactionReuse, &mut writer, 0, &bytes)
        .unwrap();
    for _ in 0..INBOUND_RECORDS {
        files
            .inbound
            .push_back(Inbound::Descriptor(Box::new(candidate())));
    }
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, &submit),
        Err(Errno::EAGAIN)
    );
    assert!(files.accepted.is_none());
    assert_eq!(
        files.write(&Node::TransactionReuse, &mut writer, 0, &bytes),
        Ok(bytes.len() as u32)
    );
    files.inbound.clear();
    files
        .write(&Node::Submit, &mut Handle::Plain, 0, &submit)
        .unwrap();
    assert_eq!(files.inbound.len(), 1);
}

#[test]
fn reusable_writer_recycles_only_after_covering_ack() {
    let mut files = export("descriptor", BASE | 4);
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let mut buffer = None;
    for id in 1..=3 {
        assert!(matches!(
            files.open(&Node::Transaction, OpenFlags(2)),
            Err(EBUSY)
        ));
        assert!(matches!(
            files.open(&Node::TransactionReuse, OpenFlags(2)),
            Err(EBUSY)
        ));
        let (bytes, submit) = assembly(id);
        files
            .write(&Node::TransactionReuse, &mut writer, 0, &bytes)
            .unwrap();
        let staging = files.staging.as_ref().unwrap();
        assert!(staging.bytes.capacity() >= STAGING.max_bytes);
        if let Some(pointer) = buffer {
            assert_eq!(staging.bytes.as_ptr(), pointer);
        }
        buffer = Some(staging.bytes.as_ptr());
        files
            .write(&Node::Submit, &mut Handle::Plain, 0, &submit)
            .unwrap();
        files
            .write(&Node::Submit, &mut Handle::Plain, 0, &submit)
            .unwrap();
        assert_eq!(files.inbound.len(), 1);
        files.take_inbound().unwrap();
        let ReadOutcome::Ready(retained) = files
            .read(&Node::TransactionReuse, &mut writer, 0, u32::MAX)
            .unwrap()
        else {
            panic!("accepted bytes")
        };
        assert_eq!(retained, bytes);
        assert_eq!(
            files.describe(&Node::TransactionReuse, Some(&writer)).size,
            bytes.len() as u64
        );
        if id > 1 {
            ack(&mut files, id - 1);
        }
        for data in [&bytes[..], &[]] {
            assert_eq!(
                files.write(&Node::TransactionReuse, &mut writer, 0, data),
                Err(EBUSY)
            );
        }
        ack(&mut files, id);
        assert_eq!(
            files.describe(&Node::TransactionReuse, Some(&writer)).size,
            0
        );
        assert_eq!(
            files.write(&Node::TransactionReuse, &mut writer, 0, &[]),
            Ok(0)
        );
        assert!(files.staging.is_none());
    }
}

#[test]
fn invalid_idle_writes_and_submit_envelopes_do_not_start_or_poison_assembly() {
    let mut files = export("descriptor", BASE | 4);
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let (bytes, submit) = assembly(1);
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, &submit),
        Err(Errno::ESTALE)
    );
    assert_eq!(
        files.write(&Node::TransactionReuse, &mut writer, 0, &[0; 4]),
        Err(Errno::EINVAL)
    );
    assert!(files.staging.is_none());
    assert!(
        matches!(files.read(&Node::TransactionReuse, &mut writer, 0, 64), Ok(ReadOutcome::Ready(bytes)) if bytes.is_empty())
    );
    files
        .write(&Node::TransactionReuse, &mut writer, 0, &bytes)
        .unwrap();
    let foreign = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: EPOCH + 1,
        submission_id: 1,
        candidate_bytes: bytes.len() as u32,
    })
    .unwrap();
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, &foreign),
        Err(Errno::ESTALE)
    );
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, b"bad"),
        Err(Errno::EINVAL)
    );
    files
        .write(&Node::Submit, &mut Handle::Plain, 0, &submit)
        .unwrap();
    assert_eq!(files.inbound.len(), 1);
}

#[test]
fn refusal_retains_bytes_and_expiry_permanently_fences_token() {
    let mut files = export("descriptor", BASE | 4);
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let (bytes, _) = assembly(1);
    assert_eq!(
        files.write(&Node::TransactionReuse, &mut writer, 1, &bytes),
        Err(Errno::EINVAL)
    );
    assert!(files.staging.is_none());
    files
        .write(&Node::TransactionReuse, &mut writer, 0, &bytes)
        .unwrap();
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, b"bad"),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        files.write(&Node::TransactionReuse, &mut writer, 0, &bytes),
        Ok(bytes.len() as u32)
    );
    let wrong_length = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: EPOCH,
        submission_id: 1,
        candidate_bytes: 32,
    })
    .unwrap();
    for _ in 0..2 {
        assert_eq!(
            files.write(&Node::Submit, &mut Handle::Plain, 0, &wrong_length),
            Err(Errno::EINVAL)
        );
    }
    assert_eq!(
        files.write(&Node::TransactionReuse, &mut writer, 0, &[]),
        Err(EBUSY)
    );
    assert!(
        files
            .read(&Node::TransactionReuse, &mut writer, 0, 64)
            .is_ok()
    );
    let old = files.staging.take().unwrap();
    let mut expired = Staging::new(old.handle, STAGING);
    expired
        .write(
            0,
            &bytes,
            Instant::now() - STAGING.assembly - Duration::from_secs(1),
        )
        .unwrap();
    files.staging = Some(expired);
    files.expire();
    let mut successor = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    assert_eq!(
        files.write(&Node::TransactionReuse, &mut writer, 0, &bytes),
        Err(Errno::ESTALE)
    );
    assert!(matches!(
        files.read(&Node::TransactionReuse, &mut writer, 0, 64),
        Err(Errno::ESTALE)
    ));
    files.release(Node::TransactionReuse, Some(writer));
    assert!(
        files
            .write(&Node::TransactionReuse, &mut successor, 0, &bytes)
            .is_ok()
    );
}

#[test]
fn clunk_preserves_accepted_custody_and_revoke_fences_writer() {
    let mut files = export("descriptor", BASE | 4);
    for flags in [0, 1, 2 | 0x200, 2 | 0x400] {
        assert!(matches!(
            files.open(&Node::TransactionReuse, OpenFlags(flags)),
            Err(Errno::EACCES)
        ));
    }
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let (bytes, submit) = assembly(1);
    files
        .write(&Node::TransactionReuse, &mut writer, 0, &bytes)
        .unwrap();
    files
        .write(&Node::Submit, &mut Handle::Plain, 0, &submit)
        .unwrap();
    files.release(Node::TransactionReuse, Some(writer));
    assert!(files.accepted.is_some());
    assert!(matches!(
        files.open(&Node::TransactionReuse, OpenFlags(2)),
        Err(EBUSY)
    ));
    ack(&mut files, 1);
    let mut next = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    files.revoke();
    assert_eq!(
        files.write(&Node::TransactionReuse, &mut next, 0, &bytes),
        Err(Errno::ESTALE)
    );
}
