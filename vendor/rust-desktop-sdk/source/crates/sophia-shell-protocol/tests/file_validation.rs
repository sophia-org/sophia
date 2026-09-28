//! Regressions for three file-contract validation gaps: indicator text over
//! its fixed 32-byte field, duplicate persistent catalog identity names, and
//! the `Negotiated` welcome's descriptor and label bounds. Each rule is the
//! one the retiring socket codec already enforced.
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use sophia_shell_protocol::shell::encoding::ValueError;
use sophia_shell_protocol::shell::encoding::indicators::{
    decode_shell_indicator_snapshot, encode_shell_indicator_snapshot,
};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

fn object(kind: ShellFileKind, epoch: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: 0,
        sequence: 0,
    }
}

// ---------------------------------------------------------------------
// Indicators: label and layout are at most 32 UTF-8 bytes.
// ---------------------------------------------------------------------

fn snapshot(label: &str, layout: &str) -> ShellIndicatorSnapshot {
    ShellIndicatorSnapshot {
        connection_epoch: 7,
        generation: 3,
        active_output: Some(OutputId::from_raw(1)),
        statuses: vec![ShellOutputStatus {
            output: OutputId::from_raw(1),
            focus_bits: 1,
            layout: layout.to_owned(),
        }],
        indicators: vec![ShellIndicator {
            output: OutputId::from_raw(1),
            indicator: 11,
            action: 5,
            slot: 0,
            state_bits: 1,
            label: label.to_owned(),
        }],
    }
}

/// Encodes without letting a panic escape: a panic is the defect under test,
/// so it is reported as a failure with its message, not as a refusal.
fn encode_value(snapshot: &ShellIndicatorSnapshot) -> Result<Vec<u8>, ValueError> {
    match catch_unwind(AssertUnwindSafe(|| {
        encode_shell_indicator_snapshot(snapshot)
    })) {
        Ok(result) => result,
        Err(panic) => panic!(
            "encoder panicked instead of refusing: {:?}",
            panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or(panic.downcast_ref::<&str>().copied())
        ),
    }
}

fn encode_file(snapshot: &ShellIndicatorSnapshot) -> Result<Vec<u8>, ShellFilePayloadError> {
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(9),
        snapshot: snapshot.clone(),
    };
    match catch_unwind(AssertUnwindSafe(|| {
        encode_shell_file_indicators(object(ShellFileKind::Indicators, 7), &value)
    })) {
        Ok(result) => result,
        Err(_) => panic!("file encoder panicked instead of refusing"),
    }
}

/// UTF-8 texts at exactly 32 bytes (ASCII, two-byte and three-byte
/// characters) and one byte past it.
fn texts() -> (Vec<String>, Vec<String>) {
    let fits = vec![
        "x".repeat(32),
        "é".repeat(16),
        format!("{}ab", "€".repeat(10)),
        String::new(),
    ];
    let over = vec![
        "x".repeat(33),
        format!("{}a", "é".repeat(16)),
        "€".repeat(11),
        "x".repeat(200),
    ];
    for text in &fits {
        assert!(text.len() <= 32);
    }
    for text in &over {
        assert!(text.len() > 32);
    }
    (fits, over)
}

#[test]
fn indicator_label_is_bounded_in_utf8_bytes_on_encode() {
    let (fits, over) = texts();
    for label in &fits {
        let s = snapshot(label, "grid");
        let bytes = encode_value(&s).unwrap();
        assert_eq!(decode_shell_indicator_snapshot(&bytes).unwrap(), s);
        let file = encode_file(&s).unwrap();
        assert_eq!(decode_shell_file_indicators(&file).unwrap().snapshot, s);
    }
    for label in &over {
        let s = snapshot(label, "grid");
        assert_eq!(
            encode_value(&s),
            Err(ValueError::InvalidRecord("shell indicator label")),
            "{} bytes",
            label.len()
        );
        assert_eq!(
            encode_file(&s),
            Err(ShellFilePayloadError::Records(ValueError::InvalidRecord(
                "shell indicator label"
            )))
        );
    }
}

#[test]
fn indicator_output_status_layout_is_bounded_in_utf8_bytes_on_encode() {
    let (fits, over) = texts();
    for layout in &fits {
        let s = snapshot("A", layout);
        let bytes = encode_value(&s).unwrap();
        assert_eq!(decode_shell_indicator_snapshot(&bytes).unwrap(), s);
    }
    for layout in &over {
        let s = snapshot("A", layout);
        assert_eq!(
            encode_value(&s),
            Err(ValueError::InvalidRecord("shell indicator layout")),
            "{} bytes",
            layout.len()
        );
        assert_eq!(
            encode_file(&s),
            Err(ShellFilePayloadError::Records(ValueError::InvalidRecord(
                "shell indicator layout"
            )))
        );
    }
}

