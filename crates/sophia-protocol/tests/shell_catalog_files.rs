//! Persistent catalog, catalog action and application catalog invariants of
//! `shell_catalog_actions.rs`, `shell_catalog_transaction.rs` and
//! `shell_launcher.rs`, on the SDK's neutral value codec, validators and
//! catalog file records, without the socket codecs. Frame prefixes,
//! family isolation between socket message kinds, identity-phase framing and
//! the exact legacy error spellings retired with the socket tests (t269).
//! Application-launcher file records are exercised in shell_launcher.rs.
#[path = "support/catalog_action_fixtures.rs"]
mod fixtures;
#[allow(dead_code)] // This test uses the catalog; shell_launcher uses the candidate.
#[path = "support/launcher_fixture.rs"]
mod launcher;

use std::collections::BTreeMap;

use fixtures::{action, records};
use sophia_protocol::shell::encoding::catalog_actions::{
    decode_shell_catalog_action_value, encode_shell_catalog_action_value,
    shell_catalog_action_value_kind,
};
use sophia_protocol::shell::encoding::content::encode_shell_content_value;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;

fn header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 2,
        submission_id: if shell_file_class(kind) == ShellFileClass::Candidate {
            1
        } else {
            0
        },
        sequence: if shell_file_class(kind) == ShellFileClass::Event {
            1
        } else {
            0
        },
    }
}

/// `persistent_catalog_roundtrip_and_all_truncations`: every record kind
/// round-trips as a value and refuses every truncation; the action records
/// the file wire carries refuse a zero transaction.
#[test]
fn catalog_action_values_round_trip_and_refuse_every_truncation() {
    let mut carried = 0;
    for record in records() {
        let kind = shell_catalog_action_value_kind(&record);
        let bytes = encode_shell_catalog_action_value(&record).unwrap();
        assert_eq!(
            decode_shell_catalog_action_value(kind, &bytes).unwrap(),
            record
        );
        for end in 0..bytes.len() {
            assert!(decode_shell_catalog_action_value(kind, &bytes[..end]).is_err());
        }
        let Some(file_kind) = shell_file_catalog_action_kind(&record) else {
            continue;
        };
        carried += 1;
        let tx_record = ShellFileCatalogActionRecord {
            transaction: TransactionId::from_raw(17),
            record: record.clone(),
        };
        let bytes = encode_shell_file_catalog_action(header(file_kind), &tx_record).unwrap();
        assert_eq!(
            decode_shell_file_catalog_action(&bytes, file_kind).unwrap(),
            tx_record
        );
        assert_eq!(
            encode_shell_file_catalog_action(
                header(file_kind),
                &ShellFileCatalogActionRecord {
                    transaction: TransactionId::from_raw(0),
                    record,
                }
            ),
            Err(ShellFilePayloadError::Identity)
        );
    }
    // Control: both file-carried action kinds (Activate, ActivationOutcome).
    assert_eq!(carried, 2);
}

/// `stable_identity_is_not_a_label_and_is_bounded`, as a value and inside
/// the persistent catalog the file wire carries identities in.
#[test]
fn stable_identity_is_not_a_label_and_is_bounded_in_values_and_catalogs() {
    let catalog = |identity: &str| ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch: 2,
            generation: 1,
            entries: vec![ShellApplicationDescriptor {
                slot: 1,
                available: true,
                label: "Terminal".to_owned(),
                keywords: String::new(),
            }],
        },
        identities: BTreeMap::from([(1, identity.to_owned())]),
    };
    let oversized = format!("registered:{}", "x".repeat(256));
    for identity in [
        "",
        "terminal",
        "registered:",
        "desktop:",
        "registered:a\n",
        oversized.as_str(),
    ] {
        let record = ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
            connection_epoch: 2,
            catalog_generation: 1,
            slot: 1,
            identity: identity.into(),
        });
        assert!(
            encode_shell_catalog_action_value(&record).is_err(),
            "{identity:?}"
        );
        assert!(
            validate_shell_persistent_catalog(&catalog(identity)).is_err(),
            "{identity:?}"
        );
    }
    assert!(validate_shell_persistent_catalog(&catalog("registered:terminal")).is_ok());
}

