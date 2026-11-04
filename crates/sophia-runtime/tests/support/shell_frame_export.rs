use super::*;

const CAPS: u64 = SHELL_FRAME_CAPABILITY
    | crate::owned_ring_protocol::CAPABILITY
    | sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;

fn unnegotiated() -> ShellFiles {
    let mut files = fixture();
    files.negotiated = false;
    files.content = false;
    files.content_limits = None;
    files.capabilities = 0;
    files.image_revision = 0;
    files
}

fn negotiate(files: &mut ShellFiles, caps: u64, revision: u16) -> Result<(), Errno> {
    let limits = sophia_protocol::ContentLimits::prototype(identity().grant);
    let bytes = crate::shell_transport::files::encode_limits_object(1, &limits).unwrap();
    files.complete_negotiation_with_image_revision(Some((bytes, limits)), caps, revision)
}

fn export() -> ShellFiles {
    let mut files = unnegotiated();
    negotiate(&mut files, CAPS, SHELL_FRAME_REVISION).unwrap();
    assert_eq!(files.submission_revision(), 13);
    assert_eq!(files.image_revision, 10);
    assert_eq!(files.negotiated_source_kinds(), 4);
    files
}

fn goldens() -> Vec<(String, Vec<u8>)> {
    include_str!("../../../../protocol/golden/sophia-frame-transactions-v1.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            let bytes = fields[2]
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            (fields[0].into(), bytes)
        })
        .collect()
}

fn golden(name: &str) -> Vec<u8> {
    goldens()
        .into_iter()
        .find(|(key, _)| key == name)
        .unwrap()
        .1
}

fn stage(files: &mut ShellFiles, bytes: &[u8]) -> (Handle, Vec<u8>) {
    let mut writer = files.open(&Node::TransactionReuse, OpenFlags(2)).unwrap();
    for (index, chunk) in bytes.chunks(17).enumerate() {
        files
            .write(
                &Node::TransactionReuse,
                &mut writer,
                (index * 17) as u64,
                chunk,
            )
            .unwrap();
    }
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: 1,
        submission_id: 12,
        candidate_bytes: bytes.len() as u32,
    })
    .unwrap();
    (writer, submit)
}

#[test]
fn frame_export_requires_exact_explicit_cpu_content_negotiation() {
    for (caps, revision) in [
        (CAPS & !SHELL_FRAME_CAPABILITY, 13),
        (CAPS & !crate::owned_ring_protocol::CAPABILITY, 13),
        (
            CAPS & !sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            13,
        ),
        (CAPS, 10),
        (CAPS, 14),
        (CAPS | (1 << 11), 13),
        (CAPS | (1 << 12), 13),
        (CAPS | (1 << 13), 13),
        (CAPS | (1 << 14), 13),
        (CAPS | (1 << 16), 13),
        (CAPS | (1 << 17), 13),
        (CAPS | (1 << 63), 13),
    ] {
        let mut files = unnegotiated();
        assert_eq!(negotiate(&mut files, caps, revision), Err(Errno::EACCES));
        assert!(!files.negotiated);
        assert_eq!(files.capabilities, 0);
        assert!(!files.image_disclosed());
    }
    for fault in 0..5 {
        let mut files = unnegotiated();
        match fault {
            0 => files.image = None,
            1 => files.catalog_allowed = true,
            2 => files.descriptor = true,
            3 => {
                assert_eq!(
                    files.complete_negotiation_with_image_revision(None, CAPS, 13),
                    Err(Errno::EACCES)
                );
                continue;
            }
            4 => files.image.as_mut().unwrap().config.discovery.source_kinds |= 2,
            _ => unreachable!(),
        }
        assert_eq!(negotiate(&mut files, CAPS, 13), Err(Errno::EACCES));
        assert!(!files.negotiated);
    }
    for fault in 0..3 {
        let mut files = unnegotiated();
        let mut limits = sophia_protocol::ContentLimits::prototype(identity().grant);
        match fault {
            0 => limits.grant.connection_epoch = 2,
            1 => limits.grant.content_grant_epoch = 0,
            2 => limits.max_candidate_bytes = 0,
            _ => unreachable!(),
        }
        assert_eq!(
            files.complete_negotiation_with_image_revision(Some((Vec::new(), limits)), CAPS, 13),
            Err(Errno::EACCES)
        );
        assert!(!files.negotiated);
    }
}

