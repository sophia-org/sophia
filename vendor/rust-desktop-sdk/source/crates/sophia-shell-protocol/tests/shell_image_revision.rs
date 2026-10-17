use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

fn header(kind: ShellFileKind) -> ShellFileHeader {
    let class = shell_file_class(kind);
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: u64::from(class == ShellFileClass::Candidate),
        sequence: u64::from(class == ShellFileClass::Event),
    }
}

#[test]
fn image_envelopes_require_explicit_r9() {
    for kind in [
        ShellFileKind::ImageBegin,
        ShellFileKind::ImageAdmitted,
        ShellFileKind::SourceReleased,
        ShellFileKind::ChannelReady,
        ShellFileKind::ChannelClosed,
        ShellFileKind::ImageLimits,
        ShellFileKind::ImageFormats,
    ] {
        let h = header(kind);
        let class = shell_file_class(kind);
        let bytes = encode_shell_file_record_at_revision(h, &[], 9).unwrap();
        assert_eq!(
            decode_shell_file_record_at_revision(&bytes, class, 9)
                .unwrap()
                .header,
            h
        );
        assert_eq!(
            encode_shell_file_record(h, &[]),
            Err(ShellFileCodecError::Kind)
        );
        assert_eq!(
            decode_shell_file_record(&bytes, class),
            Err(ShellFileCodecError::Kind)
        );
        for revision in 1..=8 {
            assert_eq!(
                decode_shell_file_record_at_revision(&bytes, class, revision),
                Err(ShellFileCodecError::Kind)
            );
            assert_eq!(
                encode_shell_file_record_at_revision(h, &[], revision),
                Err(ShellFileCodecError::Kind)
            );
        }
        for revision in [0, 10, u16::MAX] {
            assert_eq!(
                decode_shell_file_record_at_revision(&bytes, class, revision),
                Err(ShellFileCodecError::Version)
            );
            assert_eq!(
                encode_shell_file_record_at_revision(h, &[], revision),
                Err(ShellFileCodecError::Version)
            );
        }
    }
    let h = header(ShellFileKind::Limits);
    let bytes = encode_shell_file_record(h, &[]).unwrap();
    assert_eq!(
        decode_shell_file_record_at_revision(&bytes, ShellFileClass::Object, 9)
            .unwrap()
            .header,
        h
    );
}

#[test]
fn image_formats_cap_does_not_widen_other_objects() {
    let h = header(ShellFileKind::ImageFormats);
    let mut body = vec![0; SHELL_FILE_MAX_IMAGE_FORMATS_BYTES - SHELL_FILE_HEADER_BYTES];
    assert!(encode_shell_file_record_at_revision(h, &body, 9).is_ok());
    body.push(0);
    assert_eq!(
        encode_shell_file_record_at_revision(h, &body, 9),
        Err(ShellFileCodecError::Length)
    );
    assert!(encode_shell_file_record(header(ShellFileKind::Catalog), &body).is_ok());
}

fn status(status: u16, reason: u16) -> ShellFileTransactionRecord {
    ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(1),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            },
            resource: ContentResourceId {
                id: 1,
                generation: 1,
            },
            status,
            reason,
            next_ordinal: 0,
            admitted_bytes: 0,
        }),
    }
}

#[test]
fn reason_13_is_only_rejected_resource_status_at_r9() {
    let h = header(ShellFileKind::ResourceStatus);
    let value = status(3, 13);
    let bytes = encode_shell_file_resource_status_at_revision(h, &value, 9).unwrap();
    assert_eq!(
        shell_file_resource_status_bytes_at_revision(h, &value, 9).unwrap(),
        bytes.len()
    );
    assert_eq!(
        decode_shell_file_resource_status_at_revision(&bytes, 9).unwrap(),
        value
    );
    assert!(encode_shell_file_resource_status(h, &value).is_err());
    assert!(decode_shell_file_resource_status(&bytes).is_err());
    for revision in [0, 1, 8, 10] {
        assert!(encode_shell_file_resource_status_at_revision(h, &value, revision).is_err());
        assert!(decode_shell_file_resource_status_at_revision(&bytes, revision).is_err());
    }
    for status_code in [0, 1, 2, 4, 5] {
        assert!(
            encode_shell_file_resource_status_at_revision(h, &status(status_code, 13), 9).is_err()
        );
        let mut invalid = bytes.clone();
        invalid[72..74].copy_from_slice(&status_code.to_le_bytes());
        assert!(decode_shell_file_resource_status_at_revision(&invalid, 9).is_err());
    }
    let mut invalid = bytes.clone();
    invalid[74..76].copy_from_slice(&14_u16.to_le_bytes());
    assert!(decode_shell_file_resource_status_at_revision(&invalid, 9).is_err());
    assert!(encode_shell_file_resource_status_at_revision(h, &status(3, 14), 9).is_err());
    for end in 0..bytes.len() {
        assert!(decode_shell_file_resource_status_at_revision(&bytes[..end], 9).is_err());
    }
}

#[test]
fn image_objects_do_not_enter_legacy_object_published() {
    assert!(
        encode_shell_file_submitted_body(ShellFileSubmitted {
            submission_id: 1,
            candidate_kind: ShellFileKind::ImageBegin,
        })
        .is_err()
    );
    for object in [ShellFileKind::ImageLimits, ShellFileKind::ImageFormats] {
        assert!(
            encode_shell_file_object_published_body(ShellFileObjectPublished {
                object,
                generation: 1,
                qid: 1,
            })
            .is_err()
        );
    }
}

#[test]
fn existing_descriptor_candidates_keep_legacy_receipts() {
    for candidate_kind in [
        ShellFileKind::DescriptorCandidate,
        ShellFileKind::DescriptorActivationAck,
        ShellFileKind::TabsCandidate,
        ShellFileKind::ReferenceCandidate,
        ShellFileKind::LauncherCandidate,
        ShellFileKind::LauncherActivationAck,
    ] {
        let value = ShellFileSubmitted {
            submission_id: 1,
            candidate_kind,
        };
        let bytes = encode_shell_file_submitted(header(ShellFileKind::Submitted), value).unwrap();
        assert_eq!(decode_shell_file_submitted(&bytes).unwrap(), value);
    }
}
