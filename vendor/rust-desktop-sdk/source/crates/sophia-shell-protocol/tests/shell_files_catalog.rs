//! Coverage for the t252 B5 persistent catalog (r8) file records in
//! `sophia_shell_protocol::shell_files`: the whole `Catalog` object (plain or with
//! r8 identities), the catalog activation-outcome/activate records and the
//! whole `CatalogCandidate`.
// This binary only exercises the catalog fixtures; the rest of the shared
// `support` module (built for `shell_files_kdl.rs`, which uses all of it) is
// intentionally unused here.
#[allow(dead_code)]
#[path = "support/shell_files_kdl/mod.rs"]
mod support;

use sophia_shell_protocol::shell::encoding::ValueError;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use std::collections::BTreeMap;
use support::fixtures_catalog as fx;

fn object_header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    }
}

fn event_header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    }
}

fn candidate_header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    }
}

// ---------------------------------------------------------------------
// Catalog: the whole object, plain or with r8 identities.
// ---------------------------------------------------------------------

#[test]
fn catalog_round_trips() {
    let (tx, catalog, ..) = fx::catalog();
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    let encoded = encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap();
    assert_eq!(decode_shell_file_catalog(&encoded).unwrap(), value);
}

#[test]
fn catalog_plain_without_identities_round_trips() {
    let (tx, mut catalog, ..) = fx::catalog();
    catalog.identities.clear();
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    let encoded = encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap();
    let decoded = decode_shell_file_catalog(&encoded).unwrap();
    assert_eq!(decoded, value);
    assert!(decoded.catalog.identities.is_empty());
}

#[test]
fn catalog_refuses_zero_transaction() {
    let (_, catalog, ..) = fx::catalog();
    let value = ShellFileCatalog {
        transaction: TransactionId::INVALID,
        catalog,
    };
    assert_eq!(
        encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn catalog_refuses_reserved_nonzero() {
    let (tx, catalog, ..) = fx::catalog();
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    let encoded = encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap();
    let mut bad = encoded.clone();
    // Prefix reserved u32 at body offset 28, record offset 32 + 28 = 60.
    bad[60..64].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn catalog_refuses_entry_count_above_maximum() {
    let (tx, catalog, ..) = fx::catalog();
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    let encoded = encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap();
    let mut bad = encoded.clone();
    // `entry_count` sits at body offset 24, record offset 32 + 24 = 56.
    bad[56..58].copy_from_slice(&4097u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::CountTooLarge {
            count: 4097,
            max: SOPHIA_SHELL_MAX_APPLICATIONS,
        })
    );
}

#[test]
fn catalog_refuses_a_missing_identity() {
    let (tx, mut catalog, ..) = fx::catalog();
    catalog.identities.remove(&2);
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    // No partial catalog: one entry with an identity and one without fails
    // the bijection check on encode.
    assert_eq!(
        encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord(
            "persistent catalog identity bijection"
        ))
    );
}

#[test]
fn catalog_refuses_an_identity_with_no_recognised_prefix() {
    let (tx, mut catalog, ..) = fx::catalog();
    catalog.identities.insert(1, "unprefixed-name".to_owned());
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    assert_eq!(
        encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord(
            "persistent catalog identity bijection"
        ))
    );
}

#[test]
fn catalog_refuses_the_identities_present_flag_disagreeing_with_the_rows() {
    let (tx, catalog, ..) = fx::catalog();
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    let encoded = encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap();
    let mut bad = encoded.clone();
    // `identities_present` sits at body offset 26, record offset 32 + 26 = 58;
    // every entry actually carries an identity, so clearing this flag lies.
    bad[58..60].copy_from_slice(&0u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("shell catalog identities flag"))
    );
}

#[test]
fn catalog_refuses_trailing_bytes() {
    let (tx, catalog, ..) = fx::catalog();
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(tx),
        catalog,
    };
    let encoded = encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap();
    let mut trailing = encoded;
    trailing.push(0);
    let len = trailing.len() as u32;
    trailing[0..4].copy_from_slice(&len.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog(&trailing).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::TrailingBytes(1))
    );
}

#[test]
fn catalog_decode_refuses_bytes_over_the_object_cap() {
    let bytes = vec![0u8; SHELL_FILE_MAX_OBJECT_BYTES + 1];
    assert_eq!(
        decode_shell_file_catalog(&bytes).unwrap_err(),
        ShellFileCodecError::Length.into()
    );
}

