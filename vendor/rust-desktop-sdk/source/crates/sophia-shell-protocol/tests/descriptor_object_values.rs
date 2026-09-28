//! Literal offset controls for the proposed native values; not socket frames.
use sophia_shell_protocol::{shell::encoding::ValueError, shell::encoding::descriptor::*, *};

fn u16_at(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn u64_at(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn text_at(bytes: &mut [u8], at: usize, text: &str) {
    u16_at(bytes, at, text.len() as u16);
    bytes[at + 4..at + 4 + text.len()].copy_from_slice(text.as_bytes());
}
fn descriptor(slot: u16) -> ShellV1Descriptor {
    ShellV1Descriptor {
        slot,
        generation: 4,
        label: Some(DisplayLabel {
            text: "one".into(),
            redacted: true,
        }),
        trust_level: TrustLevel::Trusted,
        attention: AttentionState::Notice,
        action: ToplevelActionCapabilityRef {
            token: 6,
            issuer_epoch: 7,
            issuer_revocation_epoch: 8,
            recipient_epoch: 9,
            target_slot: slot,
            target_generation: 4,
        },
    }
}
fn row(slot: u16) -> Vec<u8> {
    let mut bytes = vec![0; 196];
    for (at, n) in [(0, slot), (2, 1), (4, 1), (6, 1), (8, 1), (52, slot)] {
        u16_at(&mut bytes, at, n);
    }
    for (at, n) in [(12, 4), (20, 6), (28, 7), (36, 8), (44, 9), (56, 4)] {
        u64_at(&mut bytes, at, n);
    }
    text_at(&mut bytes, 64, "one");
    bytes
}
fn snapshot() -> ShellV1DescriptorSnapshot {
    ShellV1DescriptorSnapshot {
        connection_epoch: 9,
        snapshot_generation: 2,
        output: OutputId::from_raw(3),
        output_generation: 4,
        broker_epoch: 7,
        broker_revocation_epoch: 8,
        descriptors: vec![descriptor(1)],
    }
}
fn literal_snapshot() -> Vec<u8> {
    let mut bytes = vec![0; 56];
    for (at, n) in [(0, 9), (8, 2), (16, 3), (24, 4), (32, 7), (40, 8)] {
        u64_at(&mut bytes, at, n);
    }
    u16_at(&mut bytes, 48, 1);
    bytes.extend(row(1));
    bytes
}

#[test]
fn descriptors_match_literal_offsets_and_preserve_label_absence() {
    let bytes = literal_snapshot();
    assert_eq!(encode_shell_descriptors_value(&snapshot()).unwrap(), bytes);
    assert_eq!(decode_shell_descriptors_value(&bytes).unwrap(), snapshot());
    let mut value = snapshot();
    value.descriptors[0].label = None;
    let bytes = encode_shell_descriptors_value(&value).unwrap();
    assert_eq!(&bytes[56 + 6..56 + 10], &[0; 4]);
    assert!(bytes[56 + 64..].iter().all(|v| *v == 0));
    assert_eq!(decode_shell_descriptors_value(&bytes).unwrap(), value);
}

#[test]
fn descriptor_corruptions_are_refused() {
    let original = literal_snapshot();
    for len in 0..original.len() {
        assert!(
            decode_shell_descriptors_value(&original[..len]).is_err(),
            "length {len}"
        );
    }
    let mut extra = original.clone();
    extra.push(0);
    assert!(matches!(
        decode_shell_descriptors_value(&extra),
        Err(ValueError::TrailingBytes(1))
    ));
    for at in [50, 52, 56 + 10, 56 + 54, 56 + 66, 56 + 71] {
        let mut bytes = original.clone();
        bytes[at] = 1;
        assert!(
            decode_shell_descriptors_value(&bytes).is_err(),
            "reserved/padding offset {at}"
        );
    }
    for (at, n) in [
        (56 + 2, 4),
        (56 + 4, 3),
        (56 + 6, 2),
        (56 + 8, 2),
        (56 + 64, 129),
        (48, 17),
    ] {
        let mut bytes = original.clone();
        u16_at(&mut bytes, at, n);
        assert!(
            decode_shell_descriptors_value(&bytes).is_err(),
            "invalid field {at}"
        );
    }
    let mut bytes = original.clone();
    bytes[56 + 68] = 0xff;
    assert!(decode_shell_descriptors_value(&bytes).is_err());
    let mut bytes = original.clone();
    u16_at(&mut bytes, 56 + 6, 0);
    assert!(decode_shell_descriptors_value(&bytes).is_err());
    // Absent redaction is cleared too, but nonempty absent storage still refuses.
    u16_at(&mut bytes, 56 + 8, 0);
    assert!(decode_shell_descriptors_value(&bytes).is_err());
    let mut value = snapshot();
    value.descriptors[0].label.as_mut().unwrap().text = "x".repeat(129);
    assert!(encode_shell_descriptors_value(&value).is_err());
}

#[test]
fn tabs_are_headers_then_rows_with_no_repeated_group_identity() {
    let value = ShellTabSnapshot {
        connection_epoch: 9,
        generation: 2,
        groups: vec![
            ShellTabGroup {
                slot: 11,
                output: OutputId::from_raw(3),
                focused: true,
                selected_slot: Some(1),
                entries: vec![descriptor(1)],
            },
            ShellTabGroup {
                slot: 12,
                output: OutputId::from_raw(4),
                focused: false,
                selected_slot: Some(2),
                entries: vec![descriptor(2)],
            },
        ],
    };
    let mut literal = vec![0; 72];
    u64_at(&mut literal, 0, 9);
    u64_at(&mut literal, 8, 2);
    u16_at(&mut literal, 16, 2);
    u16_at(&mut literal, 18, 2);
    for (start, slot, output, selected, focused) in [(24, 11, 3, 1, 1), (48, 12, 4, 2, 0)] {
        u64_at(&mut literal, start, slot);
        u64_at(&mut literal, start + 8, output);
        u16_at(&mut literal, start + 16, selected);
        u16_at(&mut literal, start + 18, focused);
        u16_at(&mut literal, start + 20, 1);
    }
    literal.extend(row(1));
    literal.extend(row(2));
    assert_eq!(encode_shell_tabs_value(&value).unwrap(), literal);
    assert_eq!(decode_shell_tabs_value(&literal).unwrap(), value);
    for (at, n) in [
        (16, 1025),
        (18, 2049),
        (24 + 18, 2),
        (24 + 20, 3),
        (48 + 20, 0),
        (48 + 16, 1),
        (48 + 22, 1),
    ] {
        let mut bytes = literal.clone();
        u16_at(&mut bytes, at, n);
        assert!(
            decode_shell_tabs_value(&bytes).is_err(),
            "invalid field {at}"
        );
    }
    let mut bytes = literal.clone();
    u64_at(&mut bytes, 48, 11);
    assert!(decode_shell_tabs_value(&bytes).is_err());
    for len in 0..literal.len() {
        assert!(decode_shell_tabs_value(&literal[..len]).is_err());
    }
    let mut bytes = literal;
    bytes.push(0);
    assert!(matches!(
        decode_shell_tabs_value(&bytes),
        Err(ValueError::TrailingBytes(1))
    ));
}

#[test]
fn maximum_object_sizes_fit_the_proposed_caps() {
    let mut descriptors = snapshot();
    descriptors.descriptors = (1..=16).map(descriptor).collect();
    let bytes = encode_shell_descriptors_value(&descriptors).unwrap();
    assert_eq!(bytes.len() + 40, 3232);
    assert_eq!(decode_shell_descriptors_value(&bytes).unwrap(), descriptors);
    let tabs = ShellTabSnapshot {
        connection_epoch: 9,
        generation: 2,
        groups: (0..1024)
            .map(|n| {
                let first = (n * 2 + 1) as u16;
                ShellTabGroup {
                    slot: n + 1,
                    output: OutputId::from_raw(3),
                    focused: false,
                    selected_slot: Some(first),
                    entries: vec![descriptor(first), descriptor(first + 1)],
                }
            })
            .collect(),
    };
    let bytes = encode_shell_tabs_value(&tabs).unwrap();
    assert_eq!(bytes.len() + 40, 426048);
    assert_eq!(decode_shell_tabs_value(&bytes).unwrap(), tabs);
    let shortcuts = ShellShortcutCatalog {
        connection_epoch: 9,
        generation: 2,
        entries: (1..=256)
            .map(|slot| ShellShortcut {
                slot,
                chord: "K".repeat(64),
                action: "A".repeat(128),
                label: Some("L".repeat(128)),
                group: Some("G".repeat(64)),
            })
            .collect(),
    };
    let bytes = encode_shell_shortcuts_value(&shortcuts).unwrap();
    assert_eq!(bytes.len() + 40, 104512);
    assert_eq!(decode_shell_shortcuts_value(&bytes).unwrap(), shortcuts);
}

#[test]
fn shortcut_literal_layout_and_optional_presence_are_strict() {
    let value = ShellShortcutCatalog {
        connection_epoch: 9,
        generation: 2,
        entries: vec![ShellShortcut {
            slot: 1,
            chord: "K".into(),
            action: "A".into(),
            label: Some("L".into()),
            group: None,
        }],
    };
    let mut literal = vec![0; 432];
    u64_at(&mut literal, 0, 9);
    u64_at(&mut literal, 8, 2);
    u16_at(&mut literal, 16, 1);
    u16_at(&mut literal, 24, 1);
    u16_at(&mut literal, 26, 1);
    text_at(&mut literal, 32, "K");
    text_at(&mut literal, 100, "A");
    text_at(&mut literal, 232, "L");
    assert_eq!(encode_shell_shortcuts_value(&value).unwrap(), literal);
    assert_eq!(decode_shell_shortcuts_value(&literal).unwrap(), value);
    for (at, n) in [
        (16, 257),
        (18, 1),
        (20, 1),
        (26, 2),
        (28, 2),
        (30, 1),
        (34, 1),
        (32, 65),
        (100, 129),
        (232, 129),
        (364, 65),
    ] {
        let mut bytes = literal.clone();
        u16_at(&mut bytes, at, n);
        assert!(decode_shell_shortcuts_value(&bytes).is_err(), "field {at}");
    }
    let mut bytes = literal.clone();
    u16_at(&mut bytes, 26, 0);
    assert!(decode_shell_shortcuts_value(&bytes).is_err());
    let mut bytes = literal.clone();
    text_at(&mut bytes, 364, "G");
    assert!(decode_shell_shortcuts_value(&bytes).is_err());
    let mut bytes = literal.clone();
    u16_at(&mut bytes, 28, 1);
    assert!(decode_shell_shortcuts_value(&bytes).is_err());
    for at in [37, 105, 237] {
        let mut bytes = literal.clone();
        bytes[at] = 1;
        assert!(decode_shell_shortcuts_value(&bytes).is_err());
    }
    for len in 0..literal.len() {
        assert!(decode_shell_shortcuts_value(&literal[..len]).is_err());
    }
    let mut bytes = literal;
    bytes.push(0);
    assert!(matches!(
        decode_shell_shortcuts_value(&bytes),
        Err(ValueError::TrailingBytes(1))
    ));
}
