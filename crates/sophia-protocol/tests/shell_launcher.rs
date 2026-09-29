//! Application-launcher semantics on whole file records. The catalog's
//! Begin/Entry/End socket transfer retired with that wire.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
#[path = "support/descriptor_file.rs"]
mod file;
#[path = "support/launcher_fixture.rs"]
mod fixture;

fn catalog(value: ShellApplicationCatalog) -> Result<Vec<u8>, ShellFilePayloadError> {
    encode_shell_file_catalog(
        file::header(ShellFileKind::Catalog),
        &ShellFileCatalog {
            transaction: TransactionId::from_raw(11),
            catalog: ShellPersistentCatalog {
                catalog: value,
                identities: Default::default(),
            },
        },
    )
}

#[test]
fn bounded_catalog_and_launch_records_round_trip() {
    let value = fixture::catalog(4096);
    let bytes = catalog(value.clone()).unwrap();
    let decoded = decode_shell_file_catalog(&bytes).unwrap();
    assert_eq!(decoded.transaction, TransactionId::from_raw(11));
    assert_eq!(decoded.catalog.catalog, value);
    assert!(decoded.catalog.identities.is_empty());
    file::round_trip(ShellDescriptorRecord::LauncherCandidate(
        fixture::candidate(),
    ));
    for operation in [
        ShellLauncherOperation::Open,
        ShellLauncherOperation::Query,
        ShellLauncherOperation::Next,
        ShellLauncherOperation::Previous,
        ShellLauncherOperation::Dismiss,
    ] {
        file::round_trip(ShellDescriptorRecord::LauncherRequest(
            ShellLauncherRequest {
                connection_epoch: 5,
                catalog_generation: 7,
                request_generation: 8,
                output: OutputId::from_raw(1),
                output_generation: 1,
                presentation_epoch: 11,
                operation,
                query: "Éditeur".into(),
            },
        ));
    }
    let activation = ShellLauncherActivation {
        connection_epoch: 5,
        catalog_generation: 7,
        request_generation: 8,
        candidate_generation: 9,
        presentation_epoch: 11,
        activation: 12,
        slot: 1,
    };
    file::round_trip(ShellDescriptorRecord::LauncherActivation(activation));
    for consumed in [false, true] {
        file::round_trip(ShellDescriptorRecord::LauncherActivationAck(
            ShellLauncherActivationAck {
                activation,
                consumed,
            },
        ));
    }
    for status in [
        ShellLaunchStatus::Started,
        ShellLaunchStatus::Failed,
        ShellLaunchStatus::Rejected,
    ] {
        file::round_trip(ShellDescriptorRecord::LaunchOutcome(ShellLaunchOutcome {
            activation,
            status,
        }));
    }
}

#[test]
fn malformed_catalog_and_candidate_cannot_cross_boundary() {
    let mut value = fixture::catalog(3);
    value.entries[1].slot = 1;
    assert!(catalog(value.clone()).is_err());
    value.entries[1].slot = 2;
    value.entries[1].label = "fake\u{202e}label".into();
    assert!(catalog(value).is_err());
    let bytes = catalog(fixture::catalog(3)).unwrap();
    for end in 0..bytes.len() {
        assert!(decode_shell_file_catalog(&bytes[..end]).is_err());
    }
    let mut candidate = fixture::candidate();
    candidate.entries.push(1);
    assert!(file::encode(ShellDescriptorRecord::LauncherCandidate(candidate.clone())).is_err());
    candidate.entries.pop();
    candidate.selected = 4;
    assert!(file::encode(ShellDescriptorRecord::LauncherCandidate(candidate)).is_err());
}