/// The r8 catalog at its maximum within the 4 MiB object cap: 4096 entries,
/// each at its own maxima (a 128-byte label, a 256-byte keyword string and a
/// 256-byte identity) and its own identity.
#[test]
fn maximal_r8_catalog_fits_within_4_mib() {
    let label = "l".repeat(128);
    let keywords = "k".repeat(256);
    let entries: Vec<_> = (1..=SOPHIA_SHELL_MAX_APPLICATIONS as u16)
        .map(|slot| ShellApplicationDescriptor {
            slot,
            available: true,
            label: label.clone(),
            keywords: keywords.clone(),
        })
        .collect();
    let identities: BTreeMap<u16, String> = (1..=SOPHIA_SHELL_MAX_APPLICATIONS as u16)
        .map(|slot| (slot, format!("registered:{}", "i".repeat(245))))
        .collect();
    let catalog = ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch: 1,
            generation: 1,
            entries,
        },
        identities,
    };
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(1),
        catalog,
    };
    let encoded = encode_shell_file_catalog(object_header(ShellFileKind::Catalog), &value).unwrap();
    assert!(encoded.len() <= SHELL_FILE_MAX_OBJECT_BYTES);
    assert_eq!(decode_shell_file_catalog(&encoded).unwrap(), value);
    // 32-byte record header + 32-byte prefix + 4096 entry rows at 656 bytes
    // each = 32 + 32 + 2,686,976 = 2,687,040 bytes, comfortably under the
    // 4,194,304-byte (4 MiB) object cap.
    assert_eq!(encoded.len(), 2_687_040);
}

// ---------------------------------------------------------------------
// CatalogActivationOutcome / CatalogActivate
// ---------------------------------------------------------------------

#[test]
fn catalog_activation_outcome_round_trips() {
    let (tx, value, _) = fx::catalog_activation_outcome();
    let tx_record = ShellFileCatalogActionRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellCatalogActionRecord::ActivationOutcome(value),
    };
    let encoded = encode_shell_file_catalog_action(
        event_header(ShellFileKind::CatalogActivationOutcome),
        &tx_record,
    )
    .unwrap();
    assert_eq!(
        decode_shell_file_catalog_action(&encoded, ShellFileKind::CatalogActivationOutcome)
            .unwrap(),
        tx_record
    );
}

#[test]
fn catalog_activation_outcome_refuses_reserved_nonzero() {
    let (tx, value, _) = fx::catalog_activation_outcome();
    let tx_record = ShellFileCatalogActionRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellCatalogActionRecord::ActivationOutcome(value),
    };
    let encoded = encode_shell_file_catalog_action(
        event_header(ShellFileKind::CatalogActivationOutcome),
        &tx_record,
    )
    .unwrap();
    let mut bad = encoded.clone();
    // The wrapped `ContentAction`'s trailing reserved u32 sits at body
    // offset 116, record offset 32 + 116 = 148.
    bad[148..152].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog_action(&bad, ShellFileKind::CatalogActivationOutcome)
            .unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn catalog_activate_round_trips() {
    let (tx, value, _) = fx::catalog_activate();
    let tx_record = ShellFileCatalogActionRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellCatalogActionRecord::Activate(value),
    };
    let encoded = encode_shell_file_catalog_action(
        candidate_header(ShellFileKind::CatalogActivate),
        &tx_record,
    )
    .unwrap();
    assert_eq!(
        decode_shell_file_catalog_action(&encoded, ShellFileKind::CatalogActivate).unwrap(),
        tx_record
    );
}

#[test]
fn catalog_activate_refuses_an_invalid_action_id() {
    let (tx, mut value, _) = fx::catalog_activate();
    value.action.action_id = 4097; // over the persistent-catalog bound
    let tx_record = ShellFileCatalogActionRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellCatalogActionRecord::Activate(value),
    };
    assert_eq!(
        encode_shell_file_catalog_action(
            candidate_header(ShellFileKind::CatalogActivate),
            &tx_record
        )
        .unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("persistent catalog activation"))
    );
}

#[test]
fn catalog_action_wrong_kind_is_refused() {
    let (tx, value, _) = fx::catalog_activate();
    let tx_record = ShellFileCatalogActionRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellCatalogActionRecord::Activate(value),
    };
    let encoded = encode_shell_file_catalog_action(
        candidate_header(ShellFileKind::CatalogActivate),
        &tx_record,
    )
    .unwrap();
    assert_eq!(
        decode_shell_file_catalog_action(&encoded, ShellFileKind::CatalogActivationOutcome)
            .unwrap_err(),
        ShellFileCodecError::Class.into()
    );
}

// ---------------------------------------------------------------------
// CatalogCandidate: whole value, no transfer shape.
// ---------------------------------------------------------------------

#[test]
fn catalog_candidate_round_trips() {
    let (tx, candidate, ..) = fx::catalog_candidate();
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_catalog_candidate(
        candidate_header(ShellFileKind::CatalogCandidate),
        &value,
    )
    .unwrap();
    assert_eq!(
        decode_shell_file_catalog_candidate(&encoded).unwrap(),
        value
    );
}

