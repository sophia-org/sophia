//! W2 prototype codec: a whole candidate's reserved `u16` becomes flags, and
//! bit 0 appends one retire intent. Only a connection that selected revision
//! 12 and was granted capability bit 17 may carry it; everything else keeps
//! the reserved-field refusal and the unchanged r8 envelope. Revision 11 and
//! bit 16 belong to the separate draw lane, so neither enables the intent.
use sophia_shell_protocol::shell::encoding::content::{
    decode_content_candidate, decode_content_candidate_with_retire, encode_content_candidate,
    encode_content_candidate_with_retire,
};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

#[test]
fn shared_c_and_rust_retire_golden_records() {
    let corpus = include_str!("../../../spec/golden/sophia-shell-retire-v1.tsv");
    let mut count = 0;
    for row in corpus.lines().filter(|row| !row.starts_with('#')) {
        let fields: Vec<_> = row.split_whitespace().collect();
        assert_eq!(fields.len(), 4);
        let hex = fields[3];
        assert_eq!(hex.len() % 2, 0);
        let bytes: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let result = decode_shell_file_candidate_with_retire(&bytes, fields[1] == "1");
        assert_eq!(result.is_ok(), fields[2] == "1", "{}", fields[0]);
        if let Ok((candidate, retire)) = result {
            let header = decode_shell_file_record(&bytes, ShellFileClass::Candidate)
                .unwrap()
                .header;
            let encoded = match retire {
                Some(retire) => {
                    encode_shell_file_candidate_with_retire(header, &candidate, &retire)
                }
                None => encode_shell_file_candidate(header, &candidate),
            }
            .unwrap();
            assert_eq!(encoded, bytes, "{}", fields[0]);
        }
        count += 1;
    }
    assert_eq!(count, 9);
}

fn candidate() -> ContentCandidate {
    ContentCandidate {
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        },
        candidate_generation: 1,
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        facts_generation: 1,
        pacing_permit: 1,
        interaction_generation: 1,
        surfaces: Vec::new(),
        placements: Vec::new(),
        targets: Vec::new(),
    }
}

fn retire() -> ContentCandidateRetire {
    ContentCandidateRetire {
        retire_transaction: TransactionId::from_raw(50),
        resource: ContentResourceId {
            id: 4,
            generation: 2,
        },
    }
}

fn header() -> ShellFileHeader {
    ShellFileHeader {
        kind: ShellFileKind::Candidate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    }
}

fn file_value() -> ShellFileCandidate {
    ShellFileCandidate {
        transaction: TransactionId::from_raw(7),
        candidate: candidate(),
    }
}

#[test]
fn the_intent_is_carried_only_by_a_retire_enabled_connection() {
    let w2 = SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT;
    assert_eq!((SOPHIA_SHELL_RETIRE_ON_PRESENT_REVISION, w2), (12, 1 << 17));
    // An earlier revision refuses even with the capability granted, r11
    // included; r12 refuses without the grant, and bit 16 is not the grant.
    for revision in [8, 10, 11] {
        assert!(!retire_on_present_enabled(revision, w2), "r{revision}");
    }
    assert!(!retire_on_present_enabled(12, 0));
    assert!(!retire_on_present_enabled(12, 1 << 16));
    assert!(!retire_on_present_enabled(u16::MAX, !w2));
    assert!(retire_on_present_enabled(12, w2));
    assert!(!retire_on_present_enabled(13, w2 | 1 << 16));
    assert!(!retire_on_present_enabled(u16::MAX, u64::MAX));
    let bytes =
        encode_shell_file_candidate_with_retire(header(), &file_value(), &retire()).unwrap();
    assert_eq!(
        decode_shell_file_candidate_with_retire(&bytes, true).unwrap(),
        (file_value(), Some(retire()))
    );
    // An old-revision or ungranted connection refuses it as a reserved field.
    assert!(decode_shell_file_candidate_with_retire(&bytes, false).is_err());
    assert!(decode_shell_file_candidate(&bytes).is_err());
}

#[test]
fn a_candidate_without_an_intent_keeps_the_unchanged_envelope() {
    let plain = encode_shell_file_candidate(header(), &file_value()).unwrap();
    assert_eq!(
        decode_shell_file_candidate_with_retire(&plain, true).unwrap(),
        (file_value(), None)
    );
    assert_eq!(decode_shell_file_candidate(&plain).unwrap(), file_value());
    let value = encode_content_candidate(&candidate()).unwrap();
    assert_eq!(decode_content_candidate(&value).unwrap(), candidate());
}