/// Where a one-byte text field sits: its u16 length (1), reserved u16 (0),
/// then the byte itself.
fn field(bytes: &[u8], marker: u8) -> usize {
    let needle = [1, 0, 0, 0, marker];
    let at = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("text field");
    assert_eq!(
        bytes.windows(needle.len()).filter(|w| *w == needle).count(),
        1,
        "the marker is unique"
    );
    at
}

/// Incoming records are refused by structure before any text is kept: a
/// declared length over the field, nonzero padding after the text, and bytes
/// that are not UTF-8, for both the label and the layout field.
#[test]
fn malformed_incoming_indicator_text_is_refused() {
    let valid = encode_value(&snapshot("Z", "Q")).unwrap();
    for marker in [b'Z', b'Q'] {
        let at = field(&valid, marker);
        let mut long = valid.clone();
        long[at..at + 2].copy_from_slice(&33u16.to_le_bytes());
        assert_eq!(
            decode_shell_indicator_snapshot(&long),
            Err(ValueError::CountTooLarge { count: 33, max: 32 })
        );
        let mut padded = valid.clone();
        padded[at + 4 + 31] = b'x';
        assert_eq!(
            decode_shell_indicator_snapshot(&padded),
            Err(ValueError::ReservedNonZero(1))
        );
        let mut invalid = valid.clone();
        invalid[at + 4] = 0xff;
        assert_eq!(
            decode_shell_indicator_snapshot(&invalid),
            Err(ValueError::InvalidRecord("shell file text"))
        );
        // Control: the untouched record decodes.
        assert!(decode_shell_indicator_snapshot(&valid).is_ok());
    }
    // A full-width multi-byte label also arrives intact through the file.
    let s = snapshot(&"é".repeat(16), &"€".repeat(10));
    let file = encode_file(&s).unwrap();
    assert_eq!(decode_shell_file_indicators(&file).unwrap().snapshot, s);
}

// ---------------------------------------------------------------------
// Persistent catalog: distinct slots cannot share one identity name.
// ---------------------------------------------------------------------

fn catalog(names: &[&str]) -> ShellPersistentCatalog {
    let entries = (1..=names.len() as u16)
        .map(|slot| ShellApplicationDescriptor {
            slot,
            available: true,
            label: format!("App {slot}"),
            keywords: String::new(),
        })
        .collect();
    ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch: 1,
            generation: 2,
            entries,
        },
        identities: names
            .iter()
            .enumerate()
            .map(|(i, name)| (i as u16 + 1, (*name).to_owned()))
            .collect(),
    }
}

fn file_catalog(value: ShellPersistentCatalog) -> ShellFileCatalog {
    ShellFileCatalog {
        transaction: TransactionId::from_raw(5),
        catalog: value,
    }
}

#[test]
fn persistent_catalog_refuses_one_identity_name_on_two_slots() {
    for names in [
        ["desktop:same", "desktop:same"].as_slice(),
        &["registered:a", "desktop:b", "registered:a"],
    ] {
        let value = catalog(names);
        assert_eq!(
            validate_shell_persistent_catalog(&value),
            Err(InvalidRecord("persistent catalog identity bijection"))
        );
        assert_eq!(
            encode_shell_file_catalog(object(ShellFileKind::Catalog, 1), &file_catalog(value)),
            Err(ShellFilePayloadError::Records(ValueError::InvalidRecord(
                "persistent catalog identity bijection"
            )))
        );
    }
}

/// A malformed incoming catalog: encode distinct names of one length, then
/// rewrite the second into the first.
#[test]
fn incoming_catalog_with_a_duplicate_identity_name_is_refused() {
    let distinct = file_catalog(catalog(&["desktop:aa", "desktop:ab"]));
    let mut bytes =
        encode_shell_file_catalog(object(ShellFileKind::Catalog, 1), &distinct).unwrap();
    assert_eq!(decode_shell_file_catalog(&bytes).unwrap(), distinct);
    let at = bytes
        .windows(10)
        .position(|w| w == b"desktop:ab")
        .expect("second identity");
    bytes[at + 9] = b'a';
    assert_eq!(
        decode_shell_file_catalog(&bytes),
        Err(ShellFilePayloadError::Records(ValueError::InvalidRecord(
            "persistent catalog identity bijection"
        )))
    );
}

/// Distinct names stay valid, and a plain launcher catalog (no identities,
/// labels free to repeat) is unaffected.
#[test]
fn distinct_identities_and_plain_catalogs_stay_valid() {
    let distinct = catalog(&["registered:a", "desktop:a", "registered:b"]);
    assert_eq!(validate_shell_persistent_catalog(&distinct), Ok(()));
    let value = file_catalog(distinct);
    let bytes = encode_shell_file_catalog(object(ShellFileKind::Catalog, 1), &value).unwrap();
    assert_eq!(decode_shell_file_catalog(&bytes).unwrap(), value);

    let mut plain = catalog(&["desktop:x", "desktop:y"]);
    plain.identities = BTreeMap::new();
    for entry in &mut plain.catalog.entries {
        entry.label = "Same".to_owned();
    }
    assert_eq!(validate_shell_application_catalog(&plain.catalog), Ok(()));
    let value = file_catalog(plain);
    let bytes = encode_shell_file_catalog(object(ShellFileKind::Catalog, 1), &value).unwrap();
    assert_eq!(decode_shell_file_catalog(&bytes).unwrap(), value);
}

