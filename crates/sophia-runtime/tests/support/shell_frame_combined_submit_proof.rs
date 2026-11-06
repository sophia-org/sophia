//! Test-only combined-call proof. No file node, SDK discovery, or wire change.
//! Transport authentication and a negotiated single-Twrite size limit are outside this seam.
use super::*;

fn combined(files: &mut ShellFiles, writer: &mut Handle, bytes: &[u8]) -> Result<u32, Errno> {
    let Handle::ReusableTransaction(id) = *writer else {
        return Err(Errno::ESTALE);
    };
    // Expire and validate the existing token even for an accepted retry.
    files.read_reusable_transaction(id, 0, 0)?;
    if files.submission_revision() != SHELL_FRAME_REVISION {
        return Err(Errno::EACCES);
    }
    let record = decode_shell_file_record_at_revision(
        bytes,
        ShellFileClass::Candidate,
        SHELL_FRAME_REVISION,
    )
    .map_err(|_| Errno::EINVAL)?;
    if record.header.connection_epoch != files.epoch {
        return Err(Errno::ESTALE);
    }
    if record.header.kind != ShellFileKind::FrameCommit {
        return Err(Errno::EINVAL);
    }
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: record.header.connection_epoch,
        submission_id: record.header.submission_id,
        candidate_bytes: u32::try_from(bytes.len()).map_err(|_| Errno::EINVAL)?,
    })
    .map_err(|_| Errno::EINVAL)?;
    if let Some(accepted) = &files.accepted {
        // The submit header alone cannot distinguish a changed payload of equal length.
        if accepted.handle != id || accepted.bytes != bytes {
            return Err(EBUSY);
        }
    } else if !files
        .staging
        .as_ref()
        .is_some_and(|staging| staging.handle == id && staging.bytes == bytes)
    {
        files.write_reusable_transaction(id, 0, bytes)?;
    }
    files.submit_transaction(&submit)?;
    Ok(bytes.len() as u32)
}

fn acknowledge(files: &mut ShellFiles, sequence: u64) {
    files
        .write(
            &Node::Ack,
            &mut Handle::Plain,
            0,
            &encode_shell_file_ack(ShellFileAck {
                connection_epoch: 1,
                sequence,
            })
            .unwrap(),
        )
        .unwrap();
}

#[test]
fn combined_call_has_the_same_custody_and_journal_as_the_two_writes() {
    let mut checked = 0;
    for (name, bytes) in goldens() {
        if !name.starts_with("commit-") {
            continue;
        }
        checked += 1;
        let mut control = export();
        let (_writer, submit) = stage(&mut control, &bytes);
        assert!(control.inbound.is_empty() && control.accepted.is_none());
        control.submit_transaction(&submit).unwrap();

        let mut candidate = export();
        let mut writer = candidate
            .open(&Node::TransactionReuse, OpenFlags(2))
            .unwrap();
        assert_eq!(
            combined(&mut candidate, &mut writer, &bytes),
            Ok(bytes.len() as u32)
        );
        assert_eq!(journal_bytes(&candidate), journal_bytes(&control), "{name}");
        assert_eq!(candidate.submission_watermark, control.submission_watermark);
        assert_eq!(candidate.accepted.as_ref().unwrap().bytes, bytes);
        for _ in 0..3 {
            combined(&mut candidate, &mut writer, &bytes).unwrap();
        }
        assert_eq!(candidate.inbound.len(), 1, "retry enqueued twice: {name}");
        assert_eq!(journal_bytes(&candidate), journal_bytes(&control));
        assert_eq!(candidate.take_inbound(), control.take_inbound());
        assert!(
            candidate.owned_pending_offer().is_none(),
            "custody is not owner admission"
        );
    }
    assert_eq!(checked, 4);
}

#[test]
fn combined_backpressure_preserves_identity_and_bytes_until_retry() {
    for journal_full in [false, true] {
        let mut files = export();
        let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
        let bytes = golden("commit-copy-retire");
        if journal_full {
            files.journal = Journal::new(
                1,
                JournalBounds {
                    bytes: 32,
                    reserve_bytes: 0,
                },
                Instant::now(),
            );
        } else {
            let value =
                decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap();
            files
                .inbound
                .extend((0..INBOUND_RECORDS).map(|_| Inbound::Frame(Box::new(value.clone()))));
        }
        for _ in 0..2 {
            assert_eq!(
                combined(&mut files, &mut writer, &bytes),
                Err(Errno::EAGAIN)
            );
            assert!(files.accepted.is_none());
            assert_eq!(files.submission_watermark, 0);
            assert_eq!(files.staging.as_ref().unwrap().bytes, bytes);
        }
        let pointer = files.staging.as_ref().unwrap().bytes.as_ptr();
        if journal_full {
            files.journal = Journal::new(
                1,
                JournalBounds {
                    bytes: 65536,
                    reserve_bytes: 12288,
                },
                Instant::now(),
            );
            files.journal.enable_frame_revision();
        } else {
            files.inbound.clear();
        }
        combined(&mut files, &mut writer, &bytes).unwrap();
        assert_eq!(files.inbound.len(), 1);
        assert_eq!(files.accepted.as_ref().unwrap().bytes.as_ptr(), pointer);
    }
}

#[test]
fn combined_retry_cannot_replace_accepted_bytes_with_the_same_header() {
    let mut files = export();
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let bytes = golden("commit-copy-retire");
    combined(&mut files, &mut writer, &bytes).unwrap();
    let journal = journal_bytes(&files);
    let mut changed = bytes.clone();
    changed[32..40].copy_from_slice(&99u64.to_le_bytes());
    assert!(decode_shell_frame_record(&changed, ShellFileClass::Candidate, 13, 8192).is_ok());
    assert_eq!(combined(&mut files, &mut writer, &changed), Err(EBUSY));
    assert_eq!(files.accepted.as_ref().unwrap().bytes, bytes);
    assert_eq!(journal_bytes(&files), journal);
    assert_eq!(files.inbound.len(), 1);
}

