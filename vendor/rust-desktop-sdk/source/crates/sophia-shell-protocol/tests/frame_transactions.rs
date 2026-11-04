use sophia_shell_protocol::shell_files::*;

#[test]
fn maximum_scene_rows_and_targets_round_trip_without_weakening_indices() {
    use sophia_shell_protocol::{ContentPixelRect, ContentTarget};
    let bytes = named("commit-retained");
    let header = decode_shell_file_record_at_revision(&bytes, ShellFileClass::Candidate, 13)
        .unwrap()
        .header;
    let ShellFrameRecord::Commit(mut value) = decode(&bytes, ShellFileClass::Candidate).unwrap()
    else {
        panic!()
    };
    value
        .candidate
        .placements
        .resize(32, value.candidate.placements[0].clone());
    value.candidate.targets = (1..=64)
        .map(|id| ContentTarget {
            surface_index: 0,
            action_kind: 1,
            target_id: id,
            target_generation: 1,
            action_id: id,
            bounds_px: ContentPixelRect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
        })
        .collect();
    let record = ShellFrameRecord::Commit(value.clone());
    let bytes = encode_shell_frame_record(header, &record, 13).unwrap();
    assert_eq!(bytes.len(), 32 + 16 + 72 + 64 + 32 * 32 + 64 * 48);
    assert_eq!(decode(&bytes, ShellFileClass::Candidate).unwrap(), record);
    value.candidate.targets[0].surface_index = 1;
    assert!(
        encode_shell_frame_record(header, &ShellFrameRecord::Commit(value.clone()), 13).is_err()
    );
    value.candidate.targets[0].surface_index = 0;
    value
        .candidate
        .targets
        .push(value.candidate.targets[0].clone());
    assert!(encode_shell_frame_record(header, &ShellFrameRecord::Commit(value), 13).is_err());
}

#[test]
fn sft_submission_receipts_require_explicit_r13() {
    let header = ShellFileHeader {
        kind: ShellFileKind::Submitted,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 13,
    };
    for kind in [
        ShellFileKind::FrameStreamOpen,
        ShellFileKind::FrameStreamClose,
        ShellFileKind::FrameCommit,
    ] {
        let value = ShellFileSubmitted {
            submission_id: 12,
            candidate_kind: kind,
        };
        let bytes = encode_shell_file_submitted_at_revision(header, value, 13).unwrap();
        assert_eq!(
            decode_shell_file_submitted_at_revision(&bytes, 13).unwrap(),
            value
        );
        assert!(decode_shell_file_submitted(&bytes).is_err());
        for revision in [8, 9, 10, 11, 12, 14] {
            assert!(encode_shell_file_submitted_at_revision(header, value, revision).is_err());
            assert!(decode_shell_file_submitted_at_revision(&bytes, revision).is_err());
        }
    }
}

#[test]
fn classify_shared_mutations_when_supplied() {
    use std::io::Write;
    let Ok(path) = std::env::var("SFT_PARITY_CASES") else {
        return;
    };
    let mut output = std::io::BufWriter::new(
        std::fs::File::create(std::env::var("SFT_PARITY_OUTPUT").unwrap()).unwrap(),
    );
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let (kind, hex) = line.split_once('\t').unwrap();
        let expected = if kind == "request" {
            ShellFileClass::Candidate
        } else {
            assert_eq!(kind, "event");
            ShellFileClass::Event
        };
        let bytes: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let result = decode(&bytes, expected);
        if let Ok(value) = &result {
            let record = decode_shell_file_record_at_revision(&bytes, expected, 13).unwrap();
            assert_eq!(
                encode_shell_frame_record(record.header, value, 13).unwrap(),
                bytes
            );
        }
        writeln!(output, "{}", u8::from(result.is_ok())).unwrap();
    }
}

fn vectors() -> Vec<(&'static str, Vec<u8>)> {
    include_str!("../../../spec/golden/sophia-frame-transactions-v1.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 3);
            let (name, hex) = (fields[0], fields[2]);
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            (name, bytes)
        })
        .collect()
}

fn class(bytes: &[u8]) -> ShellFileClass {
    if u16::from_le_bytes(bytes[6..8].try_into().unwrap()) >= 256 {
        ShellFileClass::Candidate
    } else {
        ShellFileClass::Event
    }
}