// ---------------------------------------------------------------------
// Negotiated: 1..=16 descriptors and 1..=128 label bytes.
// ---------------------------------------------------------------------

fn negotiated_header() -> ShellFileHeader {
    ShellFileHeader {
        kind: ShellFileKind::Negotiated,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    }
}

fn negotiated(revision: u16, descriptors: u16, labels: u16) -> ShellFileNegotiated {
    ShellFileNegotiated {
        welcome: ShellV1ServerWelcome {
            selected_revision: revision,
            connection_epoch: 1,
            capabilities: 0x1fa1,
            max_descriptors: descriptors,
            max_label_bytes: labels,
            max_pending_activations: 16,
        },
        limits_published: revision >= 5,
    }
}

#[test]
fn negotiated_refuses_zero_or_excess_descriptor_and_label_bounds_on_encode() {
    for (descriptors, labels) in [
        (0, 128),
        (17, 128),
        (u16::MAX, 128),
        (16, 0),
        (16, 129),
        (16, u16::MAX),
    ] {
        assert_eq!(
            encode_shell_file_negotiated(negotiated_header(), negotiated(6, descriptors, labels)),
            Err(ShellFilePayloadError::Value),
            "{descriptors} descriptors, {labels} label bytes"
        );
    }
}

/// An incoming `Negotiated` event carrying a bound the session vocabulary
/// cannot have: descriptors at body offset 20, label bytes at 22.
#[test]
fn incoming_negotiated_with_zero_or_excess_bounds_is_refused() {
    let valid = encode_shell_file_negotiated(negotiated_header(), negotiated(6, 16, 128)).unwrap();
    for (offset, value) in [(20, 0u16), (20, 17), (22, 0), (22, 129)] {
        let mut bytes = valid.clone();
        let at = SHELL_FILE_HEADER_BYTES + offset;
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
        assert_eq!(
            decode_shell_file_negotiated(&bytes),
            Err(ShellFilePayloadError::Value),
            "offset {offset} value {value}"
        );
    }
    assert_eq!(
        decode_shell_file_negotiated(&valid).unwrap(),
        negotiated(6, 16, 128)
    );
}

/// The bounds change nothing else: every revision and the capability mask
/// round-trip at both extremes of each bound.
#[test]
fn negotiated_bounds_preserve_every_revision_and_capability_mask() {
    for revision in 1..=8 {
        for (descriptors, labels) in [(1, 1), (16, 128), (1, 128), (16, 1)] {
            let value = negotiated(revision, descriptors, labels);
            let bytes = encode_shell_file_negotiated(negotiated_header(), value).unwrap();
            let decoded = decode_shell_file_negotiated(&bytes).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(decoded.welcome.capabilities, 0x1fa1);
            assert_eq!(decoded.welcome.selected_revision, revision);
        }
    }
}

// ---------------------------------------------------------------------
// Negotiated: 1..=16 pending activations, for every profile.
// ---------------------------------------------------------------------

fn with_pending(pending: u16) -> ShellFileNegotiated {
    let mut value = negotiated(6, 16, 128);
    value.welcome.max_pending_activations = pending;
    value
}

#[test]
fn negotiated_pending_activations_are_one_to_sixteen_on_encode() {
    for pending in [1, 16] {
        let bytes =
            encode_shell_file_negotiated(negotiated_header(), with_pending(pending)).unwrap();
        assert_eq!(
            decode_shell_file_negotiated(&bytes).unwrap(),
            with_pending(pending)
        );
    }
    for pending in [0, 17, u16::MAX] {
        assert_eq!(
            encode_shell_file_negotiated(negotiated_header(), with_pending(pending)),
            Err(ShellFilePayloadError::Value),
            "{pending} pending activations"
        );
    }
}

/// An incoming `Negotiated` event with pending activations at body offset 24
/// outside 1..=16 is refused, whichever revision was selected.
#[test]
fn incoming_negotiated_with_zero_or_excess_pending_activations_is_refused() {
    for revision in [6, 7, 8] {
        let valid =
            encode_shell_file_negotiated(negotiated_header(), negotiated(revision, 16, 128))
                .unwrap();
        let at = SHELL_FILE_HEADER_BYTES + 24;
        for pending in [0u16, 17, u16::MAX] {
            let mut bytes = valid.clone();
            bytes[at..at + 2].copy_from_slice(&pending.to_le_bytes());
            assert_eq!(
                decode_shell_file_negotiated(&bytes),
                Err(ShellFilePayloadError::Value),
                "revision {revision} pending {pending}"
            );
        }
        let mut one = valid.clone();
        one[at..at + 2].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            decode_shell_file_negotiated(&one)
                .unwrap()
                .welcome
                .max_pending_activations,
            1
        );
    }
}