#[test]
fn combined_call_still_requires_covering_ack_for_the_next_record() {
    let mut files = export();
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let bytes = golden("commit-copy-retire");
    combined(&mut files, &mut writer, &bytes).unwrap();
    let mut next = bytes.clone();
    next[16..24].copy_from_slice(&13u64.to_le_bytes());
    assert_eq!(combined(&mut files, &mut writer, &next), Err(EBUSY));
    acknowledge(&mut files, 1);
    assert_eq!(files.inbound.len(), 1, "receipt ACK is not owner admission");
    combined(&mut files, &mut writer, &next).unwrap();
    assert_eq!(files.inbound.len(), 2);
    assert_eq!(files.submission_watermark, 13);
    next[16..24].copy_from_slice(&14u64.to_le_bytes());
    acknowledge(&mut files, 1);
    assert_eq!(combined(&mut files, &mut writer, &next), Err(EBUSY));
    acknowledge(&mut files, 2);
    combined(&mut files, &mut writer, &next).unwrap();
    assert_eq!(files.inbound.len(), 3);
}

#[test]
fn combined_refusals_do_not_transfer_custody() {
    for fault in 0..5 {
        let mut files = export();
        let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
        let mut bytes = golden("commit-copy-retire");
        match fault {
            0 => bytes[8..16].copy_from_slice(&2u64.to_le_bytes()),
            1 => {
                bytes.pop();
            }
            2 => bytes[SHELL_FILE_HEADER_BYTES + 14] = 1,
            3 => {
                files.content_limits.as_mut().unwrap().max_candidate_bytes =
                    (bytes.len() - 1) as u32
            }
            4 => bytes = golden("open"),
            _ => unreachable!(),
        }
        for _ in 0..2 {
            assert!(combined(&mut files, &mut writer, &bytes).is_err());
            assert!(files.accepted.is_none() && files.inbound.is_empty());
            assert_eq!(files.submission_watermark, 0);
            assert!(journal_bytes(&files).is_empty());
        }
    }
}

#[test]
fn combined_replay_after_ack_is_not_a_new_submission() {
    let mut files = export();
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let bytes = golden("commit-copy-retire");
    combined(&mut files, &mut writer, &bytes).unwrap();
    acknowledge(&mut files, 1);
    assert_eq!(combined(&mut files, &mut writer, &bytes), Err(EALREADY));
    assert_eq!(files.inbound.len(), 1);
    assert!(files.accepted.is_none());
    let mut next = bytes.clone();
    next[16..24].copy_from_slice(&13u64.to_le_bytes());
    assert_eq!(combined(&mut files, &mut writer, &next), Err(EBUSY));
    assert_eq!(files.staging.as_ref().unwrap().bytes, bytes);
}

#[test]
fn combined_call_cannot_change_staged_bytes_or_complete_an_overlapping_prefix() {
    for partial in [false, true] {
        let mut files = export();
        let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
        let bytes = golden("commit-copy-retire");
        let prefix = if partial { &bytes[..64] } else { &bytes[..] };
        files
            .write(&Node::TransactionReuse, &mut writer, 0, prefix)
            .unwrap();
        let mut changed = bytes.clone();
        changed[32..40].copy_from_slice(&99u64.to_le_bytes());
        assert_eq!(
            combined(&mut files, &mut writer, &changed),
            Err(Errno::EINVAL)
        );
        assert_eq!(files.staging.as_ref().unwrap().bytes, prefix);
        assert!(files.accepted.is_none() && files.inbound.is_empty());
        if partial {
            assert_eq!(
                combined(&mut files, &mut writer, &bytes),
                Err(Errno::EINVAL)
            );
            files
                .write(&Node::TransactionReuse, &mut writer, 64, &bytes[64..])
                .unwrap();
        }
        combined(&mut files, &mut writer, &bytes).unwrap();
        assert_eq!(files.inbound.len(), 1);
    }
}

#[test]
fn combined_accepted_retry_requires_the_token_even_after_custody_survives_clunk() {
    let mut files = export();
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    let bytes = golden("commit-copy-retire");
    combined(&mut files, &mut writer, &bytes).unwrap();
    let Handle::ReusableTransaction(id) = writer else {
        unreachable!()
    };
    files.release(
        Node::TransactionReuse,
        Some(Handle::ReusableTransaction(id)),
    );
    assert!(files.accepted.is_some());
    assert_eq!(
        combined(&mut files, &mut writer, &bytes),
        Err(Errno::ESTALE)
    );
    assert_eq!(files.inbound.len(), 1);
}

#[test]
fn combined_call_requires_the_live_token_after_clunk_revoke_or_expiry() {
    for fault in 0..3 {
        let mut files = export();
        let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
        let bytes = golden("commit-copy-retire");
        let Handle::ReusableTransaction(id) = writer else {
            unreachable!()
        };
        match fault {
            0 => files.release(
                Node::TransactionReuse,
                Some(Handle::ReusableTransaction(id)),
            ),
            1 => files.revoke(),
            2 => {
                let mut expired = Staging::new(id, STAGING);
                expired
                    .write(
                        0,
                        &bytes,
                        Instant::now() - STAGING.assembly - Duration::from_secs(1),
                    )
                    .unwrap();
                files.staging = Some(expired);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            combined(&mut files, &mut writer, &bytes),
            Err(Errno::ESTALE)
        );
        assert!(files.accepted.is_none() && files.inbound.is_empty());
    }
}