#[test]
fn unknown_flags_trailing_or_short_intents_and_invalid_identities_refuse() {
    let value = encode_content_candidate_with_retire(&candidate(), &retire()).unwrap();
    // The flags word follows the six header fields and three row counts.
    let flags = 16 + 8 + 16 + 8 + 8 + 8 + 2 * 3;
    assert_eq!(u16::from_le_bytes([value[flags], value[flags + 1]]), 1);
    for bit in 1..16 {
        let mut unknown = value.clone();
        unknown[flags..flags + 2].copy_from_slice(&(1u16 | 1 << bit).to_le_bytes());
        assert!(
            decode_content_candidate_with_retire(&unknown, true).is_err(),
            "bit {bit}"
        );
    }
    let mut trailing = value.clone();
    trailing.push(0);
    assert!(decode_content_candidate_with_retire(&trailing, true).is_err());
    assert!(decode_content_candidate_with_retire(&value[..value.len() - 1], true).is_err());
    for invalid in [
        ContentCandidateRetire {
            retire_transaction: TransactionId::from_raw(0),
            ..retire()
        },
        ContentCandidateRetire {
            resource: ContentResourceId {
                id: 0,
                generation: 2,
            },
            ..retire()
        },
        ContentCandidateRetire {
            resource: ContentResourceId {
                id: 4,
                generation: 0,
            },
            ..retire()
        },
    ] {
        assert!(encode_content_candidate_with_retire(&candidate(), &invalid).is_err());
        let mut forged = value.clone();
        let tail = forged.len() - 24;
        forged[tail..tail + 8].copy_from_slice(&invalid.retire_transaction.raw().to_le_bytes());
        forged[tail + 8..tail + 16].copy_from_slice(&invalid.resource.id.to_le_bytes());
        forged[tail + 16..].copy_from_slice(&invalid.resource.generation.to_le_bytes());
        assert!(decode_content_candidate_with_retire(&forged, true).is_err());
    }
    // A whole-record trailing byte refuses through the file codec too.
    let mut record =
        encode_shell_file_candidate_with_retire(header(), &file_value(), &retire()).unwrap();
    record.push(0);
    let size = record.len() as u32;
    record[..4].copy_from_slice(&size.to_le_bytes());
    assert!(decode_shell_file_candidate_with_retire(&record, true).is_err());
}

fn candidate_with_rows() -> ContentCandidate {
    let surface = ContentSurface {
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        scale_generation: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        reservation_extent: 24,
        parent_surface_index: u16::MAX,
        anchor_parent_rect: ContentPixelRect::default(),
    };
    let placement = |id| ContentPlacement {
        resource: ContentResourceId { id, generation: 1 },
        surface_index: 0,
        destination_x_px: 3,
        destination_y_px: 4,
    };
    let target = ContentTarget {
        surface_index: 0,
        action_kind: 1,
        target_id: 1,
        target_generation: 1,
        action_id: 1,
        bounds_px: ContentPixelRect {
            x: 3,
            y: 4,
            width: 2,
            height: 1,
        },
    };
    ContentCandidate {
        surfaces: vec![surface],
        placements: vec![placement(1), placement(2)],
        targets: vec![target],
        ..candidate()
    }
}

#[test]
fn the_intent_follows_real_rows_and_every_truncation_refuses() {
    let rows = candidate_with_rows();
    let plain = encode_content_candidate(&rows).unwrap();
    let flagged = encode_content_candidate_with_retire(&rows, &retire()).unwrap();
    // The flagged value is the plain one with flag 1 and the 24-byte intent
    // after the last row.
    let flags = 16 + 8 + 16 + 8 + 8 + 8 + 2 * 3;
    let mut expected = plain.clone();
    expected[flags..flags + 2].copy_from_slice(&1u16.to_le_bytes());
    expected.extend(50u64.to_le_bytes());
    expected.extend(4u64.to_le_bytes());
    expected.extend(2u64.to_le_bytes());
    assert_eq!(flagged, expected);
    assert_eq!(
        decode_content_candidate_with_retire(&flagged, true).unwrap(),
        (rows.clone(), Some(retire()))
    );
    for cut in 1..=24 {
        let short = &flagged[..flagged.len() - cut];
        assert!(
            decode_content_candidate_with_retire(short, true).is_err(),
            "cut {cut}"
        );
    }
    // The plain value with its rows is unchanged and carries nothing.
    assert_eq!(
        decode_content_candidate_with_retire(&plain, true).unwrap(),
        (rows.clone(), None)
    );
    // An intent tail without the flag is a trailing field.
    let mut unflagged = flagged.clone();
    unflagged[flags..flags + 2].copy_from_slice(&0u16.to_le_bytes());
    assert!(decode_content_candidate_with_retire(&unflagged, true).is_err());
    let value = ShellFileCandidate {
        transaction: TransactionId::from_raw(7),
        candidate: rows,
    };
    let record = encode_shell_file_candidate_with_retire(header(), &value, &retire()).unwrap();
    assert_eq!(
        decode_shell_file_candidate_with_retire(&record, true).unwrap(),
        (value, Some(retire()))
    );
    assert!(decode_shell_file_candidate_with_retire(&record, false).is_err());
}

#[test]
fn allocated_shell_revisions_map_owned_ring_independently_of_w2_and_draw() {
    use sophia_shell_protocol::shell::content::owned_ring_image_revision;
    const OWNED: u64 = 1 << 15;
    const W2: u64 = 1 << 17;
    for (revision, capabilities, image) in [
        (10, OWNED, Some(10)),
        (10, OWNED | W2, Some(10)),
        (10, 0, None),
        (12, OWNED | W2, Some(10)),
        (12, OWNED, Some(10)),
        (12, W2, None),
        (12, OWNED | 1 << 16, Some(10)),
        (9, OWNED, None),
        (11, OWNED, Some(10)),
        (11, OWNED | W2, Some(10)),
        (11, 1 << 16, None),
        (13, OWNED | W2, None),
    ] {
        assert_eq!(
            owned_ring_image_revision(revision, capabilities),
            image,
            "r{revision} {capabilities:#x}"
        );
    }
}
