//! Regression coverage for `sophia_protocol::ipc::shell_catalog_transaction`:
//! `decode_shell_persistent_catalog`'s public error must stay
//! byte/value-identical across the move of its identity-bijection check into
//! `crate::shell::catalog_transaction::validate` (t252 B5).
use sophia_protocol::*;

fn bad() -> IpcCodecError {
    IpcCodecError::InvalidRecord("persistent catalog transaction")
}

/// A catalog with a real entry and no identity frames at all fails the
/// bijection check (no partial dock disclosure) with the same single error
/// the pre-move inline check returned, regardless of which of its combined
/// conditions actually tripped.
#[test]
fn a_real_entry_with_no_identity_reports_the_exact_ipc_error() {
    let tx = TransactionId::from_raw(5);
    let catalog = ShellApplicationCatalog {
        connection_epoch: 1,
        generation: 2,
        entries: vec![ShellApplicationDescriptor {
            slot: 1,
            available: true,
            label: "Terminal".to_owned(),
            keywords: String::new(),
        }],
    };
    let frames = encode_shell_application_catalog(tx, &catalog).expect("encode");
    assert_eq!(decode_shell_persistent_catalog(&frames), Err(bad()));
}

/// An empty catalog (no entries, no identities) is the trivial bijection and
/// still round-trips through the persistent-catalog decoder.
#[test]
fn an_empty_catalog_round_trips_through_the_persistent_decoder() {
    let tx = TransactionId::from_raw(6);
    let catalog = ShellApplicationCatalog {
        connection_epoch: 1,
        generation: 2,
        entries: Vec::new(),
    };
    let frames = encode_shell_application_catalog(tx, &catalog).expect("encode");
    let (decoded_tx, decoded) = decode_shell_persistent_catalog(&frames).expect("decode");
    assert_eq!(decoded_tx, tx);
    assert!(decoded.identities.is_empty());
    assert!(decoded.catalog.entries.is_empty());
}

/// A duplicated identity slot fails with the same single error, whichever
/// sub-condition the original inline check happened to trip on.
#[test]
fn a_duplicate_identity_slot_reports_the_exact_ipc_error() {
    let tx = TransactionId::from_raw(7);
    let catalog = ShellApplicationCatalog {
        connection_epoch: 1,
        generation: 2,
        entries: vec![
            ShellApplicationDescriptor {
                slot: 1,
                available: true,
                label: "A".to_owned(),
                keywords: String::new(),
            },
            ShellApplicationDescriptor {
                slot: 2,
                available: true,
                label: "B".to_owned(),
                keywords: String::new(),
            },
        ],
    };
    let mut frames = encode_shell_application_catalog(tx, &catalog).expect("encode");
    let identity = |slot, name: &str| {
        encode_shell_catalog_action_frame(
            tx,
            &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                connection_epoch: 1,
                catalog_generation: 2,
                slot,
                identity: name.to_owned(),
            }),
        )
        .expect("encode identity")
    };
    // Insert two identity frames naming the same slot, right after the
    // Begin frame, before the End frame.
    let end = frames.pop().expect("End frame");
    frames.push(identity(1, "registered:a"));
    frames.push(identity(1, "registered:a-again"));
    frames.push(end);
    assert_eq!(decode_shell_persistent_catalog(&frames), Err(bad()));
}
