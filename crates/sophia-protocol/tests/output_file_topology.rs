use sophia_protocol::output_files::*;
use sophia_protocol::*;

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .flat_map(|word| {
            assert_eq!(word.len() % 2, 0);
            (0..word.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&word[offset..offset + 2], 16).unwrap())
        })
        .collect()
}

fn literal() -> Vec<u8> {
    let mut bytes = hex("
        0700000000000000 0200000000000000 0100 0100 0100 0000
        0300000000000000 0400000000000000 0700 ff00 0400 0100
        0500000000000000 0000 0000 00000000 44502d31
    ");
    bytes.extend([0; 60]); // The remaining fixed 64-byte label slot.
    bytes.extend(hex("
        0500000000000000 20030000 58020000 60ea0000 0100 0000
        0200000000000000 0600000000000000 00000000 00000000 20030000 58020000 0100 0000
        0300000000000000 0300 0000
    "));
    bytes.extend([0; 36]); // Three unused member slots.
    assert_eq!(bytes.len(), 236);
    bytes
}

fn snapshot() -> OutputV1Snapshot {
    OutputV1Snapshot {
        connection_epoch: 9,
        snapshot: OutputAuthoritySnapshot {
            topology_epoch: 7,
            primary_output: OutputId::from_raw(2),
            heads: vec![OutputHeadDescriptor {
                head: DisplayHeadId::from_raw(3),
                generation: 4,
                label: "DP-1".into(),
                connected: true,
                enabled: true,
                vrr_capable: true,
                transforms: OutputTransformSet::ALL,
                current_mode: Some(DisplayModeId::from_raw(5)),
                modes: vec![OutputModeDescriptor {
                    mode: DisplayModeId::from_raw(5),
                    pixel_size: Size {
                        width: 800,
                        height: 600,
                    },
                    refresh_millihz: 60_000,
                    preferred: true,
                }],
            }],
            groups: vec![OutputLogicalGroupState {
                output: OutputId::from_raw(2),
                generation: 6,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 800,
                    height: 600,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Exact,
                }],
            }],
        },
    }
}

#[test]
fn literal_topology_rows_match_the_native_layout() {
    let bytes = literal();
    assert_eq!(decode_output_file_topology(&bytes, 9).unwrap(), snapshot());
    assert_eq!(encode_output_file_topology(&snapshot()).unwrap(), bytes);
    let header = OutputFileHeader {
        kind: OutputFileKind::Topology,
        connection_epoch: 9,
        submission_id: 0,
        sequence: 0,
    };
    let record = encode_output_file_record(header, &bytes).unwrap();
    assert_eq!(record.len(), 268);
    let decoded = decode_output_file_record(&record, OutputFileClass::Object).unwrap();
    assert_eq!(
        decode_output_file_topology(decoded.body, decoded.header.connection_epoch).unwrap(),
        snapshot()
    );
}

#[test]
fn topology_refuses_every_truncation_and_nonzero_padding_byte() {
    let bytes = literal();
    for end in 0..bytes.len() {
        assert!(
            decode_output_file_topology(&bytes[..end], 9).is_err(),
            "prefix {end}"
        );
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(decode_output_file_topology(&extra, 9).is_err());
    for offset in (22..24)
        .chain(58..64)
        .chain(68..128)
        .chain(150..152)
        .chain(186..188)
        .chain(198..236)
    {
        let mut bad = bytes.clone();
        bad[offset] = 1;
        assert!(
            decode_output_file_topology(&bad, 9).is_err(),
            "padding {offset}"
        );
    }
}

#[test]
fn topology_refuses_invalid_counts_flags_text_and_scalar_values() {
    let bytes = literal();
    assert!(decode_output_file_topology(&bytes, 0).is_err());
    for (offset, value) in [
        (0, 0),
        (8, 0), // topology epoch and primary identity
        (16, 0),
        (16, 17),
        (18, 0),
        (18, 17),
        (20, 0),
        (21, 9),
        (24, 0),
        (32, 0), // head and generation
        (40, 8),
        (42, 0),
        (43, 1), // flags and transform set
        (44, 0),
        (44, 65),
        (46, 0),
        (46, 129),
        (48, 0),
        (56, 1),
        (64, 255), // invalid UTF-8
        (128, 0),
        (139, 128),
        (143, 128),
        (148, 2), // mode, negative width/height, bool
        (152, 0),
        (160, 0),
        (171, 128),
        (175, 128),
        (179, 128),
        (183, 128),
        (184, 0),
        (184, 5),
        (188, 0),
        (188, 99),
        (196, 0),
        (196, 4),
    ] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        assert!(
            decode_output_file_topology(&bad, 9).is_err(),
            "offset {offset}, value {value}"
        );
    }
    let mut bad = bytes;
    bad[144..148].fill(0); // refresh must be positive
    assert!(decode_output_file_topology(&bad, 9).is_err());
}

fn two_heads() -> OutputV1Snapshot {
    let mut value = snapshot();
    let mut second = value.snapshot.heads[0].clone();
    second.head = DisplayHeadId::from_raw(4);
    second.modes[0].mode = DisplayModeId::from_raw(6);
    second.current_mode = Some(DisplayModeId::from_raw(6));
    let mut other_mode = second.modes[0];
    other_mode.mode = DisplayModeId::from_raw(7);
    second.modes.push(other_mode);
    value.snapshot.heads.push(second);
    value.snapshot.groups[0].members.push(OutputGroupMember {
        head: DisplayHeadId::from_raw(4),
        mapping: OutputHeadMapping::Fit,
    });
    value
}

#[test]
fn mode_ranges_must_tile_the_table_once_in_head_order() {
    let value = two_heads();
    let bytes = encode_output_file_topology(&value).unwrap();
    assert_eq!(bytes.len(), 388);
    assert_eq!(&bytes[160..162], &[1, 0]); // second head starts after first mode
    assert_eq!(decode_output_file_topology(&bytes, 9).unwrap(), value);
    for first_mode in [0u16, 2, 3, 2048, u16::MAX] {
        let mut bad = bytes.clone();
        bad[160..162].copy_from_slice(&first_mode.to_le_bytes());
        assert!(
            decode_output_file_topology(&bad, 9).is_err(),
            "first {first_mode}"
        );
    }
    // A physically present but unclaimed extra row must not be silently lost.
    let mut unclaimed = bytes;
    unclaimed[20..22].copy_from_slice(&4u16.to_le_bytes());
    unclaimed.splice(304..304, [0; 24]);
    assert!(decode_output_file_topology(&unclaimed, 9).is_err());
}

#[test]
fn topology_encoder_and_decoder_apply_the_existing_snapshot_invariants() {
    let valid = two_heads();
    let mut invalid = valid.clone();
    invalid.snapshot.heads[1].head = invalid.snapshot.heads[0].head;
    assert!(encode_output_file_topology(&invalid).is_err());
    let mut bytes = encode_output_file_topology(&valid).unwrap();
    bytes[128..136].copy_from_slice(&3u64.to_le_bytes());
    assert!(decode_output_file_topology(&bytes, 9).is_err());

    let mut invalid = valid.clone();
    invalid.snapshot.heads[1].modes[1].mode = invalid.snapshot.heads[1].modes[0].mode;
    assert!(encode_output_file_topology(&invalid).is_err());
    let mut bytes = encode_output_file_topology(&valid).unwrap();
    bytes[280..288].copy_from_slice(&6u64.to_le_bytes());
    assert!(decode_output_file_topology(&bytes, 9).is_err());

    for which in 0..7 {
        let mut invalid = valid.clone();
        match which {
            0 => invalid.snapshot.heads[0].connected = false,
            1 => invalid.snapshot.heads[0].current_mode = None,
            2 => invalid.snapshot.heads[0].current_mode = Some(DisplayModeId::from_raw(99)),
            3 => invalid.snapshot.primary_output = OutputId::from_raw(99),
            4 => {
                invalid.snapshot.groups[0].members.pop();
            }
            5 => invalid.snapshot.groups[0].members[1].head = DisplayHeadId::from_raw(3),
            _ => invalid.snapshot.groups[0].logical.width = 0,
        }
        assert!(
            encode_output_file_topology(&invalid).is_err(),
            "case {which}"
        );
    }
}

#[test]
fn disabled_heads_and_exactly_bounded_utf8_labels_are_preserved() {
    let mut value = two_heads();
    value.snapshot.heads[1].enabled = false;
    value.snapshot.heads[1].connected = false;
    value.snapshot.heads[1].current_mode = None;
    value.snapshot.groups[0].members.pop();
    value.snapshot.heads[0].label = "é".repeat(32);
    let bytes = encode_output_file_topology(&value).unwrap();
    assert_eq!(decode_output_file_topology(&bytes, 9).unwrap(), value);
    value.snapshot.heads[0].label.push('x');
    assert!(encode_output_file_topology(&value).is_err());
    value.snapshot.heads[0].label = "a".into();
    value.snapshot.heads[1].current_mode = Some(DisplayModeId::INVALID);
    assert!(encode_output_file_topology(&value).is_err());
}

#[test]
fn maximum_topology_fits_the_record_bound_and_excess_rows_refuse() {
    let mut value = snapshot();
    let original_head = value.snapshot.heads[0].clone();
    let original_group = value.snapshot.groups[0].clone();
    value.snapshot.heads.clear();
    value.snapshot.groups.clear();
    for id in 1..=16 {
        let mut head = original_head.clone();
        head.head = DisplayHeadId::from_raw(id);
        head.label = "x".repeat(64);
        head.current_mode = Some(DisplayModeId::from_raw(1));
        head.modes = (1..=128)
            .map(|mode| OutputModeDescriptor {
                mode: DisplayModeId::from_raw(mode),
                ..original_head.modes[0]
            })
            .collect();
        let mut group = original_group.clone();
        group.output = OutputId::from_raw(id);
        group.logical.x = (id as i32 - 1) * 800;
        group.members[0].head = head.head;
        value.snapshot.heads.push(head);
        value.snapshot.groups.push(group);
    }
    let body = encode_output_file_topology(&value).unwrap();
    assert_eq!(body.len() + OUTPUT_FILE_HEADER_BYTES, 52_216);
    assert_eq!(OUTPUT_FILE_MAX_TOPOLOGY_BYTES, 52_216);
    let record = encode_output_file_record(
        OutputFileHeader {
            kind: OutputFileKind::Topology,
            connection_epoch: 9,
            submission_id: 0,
            sequence: 0,
        },
        &body,
    )
    .unwrap();
    assert!(record.len() <= OUTPUT_FILE_MAX_BYTES);
    assert_eq!(decode_output_file_topology(&body, 9).unwrap(), value);
    for which in 0..3 {
        let mut bad = value.clone();
        match which {
            0 => bad.snapshot.heads.push(value.snapshot.heads[0].clone()),
            1 => bad.snapshot.groups.push(value.snapshot.groups[0].clone()),
            _ => bad.snapshot.heads[0]
                .modes
                .push(value.snapshot.heads[0].modes[0]),
        }
        assert!(encode_output_file_topology(&bad).is_err(), "case {which}");
    }
}

#[test]
fn four_member_groups_use_every_slot_and_mapping() {
    let mut value = snapshot();
    for id in 4..=6 {
        let mut head = value.snapshot.heads[0].clone();
        head.head = DisplayHeadId::from_raw(id);
        value.snapshot.heads.push(head);
        value.snapshot.groups[0].members.push(OutputGroupMember {
            head: DisplayHeadId::from_raw(id),
            mapping: match id {
                4 => OutputHeadMapping::Fit,
                5 => OutputHeadMapping::Cover,
                _ => OutputHeadMapping::Exact,
            },
        });
    }
    let bytes = encode_output_file_topology(&value).unwrap();
    assert_eq!(decode_output_file_topology(&bytes, 9).unwrap(), value);
    let extra_member = value.snapshot.groups[0].members[0];
    value.snapshot.groups[0].members.push(extra_member);
    assert!(encode_output_file_topology(&value).is_err());
}
