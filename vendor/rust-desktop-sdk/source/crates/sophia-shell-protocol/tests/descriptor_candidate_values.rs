//! Candidate layout controls use literal offsets from the proposed contract.
use sophia_shell_protocol::{shell::encoding::ValueError, shell::encoding::descriptor::*, *};

fn word(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn wide(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn prefix(len: usize, values: &[u64]) -> Vec<u8> {
    let mut bytes = vec![0; len];
    for (i, value) in values.iter().enumerate() {
        wide(&mut bytes, i * 8, *value);
    }
    bytes
}
fn text(bytes: &mut [u8], at: usize, value: &str) {
    word(bytes, at, value.len() as u16);
    bytes[at + 4..at + 4 + value.len()].copy_from_slice(value.as_bytes());
}
fn check<T: std::fmt::Debug + PartialEq>(
    value: &T,
    literal: &[u8],
    encode: fn(&T) -> Result<Vec<u8>, ValueError>,
    decode: fn(&[u8]) -> Result<T, ValueError>,
) {
    assert_eq!(encode(value).unwrap(), literal);
    assert_eq!(&decode(literal).unwrap(), value);
    for len in 0..literal.len() {
        assert!(decode(&literal[..len]).is_err(), "truncation {len}");
    }
    let mut extra = literal.to_vec();
    extra.push(0);
    assert!(matches!(decode(&extra), Err(ValueError::TrailingBytes(1))));
}
fn bad<T>(literal: &[u8], fields: &[(usize, u16)], decode: fn(&[u8]) -> Result<T, ValueError>) {
    for &(at, n) in fields {
        let mut bytes = literal.to_vec();
        word(&mut bytes, at, n);
        assert!(decode(&bytes).is_err(), "field {at} = {n}");
    }
}
fn descriptor() -> ShellV1Candidate {
    ShellV1Candidate {
        connection_epoch: 1,
        snapshot_generation: 2,
        candidate_generation: 3,
        output: OutputId::from_raw(4),
        visible: true,
        selected_slot: Some(5),
        reservation: Some(ShellV1WorkAreaReservation {
            edge: ShellV1ReservationEdge::Top,
            thickness_px: 512,
        }),
        entries: vec![ShellV1CandidateEntry {
            slot: 5,
            generation: 6,
        }],
    }
}
fn reference() -> ShellReferenceCandidate {
    ShellReferenceCandidate {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        output: OutputId::from_raw(5),
        visible: true,
        page: 6,
        style: ShellReferenceStyle {
            body_size: 12,
            title_size: 16,
            padding: 8,
            row_gap: 4,
            key_gap: 5,
            column_gap: 6,
            border: 2,
            margin: 7,
            columns: 2,
            colors: [
                0x11223344, 0xff223344, 0xff334455, 0xff445566, 0xff556677, 0xff667788,
            ],
            title: "Title".into(),
        },
        entries: vec![ShellReferenceEntry {
            slot: 7,
            key: "K".into(),
            label: "Label".into(),
        }],
    }
}
fn launcher() -> ShellLauncherCandidate {
    ShellLauncherCandidate {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        output: OutputId::from_raw(5),
        visible: true,
        selected: 4096,
        entries: vec![4096],
        font_size: 14,
        colors: [0x11223344, 0xff223344, 0xff334455, 0xff445566],
    }
}

#[test]
fn descriptor_candidate_layout_reservation_and_selection() {
    let mut value = descriptor();
    for (edge, tag) in [
        (ShellV1ReservationEdge::Top, 1),
        (ShellV1ReservationEdge::Bottom, 2),
        (ShellV1ReservationEdge::Left, 3),
        (ShellV1ReservationEdge::Right, 4),
    ] {
        value.reservation.as_mut().unwrap().edge = edge;
        let mut literal = prefix(56, &[1, 2, 3, 4]);
        for (at, n) in [(32, 1), (34, tag), (36, 512), (38, 5), (40, 1), (44, 5)] {
            word(&mut literal, at, n);
        }
        wide(&mut literal, 48, 6);
        check(
            &value,
            &literal,
            encode_shell_descriptor_candidate_value,
            decode_shell_descriptor_candidate_value,
        );
        bad(
            &literal,
            &[
                (32, 2),
                (34, 0),
                (34, 5),
                (36, 0),
                (36, 513),
                (38, 0),
                (38, 6),
                (40, 17),
                (42, 1),
                (44, 0),
                (46, 1),
                (48, 0),
            ],
            decode_shell_descriptor_candidate_value,
        );
    }
    value.reservation = None;
    value.visible = false;
    value.selected_slot = None;
    value.entries.clear();
    let literal = prefix(44, &[1, 2, 3, 4]);
    check(
        &value,
        &literal,
        encode_shell_descriptor_candidate_value,
        decode_shell_descriptor_candidate_value,
    );
    bad(
        &literal,
        &[(32, 1), (34, 1), (36, 1), (38, 5)],
        decode_shell_descriptor_candidate_value,
    );
    value.visible = true;
    assert!(encode_shell_descriptor_candidate_value(&value).is_err());
}

#[test]
fn tab_candidate_is_a_complete_ordered_group_table() {
    let value = ShellTabCandidate {
        connection_epoch: 1,
        snapshot_generation: 2,
        candidate_generation: 3,
        groups: vec![7, 6],
    };
    let mut literal = prefix(44, &[1, 2, 3]);
    word(&mut literal, 24, 2);
    wide(&mut literal, 28, 7);
    wide(&mut literal, 36, 6);
    check(
        &value,
        &literal,
        encode_shell_tab_candidate_value,
        decode_shell_tab_candidate_value,
    );
    bad(
        &literal,
        &[
            (0, 0),
            (8, 0),
            (16, 0),
            (24, 1025),
            (26, 1),
            (28, 0),
            (36, 7),
        ],
        decode_shell_tab_candidate_value,
    );
    // Ordering and the tab namespace bit are checked by the owner, not invented by the codec.
    let mut empty = value;
    empty.groups.clear();
    let literal = prefix(28, &[1, 2, 3]);
    check(
        &empty,
        &literal,
        encode_shell_tab_candidate_value,
        decode_shell_tab_candidate_value,
    );
}

#[test]
fn reference_candidate_style_and_rows_have_independent_literal_offsets() {
    let value = reference();
    let mut literal = prefix(428, &[1, 2, 3, 4, 5]);
    for (at, n) in [
        (40, 1),
        (42, 6),
        (44, 1),
        (48, 12),
        (50, 16),
        (52, 8),
        (54, 4),
        (56, 5),
        (58, 6),
        (60, 2),
        (62, 7),
        (64, 2),
        (224, 7),
    ] {
        word(&mut literal, at, n);
    }
    literal[68..92].copy_from_slice(&[
        0x44, 0x33, 0x22, 0x11, 0x44, 0x33, 0x22, 0xff, 0x55, 0x44, 0x33, 0xff, 0x66, 0x55, 0x44,
        0xff, 0x77, 0x66, 0x55, 0xff, 0x88, 0x77, 0x66, 0xff,
    ]);
    text(&mut literal, 92, "Title");
    text(&mut literal, 228, "K");
    text(&mut literal, 296, "Label");
    check(
        &value,
        &literal,
        encode_shell_reference_candidate_value,
        decode_shell_reference_candidate_value,
    );
    bad(
        &literal,
        &[
            (40, 2),
            (44, 257),
            (46, 1),
            (48, 7),
            (48, 33),
            (50, 49),
            (52, 65),
            (54, 33),
            (56, 65),
            (58, 65),
            (60, 17),
            (62, 129),
            (64, 0),
            (64, 5),
            (66, 1),
            (74, 0),
            (92, 129),
            (94, 1),
            (102, 1),
            (224, 0),
            (226, 1),
            (228, 65),
            (230, 1),
            (234, 1),
            (296, 129),
        ],
        decode_shell_reference_candidate_value,
    );
    let mut invalid = literal;
    invalid[232] = 0xff;
    assert!(decode_shell_reference_candidate_value(&invalid).is_err());
    let mut hidden = value;
    hidden.visible = false;
    hidden.entries.clear();
    hidden.page = u16::MAX;
    assert_eq!(
        decode_shell_reference_candidate_value(
            &encode_shell_reference_candidate_value(&hidden).unwrap()
        )
        .unwrap(),
        hidden
    );
}

#[test]
fn launcher_candidate_has_no_socket_fragment_header() {
    let value = launcher();
    let mut literal = prefix(66, &[1, 2, 3, 4, 5]);
    for (at, n) in [(40, 1), (42, 4096), (44, 1), (46, 14), (64, 4096)] {
        word(&mut literal, at, n);
    }
    literal[48..64].copy_from_slice(&[
        0x44, 0x33, 0x22, 0x11, 0x44, 0x33, 0x22, 0xff, 0x55, 0x44, 0x33, 0xff, 0x66, 0x55, 0x44,
        0xff,
    ]);
    check(
        &value,
        &literal,
        encode_shell_launcher_candidate_value,
        decode_shell_launcher_candidate_value,
    );
    bad(
        &literal,
        &[
            (40, 2),
            (42, 4095),
            (44, 33),
            (46, 9),
            (46, 33),
            (54, 0),
            (64, 0),
            (64, 4097),
        ],
        decode_shell_launcher_candidate_value,
    );
    let mut empty = value;
    empty.selected = 0;
    empty.entries.clear();
    // An empty visible launcher is valid; descriptor visibility rules do not apply here.
    assert_eq!(
        decode_shell_launcher_candidate_value(
            &encode_shell_launcher_candidate_value(&empty).unwrap()
        )
        .unwrap(),
        empty
    );
}

#[test]
fn maximum_candidates_fit_individual_caps_and_one_past_is_refused() {
    let mut descriptor = descriptor();
    descriptor.selected_slot = Some(1);
    descriptor.entries = (1..=16)
        .map(|slot| ShellV1CandidateEntry {
            slot,
            generation: 6,
        })
        .collect();
    let bytes = encode_shell_descriptor_candidate_value(&descriptor).unwrap();
    assert_eq!(bytes.len() + 40, 276);
    assert_eq!(
        decode_shell_descriptor_candidate_value(&bytes).unwrap(),
        descriptor
    );
    descriptor.entries.push(ShellV1CandidateEntry {
        slot: 17,
        generation: 6,
    });
    assert!(encode_shell_descriptor_candidate_value(&descriptor).is_err());
    let mut tabs = ShellTabCandidate {
        connection_epoch: 1,
        snapshot_generation: 2,
        candidate_generation: 3,
        groups: (1..=1024).collect(),
    };
    let bytes = encode_shell_tab_candidate_value(&tabs).unwrap();
    assert_eq!(bytes.len() + 40, 8260);
    assert_eq!(decode_shell_tab_candidate_value(&bytes).unwrap(), tabs);
    tabs.groups.push(1025);
    assert!(encode_shell_tab_candidate_value(&tabs).is_err());
    let mut reference = reference();
    reference.entries = (1..=256)
        .map(|slot| ShellReferenceEntry {
            slot,
            key: "K".repeat(64),
            label: "L".repeat(128),
        })
        .collect();
    let bytes = encode_shell_reference_candidate_value(&reference).unwrap();
    assert_eq!(bytes.len() + 40, 52488);
    assert_eq!(
        decode_shell_reference_candidate_value(&bytes).unwrap(),
        reference
    );
    reference.entries.push(ShellReferenceEntry {
        slot: 257,
        key: "K".into(),
        label: "L".into(),
    });
    assert!(encode_shell_reference_candidate_value(&reference).is_err());
    let mut launcher = launcher();
    launcher.selected = 32;
    launcher.entries = (1..=32).collect();
    let bytes = encode_shell_launcher_candidate_value(&launcher).unwrap();
    assert_eq!(bytes.len() + 40, 168);
    assert_eq!(
        decode_shell_launcher_candidate_value(&bytes).unwrap(),
        launcher
    );
    launcher.entries.push(33);
    assert!(encode_shell_launcher_candidate_value(&launcher).is_err());
}