#[test]
fn catalog_candidate_refuses_zero_transaction() {
    let (_, candidate, ..) = fx::catalog_candidate();
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::INVALID,
        candidate,
    };
    assert_eq!(
        encode_shell_file_catalog_candidate(
            candidate_header(ShellFileKind::CatalogCandidate),
            &value
        )
        .unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn catalog_candidate_refuses_reserved_nonzero() {
    let (tx, candidate, ..) = fx::catalog_candidate();
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_catalog_candidate(
        candidate_header(ShellFileKind::CatalogCandidate),
        &value,
    )
    .unwrap();
    let mut bad = encoded.clone();
    // Prefix reserved u16 at body offset 86, record offset 32 + 86 = 118.
    bad[118..120].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog_candidate(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn catalog_candidate_refuses_count_above_maximum() {
    let (tx, candidate, ..) = fx::catalog_candidate();
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_catalog_candidate(
        candidate_header(ShellFileKind::CatalogCandidate),
        &value,
    )
    .unwrap();
    let mut bad = encoded.clone();
    // `surface_count` sits at body offset 80, record offset 32 + 80 = 112;
    // the base profile's maximum is 8.
    bad[112..114].copy_from_slice(&9u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog_candidate(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::CountTooLarge { count: 9, max: 8 })
    );
}

#[test]
fn catalog_candidate_refuses_trailing_bytes() {
    let (tx, candidate, ..) = fx::catalog_candidate();
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_catalog_candidate(
        candidate_header(ShellFileKind::CatalogCandidate),
        &value,
    )
    .unwrap();
    let mut trailing = encoded;
    trailing.push(0);
    let len = trailing.len() as u32;
    trailing[0..4].copy_from_slice(&len.to_le_bytes());
    assert_eq!(
        decode_shell_file_catalog_candidate(&trailing).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::TrailingBytes(1))
    );
}

#[test]
fn catalog_candidate_refuses_invalid_row_content() {
    let (tx, mut candidate, ..) = fx::catalog_candidate();
    // The persistent catalog restricts every surface to role=1 (panel).
    // role=2 (popout) is a valid base-content shape on its own -- give it a
    // proper popout parent index and anchor so only the catalog-specific
    // restriction, not the generic content validator, is what rejects it
    // (every part is validated on encode).
    candidate.candidate.surfaces[0].role = 2;
    candidate.candidate.surfaces[0].parent_surface_index = 0;
    candidate.candidate.surfaces[0].anchor_parent_rect = ContentPixelRect {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    };
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    assert_eq!(
        encode_shell_file_catalog_candidate(
            candidate_header(ShellFileKind::CatalogCandidate),
            &value
        )
        .unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("persistent catalog rows"))
    );
}

#[test]
fn catalog_candidate_decode_refuses_bytes_over_the_max_cap() {
    let bytes = vec![0u8; SHELL_FILE_MAX_CANDIDATE_BYTES + 1];
    assert_eq!(
        decode_shell_file_catalog_candidate(&bytes).unwrap_err(),
        ShellFileCodecError::Length.into()
    );
}

#[test]
fn maximal_catalog_candidate_fits_within_the_cap() {
    // The base profile's maximum shape: 8 surfaces, 32 placements, 64
    // targets (crates/sophia-protocol/src/shell/content/validation.rs
    // `counts()`), each surface a panel (role=1, as the persistent-catalog
    // validators require).
    let surfaces: Vec<_> = (0..8)
        .map(|i| ContentSurface {
            allocation: ContentAllocationId {
                id: i + 1,
                generation: 1,
            },
            scale_generation: 1,
            role: 1,
            edge: 1,
            margins: ContentMargins::default(),
            reservation_extent: 0,
            parent_surface_index: u16::MAX,
            anchor_parent_rect: ContentPixelRect::default(),
        })
        .collect();
    let placements: Vec<_> = (0..32)
        .map(|i| ContentPlacement {
            resource: ContentResourceId {
                id: i + 1,
                generation: 1,
            },
            surface_index: (i % 8) as u16,
            destination_x_px: 0,
            destination_y_px: 0,
        })
        .collect();
    let targets: Vec<_> = (0..64)
        .map(|i| ContentTarget {
            surface_index: (i % 8) as u16,
            action_kind: 3,
            target_id: i + 1,
            target_generation: 1,
            action_id: (i % 4096) + 1,
            bounds_px: ContentPixelRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        })
        .collect();
    let candidate = CatalogContentCandidate {
        candidate: ContentCandidate {
            grant: ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            },
            candidate_generation: 1,
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surfaces,
            placements,
            targets,
        },
        catalog_generation: 1,
    };
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::from_raw(1),
        candidate,
    };
    let encoded = encode_shell_file_catalog_candidate(
        candidate_header(ShellFileKind::CatalogCandidate),
        &value,
    )
    .unwrap();
    assert!(encoded.len() <= SHELL_FILE_MAX_CANDIDATE_BYTES);
    assert_eq!(
        decode_shell_file_catalog_candidate(&encoded).unwrap(),
        value
    );
    // 32-byte record header + 88-byte prefix + 8 surface rows (64 B each)
    // + 32 placement rows (32 B each) + 64 target rows (48 B each)
    // = 32 + 88 + 512 + 1024 + 3072 = 4728 bytes, well under the 8192 cap.
    assert_eq!(encoded.len(), 4728);
}