#[test]
fn fragmented_frames_transfer_whole_once_and_receipt_ack_does_not_consume_input() {
    let mut count = 0;
    for (name, bytes) in goldens() {
        let Ok(expected) = decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192)
        else {
            continue;
        };
        count += 1;
        let mut files = export();
        let (_writer, submit) = stage(&mut files, &bytes);
        files.submit_transaction(&submit).unwrap();
        files.submit_transaction(&submit).unwrap();
        assert_eq!(files.inbound.len(), 1, "{name}");
        let journal = journal_bytes(&files);
        let receipt = decode_shell_file_submitted_at_revision(&journal, 13).unwrap();
        assert_eq!(receipt.candidate_kind, expected.kind());
        assert_eq!(receipt.submission_id, 12);
        files
            .write(
                &Node::Ack,
                &mut Handle::Plain,
                0,
                &encode_shell_file_ack(ShellFileAck {
                    connection_epoch: 1,
                    sequence: files.accepted.as_ref().unwrap().sequence,
                })
                .unwrap(),
            )
            .unwrap();
        assert!(files.accepted.is_none());
        assert_eq!(files.inbound.len(), 1, "receipt ACK is not owner admission");
        assert!(
            files.peek_candidate_family().is_none(),
            "SFT is never split into legacy parts"
        );
        assert_eq!(
            files.take_inbound(),
            Some(Inbound::Frame(Box::new(expected)))
        );
        assert!(
            files.owned_pending_offer().is_none(),
            "embedded offer is not separately admitted"
        );
    }
    assert_eq!(count, 6);
}

#[test]
fn the_negotiated_journal_preserves_all_frame_event_shapes_including_prepared() {
    let mut count = 0;
    for (_, bytes) in goldens() {
        let Ok(expected) = decode_shell_frame_record(&bytes, ShellFileClass::Event, 13, 8192)
        else {
            continue;
        };
        count += 1;
        let mut files = export();
        files
            .append_event(expected.kind(), &bytes[SHELL_FILE_HEADER_BYTES..], true)
            .unwrap();
        assert_eq!(
            decode_shell_frame_record(&journal_bytes(&files), ShellFileClass::Event, 13, 8192)
                .unwrap(),
            expected
        );
        let mut legacy = fixture();
        assert!(
            legacy
                .append_event(expected.kind(), &bytes[SHELL_FILE_HEADER_BYTES..], true)
                .is_err()
        );
    }
    assert_eq!(count, 10);
}

#[test]
fn malformed_or_over_grant_frames_never_acquire_file_custody() {
    for fault in 0..4 {
        let mut files = export();
        let mut bytes = golden("commit-copy-retire");
        match fault {
            0 => bytes[SHELL_FILE_HEADER_BYTES + 14] = 1,
            1 => {
                bytes.pop();
                let len = bytes.len() as u32;
                bytes[..4].copy_from_slice(&len.to_le_bytes());
            }
            2 => {
                files.content_limits.as_mut().unwrap().max_candidate_bytes =
                    (bytes.len() - 1) as u32
            }
            3 => bytes[8..16].copy_from_slice(&2u64.to_le_bytes()),
            _ => unreachable!(),
        }
        let (_writer, submit) = stage(&mut files, &bytes);
        assert!(files.submit_transaction(&submit).is_err());
        assert!(files.accepted.is_none());
        assert!(files.inbound.is_empty());
        assert_eq!(files.submission_watermark, 0);
    }
}

#[test]
fn receipt_backpressure_leaves_frame_bytes_and_identity_available_for_retry() {
    let mut files = export();
    files.journal = Journal::new(
        1,
        JournalBounds {
            bytes: 32,
            reserve_bytes: 0,
        },
        Instant::now(),
    );
    let bytes = golden("commit-copy-retire");
    let (_writer, submit) = stage(&mut files, &bytes);
    assert_eq!(files.submit_transaction(&submit), Err(Errno::EAGAIN));
    assert!(files.accepted.is_none());
    assert!(files.inbound.is_empty());
    assert_eq!(files.submission_watermark, 0);
    assert_eq!(files.staging.as_ref().unwrap().bytes, bytes);
    files.journal = Journal::new(
        1,
        JournalBounds {
            bytes: 65536,
            reserve_bytes: 12288,
        },
        Instant::now(),
    );
    files.journal.enable_frame_revision();
    files.submit_transaction(&submit).unwrap();
    assert_eq!(files.inbound.len(), 1);
}

#[test]
fn legacy_file_negotiation_cannot_submit_frame_requests() {
    for name in ["open", "close", "commit-retained", "commit-copy-retire"] {
        let mut files = fixture();
        let (_writer, submit) = stage(&mut files, &golden(name));
        assert!(files.submit_transaction(&submit).is_err());
        assert!(files.inbound.is_empty());
        assert!(files.accepted.is_none());
    }
}