/// `cancellation_and_wrong_action_family_never_become_catalog_requests`.
#[test]
fn cancellation_and_wrong_action_family_never_become_catalog_values() {
    for (kind, slot, generation) in [(2, 9, 11), (1, 4097, 11), (1, 9, 0)] {
        let mut action = action();
        action.kind = kind;
        action.action_id = slot;
        assert!(
            encode_shell_catalog_action_value(&ShellCatalogActionRecord::Activate(
                CatalogActivation {
                    action,
                    catalog_generation: generation
                }
            ))
            .is_err()
        );
    }
    let ShellCatalogActionRecord::CandidateChunk(mut chunk) = records().remove(2) else {
        unreachable!()
    };
    assert!(
        encode_shell_content_value(&ShellContentRecord::CandidateChunk(chunk.clone())).is_err()
    );
    for kind in [1, 2, 4] {
        chunk.targets[0].action_kind = kind;
        assert!(
            encode_shell_catalog_action_value(&ShellCatalogActionRecord::CandidateChunk(
                chunk.clone()
            ))
            .is_err()
        );
    }
}

/// `a_real_entry_with_no_identity_reports_the_exact_ipc_error` and
/// `an_empty_catalog_round_trips_through_the_persistent_decoder`. A plain
/// catalog is a valid file `Catalog` for the launcher's view; the dock's
/// identity bijection is the persistent validator's rule.
#[test]
fn persistent_catalog_needs_every_identity_and_the_empty_catalog_round_trips() {
    let entry = ShellApplicationDescriptor {
        slot: 1,
        available: true,
        label: "Terminal".to_owned(),
        keywords: String::new(),
    };
    let unnamed = ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch: 1,
            generation: 2,
            entries: vec![entry],
        },
        identities: BTreeMap::new(),
    };
    assert!(validate_shell_persistent_catalog(&unnamed).is_err());

    let empty = ShellFileCatalog {
        transaction: TransactionId::from_raw(6),
        catalog: ShellPersistentCatalog {
            catalog: ShellApplicationCatalog {
                connection_epoch: 1,
                generation: 2,
                entries: Vec::new(),
            },
            identities: BTreeMap::new(),
        },
    };
    assert!(validate_shell_persistent_catalog(&empty.catalog).is_ok());
    let bytes = encode_shell_file_catalog(header(ShellFileKind::Catalog), &empty).unwrap();
    assert_eq!(decode_shell_file_catalog(&bytes).unwrap(), empty);
}

/// `bounded_catalog_and_launch_records_round_trip`, the catalog half, and
/// `malformed_catalog_and_candidate_cannot_cross_boundary`, the catalog
/// half: a maximal plain catalog is a file `Catalog`, and duplicate slots
/// and bidi controls are refused.
#[test]
fn application_catalog_is_bounded_and_refuses_duplicate_slots_and_bidi_labels() {
    let maximal = ShellFileCatalog {
        transaction: TransactionId::from_raw(1),
        catalog: ShellPersistentCatalog {
            catalog: launcher::catalog(4096),
            identities: BTreeMap::new(),
        },
    };
    let bytes = encode_shell_file_catalog(header(ShellFileKind::Catalog), &maximal).unwrap();
    assert_eq!(decode_shell_file_catalog(&bytes).unwrap(), maximal);
    assert!(validate_shell_application_catalog(&launcher::catalog(4097)).is_err());

    let mut catalog = launcher::catalog(3);
    assert!(validate_shell_application_catalog(&catalog).is_ok());
    catalog.entries[1].slot = 1;
    assert!(validate_shell_application_catalog(&catalog).is_err());
    catalog.entries[1].slot = 2;
    catalog.entries[1].label = "fake\u{202e}label".into();
    assert!(validate_shell_application_catalog(&catalog).is_err());
}
