use sophia_protocol::wm_files::*;

// These literals follow the file layout, independently of the encoder.
fn header() -> Vec<u8> {
    vec![
        36, 0, 0, 0, 1, 0, 6, 1, // size, version, Projection
        9, 0, 0, 0, 0, 0, 0, 0, // epoch
        7, 0, 0, 0, 0, 0, 0, 0, // submission, not domain transaction
        0, 0, 0, 0, 0, 0, 0, 0, // no event sequence
        1, 2, 3, 4,
    ]
}

#[test]
fn complete_record_matches_the_independent_layout_and_borrows_its_body() {
    let bytes = header();
    let record = decode_wm_file_record(&bytes, WmFileClass::Candidate).unwrap();
    assert_eq!(record.header.kind, WmFileKind::Projection);
    assert_eq!(record.header.connection_epoch, 9);
    assert_eq!(record.header.submission_id, 7);
    assert_eq!(record.header.sequence, 0);
    assert_eq!(record.body, &[1, 2, 3, 4]);
    assert_eq!(record.body.as_ptr(), bytes[32..].as_ptr());
    assert_eq!(
        encode_wm_file_record(record.header, record.body).unwrap(),
        bytes
    );
}

#[test]
fn every_truncation_and_inconsistent_length_refuses() {
    let bytes = header();
    for end in 0..bytes.len() {
        assert!(
            decode_wm_file_record(&bytes[..end], WmFileClass::Candidate).is_err(),
            "length {end}"
        );
    }
    for size in [0u32, 31, 35, 37, u32::MAX] {
        let mut malformed = bytes.clone();
        malformed[..4].copy_from_slice(&size.to_le_bytes());
        assert_eq!(
            decode_wm_file_record(&malformed, WmFileClass::Candidate),
            Err(WmFileCodecError::Length)
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_wm_file_record(&trailing, WmFileClass::Candidate).is_err());
    let record = decode_wm_file_record(&bytes, WmFileClass::Candidate).unwrap();
    let maximum_body = vec![0; WM_FILE_MAX_BYTES - WM_FILE_HEADER_BYTES];
    let maximum = encode_wm_file_record(record.header, &maximum_body).unwrap();
    assert!(decode_wm_file_record(&maximum, WmFileClass::Candidate).is_ok());
    assert_eq!(
        encode_wm_file_record(record.header, &vec![0; maximum_body.len() + 1]),
        Err(WmFileCodecError::Length)
    );
}

#[test]
fn version_kind_epoch_and_direction_identity_are_checked() {
    for (offset, value, error) in [
        (4, 2, WmFileCodecError::Version),
        (6, 255, WmFileCodecError::Kind),
        (8, 0, WmFileCodecError::Identity),
        (16, 0, WmFileCodecError::Identity),
        (24, 1, WmFileCodecError::Identity),
    ] {
        let mut bytes = header();
        bytes[offset] = value;
        assert_eq!(
            decode_wm_file_record(&bytes, WmFileClass::Candidate),
            Err(error)
        );
    }
    let mut bytes = header();
    bytes[6..8].copy_from_slice(&2u16.to_le_bytes()); // Snapshot
    assert_eq!(
        decode_wm_file_record(&bytes, WmFileClass::Object),
        Err(WmFileCodecError::Identity)
    );
    bytes[16] = 0;
    assert!(decode_wm_file_record(&bytes, WmFileClass::Object).is_ok());
    bytes[6] = 22; // Cycle
    assert_eq!(
        decode_wm_file_record(&bytes, WmFileClass::Event),
        Err(WmFileCodecError::Identity)
    );
    bytes[24] = 1;
    assert!(decode_wm_file_record(&bytes, WmFileClass::Event).is_ok());
    bytes[16] = 7;
    assert_eq!(
        decode_wm_file_record(&bytes, WmFileClass::Event),
        Err(WmFileCodecError::Identity)
    );
}

fn section() -> Vec<u8> {
    vec![1, 0, 0, 0, 2, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4]
}

#[test]
fn sections_borrow_complete_rows_without_claiming_context_validation() {
    let bytes = section();
    let sections = decode_wm_file_sections(&bytes, 1).unwrap();
    assert_eq!(
        sections,
        vec![WmFileSection {
            kind: 1,
            count: 2,
            bytes: &[1, 2, 3, 4]
        }]
    );
    assert_eq!(sections[0].bytes.as_ptr(), bytes[16..].as_ptr());
    assert_eq!(encode_wm_file_sections(&sections).unwrap(), bytes);
    assert!(decode_wm_file_sections(&[], 0).unwrap().is_empty());
    // Kind and row count need their snapshot/projection context next.
    let mut unknown_context = section();
    unknown_context[0..2].copy_from_slice(&u16::MAX.to_le_bytes());
    unknown_context[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_wm_file_sections(&unknown_context, 1).unwrap()[0].count,
        u32::MAX
    );
}

#[test]
fn section_bounds_reserved_fields_and_order_refuse_without_row_allocation() {
    let bytes = section();
    for end in 0..bytes.len() {
        assert!(decode_wm_file_sections(&bytes[..end], 1).is_err());
    }
    for offset in [2, 3, 12, 13, 14, 15] {
        let mut malformed = bytes.clone();
        malformed[offset] = 1;
        assert_eq!(
            decode_wm_file_sections(&malformed, 1),
            Err(WmFileCodecError::Reserved)
        );
    }
    for (offset, value) in [(0, 0), (4, 0), (8, 0), (8, u32::MAX)] {
        let mut malformed = bytes.clone();
        malformed[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(decode_wm_file_sections(&malformed, 1).is_err());
    }
    assert!(decode_wm_file_sections(&bytes, 0).is_err());
    assert!(decode_wm_file_sections(&bytes, 33).is_err());
    assert!(decode_wm_file_sections(&[bytes.clone(), bytes.clone()].concat(), 2).is_err());
    let mut larger = bytes.clone();
    larger[0] = 2;
    assert!(decode_wm_file_sections(&[bytes.clone(), larger.clone()].concat(), 2).is_ok());
    assert!(decode_wm_file_sections(&[larger, bytes].concat(), 2).is_err());
}

#[test]
fn section_encoder_checks_the_same_aggregate_bounds_before_copying() {
    let bytes = [1];
    let sections: Vec<_> = (1..=32)
        .map(|kind| WmFileSection {
            kind,
            count: 1,
            bytes: &bytes,
        })
        .collect();
    assert!(encode_wm_file_sections(&sections).is_ok());
    let mut too_many = sections.clone();
    too_many.push(WmFileSection {
        kind: 33,
        count: 1,
        bytes: &bytes,
    });
    assert!(encode_wm_file_sections(&too_many).is_err());
    assert!(encode_wm_file_sections(&[sections[0], sections[0]]).is_err());
    assert!(
        encode_wm_file_sections(&[WmFileSection {
            kind: 1,
            count: 0,
            bytes: &bytes
        }])
        .is_err()
    );
    assert!(
        encode_wm_file_sections(&[WmFileSection {
            kind: 1,
            count: 1,
            bytes: &[]
        }])
        .is_err()
    );
    assert!(
        encode_wm_file_sections(&[WmFileSection {
            kind: 1,
            count: 1,
            bytes: &vec![0; WM_FILE_MAX_BYTES]
        }])
        .is_err()
    );
}

#[test]
fn submit_has_explicit_length_epoch_id_and_zero_reserved_bytes() {
    let bytes = vec![
        9, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 36, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(
        decode_wm_file_submit(&bytes).unwrap(),
        WmFileSubmit {
            connection_epoch: 9,
            submission_id: 7,
            candidate_bytes: 36
        }
    );
    for end in 0..bytes.len() {
        assert!(decode_wm_file_submit(&bytes[..end]).is_err());
    }
    for offset in [0, 8, 16] {
        let mut malformed = bytes.clone();
        malformed[offset] = 0;
        assert!(decode_wm_file_submit(&malformed).is_err());
    }
    for offset in 20..24 {
        let mut malformed = bytes.clone();
        malformed[offset] = 1;
        assert_eq!(
            decode_wm_file_submit(&malformed),
            Err(WmFileCodecError::Reserved)
        );
    }
    let mut excessive = bytes.clone();
    excessive[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_wm_file_submit(&excessive).is_err());
}

#[test]
fn ack_requires_exact_nonzero_identity() {
    let bytes = vec![9, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        decode_wm_file_ack(&bytes).unwrap(),
        WmFileAck {
            connection_epoch: 9,
            sequence: 7
        }
    );
    for end in 0..bytes.len() {
        assert!(decode_wm_file_ack(&bytes[..end]).is_err());
    }
    for offset in [0, 8] {
        let mut malformed = bytes.clone();
        malformed[offset] = 0;
        assert!(decode_wm_file_ack(&malformed).is_err());
    }
    assert!(decode_wm_file_ack(&[bytes, vec![0]].concat()).is_err());
}

#[test]
fn every_kind_has_an_exhaustive_class_and_refuses_each_wrong_file_class() {
    use WmFileKind::*;
    let cases = [
        (Limits, 1),
        (Snapshot, 2),
        (Negotiated, 16),
        (Submitted, 17),
        (ProfilePrepare, 18),
        (ProfileActivate, 19),
        (ProfileRollback, 20),
        (ConfigurationOutcome, 21),
        (Cycle, 22),
        (ProjectionOutcome, 23),
        (SessionOperationOutcome, 24),
        (PresentationReceipt, 25),
        (Negotiate, 256),
        (ProfilePrepared, 257),
        (ProfileActive, 258),
        (ProfileRolledBack, 259),
        (Configuration, 260),
        (Dirty, 261),
        (Projection, 262),
        (SessionOperation, 263),
    ];
    for (kind, number) in cases {
        let class = wm_file_class(kind);
        let header = WmFileHeader {
            kind,
            connection_epoch: 9,
            submission_id: u64::from(class == WmFileClass::Candidate),
            sequence: u64::from(class == WmFileClass::Event),
        };
        let bytes = encode_wm_file_record(header, &[]).unwrap();
        assert_eq!(u16::from_le_bytes(bytes[6..8].try_into().unwrap()), number);
        assert_eq!(decode_wm_file_record(&bytes, class).unwrap().header, header);
        for wrong in [
            WmFileClass::Object,
            WmFileClass::Event,
            WmFileClass::Candidate,
        ] {
            if wrong != class {
                assert_eq!(
                    decode_wm_file_record(&bytes, wrong),
                    Err(WmFileCodecError::Class)
                );
            }
        }
    }
}
