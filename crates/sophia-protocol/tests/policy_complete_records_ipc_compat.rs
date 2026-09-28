//! The socket (`sophia_wm_v1`) parity of the shared WM records: the legacy
//! record golden, unchanged by the file envelopes, and the configuration
//! action rows the legacy encoder shares. IPC-only: it retires with the
//! socket wire (t269). The neutral records are in `policy_complete_records.rs`.
use sophia_protocol::wm_files::*;
use sophia_protocol::*;
#[path = "support/policy_record_fixture.rs"]
mod fixture;
#[path = "support/policy_record_ipc_fixture.rs"]
mod fixture_ipc;
#[test]
fn current_ipc_bytes_equal_the_separately_built_pre_extraction_source() {
    assert_eq!(
        fixture_ipc::legacy_bytes(),
        include_bytes!("fixtures/policy-records-95b39662.bin").as_slice()
    );
}
/// Moved from `wm_file_arrays.rs`, whose file-only binary must build without
/// the socket codecs: new file envelopes do not change the old scalar/chunk
/// encodings.
#[test]
fn file_envelopes_leave_the_legacy_record_bytes_unchanged() {
    let header = |kind| WmFileHeader {
        kind,
        connection_epoch: 2,
        submission_id: if wm_file_class(kind) == WmFileClass::Candidate {
            81
        } else {
            0
        },
        sequence: 0,
    };
    let snapshot = WmFileSnapshot {
        transaction: TransactionId::from_raw(9),
        snapshot: PolicyDecodedSnapshot {
            scene: fixture::scene(),
            actions: fixture::actions(),
            classifications: fixture::classifications(),
            launch_origins: fixture::origins(),
        },
    };
    let configuration = WmFileConfiguration {
        transaction: TransactionId::from_raw(23),
        configuration: PolicyConfiguration {
            connection_epoch: 2,
            generation: 3,
            actions: fixture::actions(),
            chrome: WmChromePolicy::default(),
        },
    };
    encode_wm_file_snapshot(header(WmFileKind::Snapshot), &snapshot, u64::MAX).unwrap();
    encode_wm_file_projection(
        header(WmFileKind::Projection),
        &fixture::proposal(),
        u64::MAX,
    )
    .unwrap();
    encode_wm_file_configuration(header(WmFileKind::Configuration), &configuration, u64::MAX)
        .unwrap();
    assert_eq!(
        fixture_ipc::legacy_bytes(),
        include_bytes!("fixtures/policy-records-95b39662.bin").as_slice()
    );
}
/// Moved from `configuration_records_share_catalog_and_chrome_validation`,
/// whose neutral assertions stay in `policy_complete_records.rs`.
#[test]
fn configuration_legacy_action_rows_equal_the_shared_records() {
    let configuration = PolicyConfiguration {
        connection_epoch: 2,
        generation: 3,
        actions: fixture::actions(),
        chrome: WmChromePolicy::default(),
    };
    let sections = encode_policy_configuration_records(&configuration).unwrap();
    let legacy = encode_wm_v1_policy_configuration(&configuration).unwrap();
    assert_eq!(legacy.actions, sections[0].bytes);
    assert_eq!(u32::from(legacy.action_count), sections[0].count);
}