fn decode(
    bytes: &[u8],
    expected: ShellFileClass,
) -> Result<ShellFrameRecord, ShellFilePayloadError> {
    decode_shell_frame_record(bytes, expected, SHELL_FRAME_REVISION, 8192)
}

fn named(name: &str) -> Vec<u8> {
    vectors().into_iter().find(|v| v.0 == name).unwrap().1
}

fn length(bytes: &mut [u8]) {
    let len = bytes.len() as u32;
    bytes[..4].copy_from_slice(&len.to_le_bytes());
}

fn refuses(name: &str, at: usize, replacement: &[u8]) {
    let mut bytes = named(name);
    let expected = class(&bytes);
    bytes[at..at + replacement.len()].copy_from_slice(replacement);
    assert!(
        decode(&bytes, expected).is_err(),
        "{name} accepted mutation at {at}"
    );
}

#[test]
fn every_shared_vector_decodes_and_encodes_exactly() {
    let corpus = vectors();
    assert_eq!(corpus.len(), 16);
    for (name, bytes) in corpus {
        let record = decode_shell_file_record_at_revision(&bytes, class(&bytes), 13).unwrap();
        let value = decode(&bytes, class(&bytes)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(
            shell_frame_record_bytes(record.header, &value, 13).unwrap(),
            bytes.len(),
            "{name}"
        );
        assert_eq!(
            encode_shell_frame_record(record.header, &value, 13).unwrap(),
            bytes,
            "{name}"
        );
        assert!(
            decode_shell_frame_record(&bytes, class(&bytes), 13, bytes.len() - 1).is_err(),
            "{name} grant cap"
        );
        assert!(
            decode(
                &bytes,
                if class(&bytes) == ShellFileClass::Event {
                    ShellFileClass::Candidate
                } else {
                    ShellFileClass::Event
                }
            )
            .is_err()
        );
        for revision in [1, 8, 9, 10, 11, 12, 14, u16::MAX] {
            assert!(
                decode_shell_frame_record(&bytes, class(&bytes), revision, 8192).is_err(),
                "{name} r{revision}"
            );
            assert!(
                decode_shell_file_record_at_revision(&bytes, class(&bytes), revision).is_err(),
                "envelope {name} r{revision}"
            );
            assert!(encode_shell_frame_record(record.header, &value, revision).is_err());
        }
        assert!(decode_shell_file_record(&bytes, class(&bytes)).is_err());
    }
}

#[test]
fn every_truncation_trailing_byte_and_envelope_fault_refuses_without_panic() {
    for (name, bytes) in vectors() {
        let expected = class(&bytes);
        for len in 0..bytes.len() {
            assert!(
                decode(&bytes[..len], expected).is_err(),
                "{name} truncation {len}"
            );
            if len >= 32 {
                let mut short = bytes[..len].to_vec();
                length(&mut short);
                assert!(
                    decode(&short, expected).is_err(),
                    "{name} declared truncation {len}"
                );
            }
        }
        let mut extra = bytes.clone();
        extra.push(0);
        length(&mut extra);
        assert!(decode(&extra, expected).is_err(), "{name} trailing byte");
        refuses(name, 4, &2u16.to_le_bytes());
        refuses(name, 8, &2u64.to_le_bytes()); // mismatched grant
        refuses(name, 32, &0u64.to_le_bytes()); // transaction
        let mut oversized = bytes;
        oversized.resize(8193, 0);
        length(&mut oversized);
        assert!(decode_shell_frame_record(&oversized, expected, 13, usize::MAX).is_err());
    }
}

#[test]
fn stream_and_refusal_fields_remain_bounded_and_typed() {
    for name in ["open", "close", "opened", "closed"] {
        for at in (40..104).step_by(8) {
            refuses(name, at, &0u64.to_le_bytes());
        }
    }
    for credits in [0u32, 2, u32::MAX] {
        refuses("opened", 104, &credits.to_le_bytes());
    }
    for ttl in [0u32, 251, u32::MAX] {
        refuses("opened", 108, &ttl.to_le_bytes());
    }
    refuses("closed", 106, &[1]);
    for why in [0u16, 13, u16::MAX] {
        refuses("closed", 104, &why.to_le_bytes());
        refuses("refused-commit", 90, &why.to_le_bytes());
    }
    refuses("refused-commit", 92, &[1]);
    refuses("refused-commit", 88, &262u16.to_le_bytes());
    refuses("refused-commit", 80, &0u64.to_le_bytes());
    refuses("refused-open", 80, &9u64.to_le_bytes());
    refuses("refused-close", 80, &9u64.to_le_bytes());
    for at in [40, 48, 56, 64, 72] {
        refuses("refused-commit", at, &0u64.to_le_bytes());
    }
}

#[test]
fn complete_frame_bounds_precede_rows_and_optional_payloads_cannot_be_smuggled() {
    for name in [
        "commit-retained",
        "commit-retained-retire",
        "commit-copy",
        "commit-copy-retire",
    ] {
        refuses(name, 44, &4u16.to_le_bytes());
        refuses(name, 46, &[1]);
        for count in [0u16, 2, u16::MAX] {
            refuses(name, 112, &count.to_le_bytes());
        }
        refuses(name, 114, &33u16.to_le_bytes());
        refuses(name, 116, &65u16.to_le_bytes());
        refuses(name, 118, &[1]);
        for size in [0u32, 1, 135, 167, 169, u32::MAX] {
            refuses(name, 40, &size.to_le_bytes());
        }
    }
    // D4C1 begins at 216. Slot, epochs, allocation/output/facts all belong to this frame.
    for slot in [2u64, 3, u64::MAX] {
        refuses("commit-copy", 280, &slot.to_le_bytes());
    }
    for at in [232, 240, 304, 312, 320, 328, 336, 344] {
        refuses("commit-copy", at, &99u64.to_le_bytes());
    }
    for at in [248, 256, 264, 272, 288, 296, 304, 360] {
        refuses("commit-copy", at, &0u64.to_le_bytes());
    }
    refuses("commit-copy", 288, &25u64.to_le_bytes());
    refuses("commit-copy", 200, &1u16.to_le_bytes());
    refuses("commit-copy", 352, &1_048_577u64.to_le_bytes());
    refuses("commit-copy", 216, &151u32.to_le_bytes());
    refuses("commit-copy", 220, b"D4E1");
    refuses("commit-retained-retire", 216, &11u64.to_le_bytes()); // same transaction
    // Matching both resource words is forbidden, not merely the id.
    let mut retained = named("commit-retained-retire");
    retained[224..232].copy_from_slice(&23u64.to_le_bytes());
    retained[232..240].copy_from_slice(&24u64.to_le_bytes());
    assert!(decode(&retained, ShellFileClass::Candidate).is_err());
}

#[test]
fn prepared_cannot_return_credit_and_terminals_keep_existing_outcome_rules() {
    refuses("prepared", 140, &1u64.to_le_bytes());
    for name in [
        "prepared",
        "presented-credit",
        "presented-closed",
        "rejected-credit",
        "superseded-credit",
    ] {
        for at in [108, 116, 124, 132] {
            refuses(name, at, &0u64.to_le_bytes());
        }
        refuses(name, 80, &0u16.to_le_bytes()); // kind
        refuses(name, 82, &13u16.to_le_bytes()); // reason
    }
    refuses("presented-credit", 84, &0u64.to_le_bytes());
    refuses("prepared", 84, &1u64.to_le_bytes());
}

#[test]
fn value_encoding_rejects_invalid_profile_without_creating_bytes() {
    let bytes = named("commit-copy-retire");
    let header = decode_shell_file_record_at_revision(&bytes, ShellFileClass::Candidate, 13)
        .unwrap()
        .header;
    let ShellFrameRecord::Commit(mut value) = decode(&bytes, ShellFileClass::Candidate).unwrap()
    else {
        panic!()
    };
    value.image.as_mut().unwrap().slot = 2;
    assert!(
        encode_shell_frame_record(header, &ShellFrameRecord::Commit(value.clone()), 13).is_err()
    );
    value.image.as_mut().unwrap().slot = 0;
    value
        .candidate
        .surfaces
        .push(value.candidate.surfaces[0].clone());
    assert!(shell_frame_record_bytes(header, &ShellFrameRecord::Commit(value), 13).is_err());
}
