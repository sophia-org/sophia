use sophia_protocol::wm_files::*;
use sophia_protocol::*;

#[path = "support/policy_record_fixture.rs"]
mod fixture;

fn header(kind: WmFileKind) -> WmFileHeader {
    WmFileHeader {
        kind,
        connection_epoch: 2,
        submission_id: if wm_file_class(kind) == WmFileClass::Candidate {
            81
        } else {
            0
        },
        sequence: 0,
    }
}

fn snapshot() -> WmFileSnapshot {
    WmFileSnapshot {
        transaction: TransactionId::from_raw(9),
        snapshot: PolicyDecodedSnapshot {
            scene: fixture::scene(),
            actions: fixture::actions(),
            classifications: fixture::classifications(),
            launch_origins: fixture::origins(),
        },
    }
}

fn configuration() -> WmFileConfiguration {
    WmFileConfiguration {
        transaction: TransactionId::from_raw(23),
        configuration: PolicyConfiguration {
            connection_epoch: 2,
            generation: 3,
            actions: fixture::actions(),
            chrome: WmChromePolicy::default(),
        },
    }
}

#[test]
fn complete_arrays_preserve_all_neutral_semantics_and_domain_transactions() {
    let scene = snapshot();
    let bytes = encode_wm_file_snapshot(header(WmFileKind::Snapshot), &scene, u64::MAX).unwrap();
    assert_eq!(decode_wm_file_snapshot(&bytes, u64::MAX).unwrap(), scene);
    let proposal = fixture::proposal();
    let bytes =
        encode_wm_file_projection(header(WmFileKind::Projection), &proposal, u64::MAX).unwrap();
    assert_eq!(
        decode_wm_file_record(&bytes, WmFileClass::Candidate)
            .unwrap()
            .header
            .submission_id,
        81
    );
    assert_eq!(
        decode_wm_file_projection(&bytes, u64::MAX).unwrap(),
        proposal
    );
    assert_eq!(proposal.transaction.raw(), 11);
    let config = configuration();
    let bytes =
        encode_wm_file_configuration(header(WmFileKind::Configuration), &config, u64::MAX).unwrap();
    assert_eq!(
        decode_wm_file_configuration(&bytes, u64::MAX).unwrap(),
        config
    );
    // New file envelopes do not change the old scalar/chunk encodings.
    assert_eq!(
        fixture::legacy_bytes(),
        include_bytes!("fixtures/policy-records-95b39662.bin").as_slice()
    );
}

#[test]
fn prefix_offsets_match_the_published_little_endian_layout() {
    let s = encode_wm_file_snapshot(header(WmFileKind::Snapshot), &snapshot(), u64::MAX).unwrap();
    assert_eq!(
        &s[32..56],
        &[
            9, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(&s[58..64], &[0; 6]);
    let p = encode_wm_file_projection(
        header(WmFileKind::Projection),
        &fixture::proposal(),
        u64::MAX,
    )
    .unwrap();
    assert_eq!(
        &p[32..64],
        &[
            11, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
            0, 0, 0
        ]
    );
    assert_eq!(&p[66..72], &[0; 6]);
    let c = encode_wm_file_configuration(
        header(WmFileKind::Configuration),
        &configuration(),
        u64::MAX,
    )
    .unwrap();
    assert_eq!(
        &c[32..50],
        &[23, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0]
    );
    assert_eq!(&c[52..60], &[2, 0, 0, 0, 255, 183, 112, 0]);
    assert_eq!(&c[72..80], &[0; 8]);
}

#[test]
fn every_snapshot_extension_is_gated_by_the_selected_capabilities() {
    let bytes =
        encode_wm_file_snapshot(header(WmFileKind::Snapshot), &snapshot(), u64::MAX).unwrap();
    for capability in [
        SOPHIA_WM_CAPABILITY_ACTIONS,
        SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
        SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT,
        SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
        SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS,
    ] {
        assert_eq!(
            decode_wm_file_snapshot(&bytes, !capability),
            Err(WmFilePayloadError::Capabilities {
                missing: capability
            })
        );
    }
    let omitted = encode_wm_file_snapshot(header(WmFileKind::Snapshot), &snapshot(), 0).unwrap();
    let decoded = decode_wm_file_snapshot(&omitted, 0).unwrap();
    assert!(decoded.snapshot.actions.is_empty());
    assert!(decoded.snapshot.classifications.is_empty());
    assert!(decoded.snapshot.launch_origins.is_empty());
    assert!(decoded.snapshot.scene.session_operations.is_empty());
    assert!(
        decoded
            .snapshot
            .scene
            .outputs
            .iter()
            .all(|o| o.policy_key.is_none())
    );
}

#[test]
fn unnegotiated_projection_sections_refuse_before_domain_delivery() {
    let proposal = fixture::proposal();
    let bytes =
        encode_wm_file_projection(header(WmFileKind::Projection), &proposal, u64::MAX).unwrap();
    for capability in [
        SOPHIA_WM_CAPABILITY_INDICATORS,
        SOPHIA_WM_CAPABILITY_TAB_GROUPS,
        SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS,
        SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
        SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT,
        SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES,
        SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        SOPHIA_WM_CAPABILITY_ACTIONS,
    ] {
        assert_eq!(
            decode_wm_file_projection(&bytes, !capability),
            Err(WmFilePayloadError::Capabilities {
                missing: capability
            })
        );
        assert!(
            encode_wm_file_projection(header(WmFileKind::Projection), &proposal, !capability)
                .is_err()
        );
    }
    let mut action_without_bindings = proposal;
    action_without_bindings
        .presentation
        .as_mut()
        .unwrap()
        .bindings
        .clear();
    action_without_bindings
        .presentation
        .as_mut()
        .unwrap()
        .keyboard_output = None;
    let bytes = encode_wm_file_projection(
        header(WmFileKind::Projection),
        &action_without_bindings,
        u64::MAX,
    )
    .unwrap();
    assert!(decode_wm_file_projection(&bytes, !SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS).is_err());
}

#[test]
fn configuration_caps_catalog_and_chrome_share_validation() {
    let config = configuration();
    let bytes =
        encode_wm_file_configuration(header(WmFileKind::Configuration), &config, u64::MAX).unwrap();
    for capability in [
        SOPHIA_WM_CAPABILITY_CONFIGURATION,
        SOPHIA_WM_CAPABILITY_ACTIONS,
        SOPHIA_WM_CAPABILITY_CHROME,
    ] {
        assert_eq!(
            decode_wm_file_configuration(&bytes, !capability),
            Err(WmFilePayloadError::Capabilities {
                missing: capability
            })
        );
    }
    let mut bad = config.clone();
    bad.configuration
        .actions
        .push(bad.configuration.actions[0].clone());
    assert!(
        encode_wm_file_configuration(header(WmFileKind::Configuration), &bad, u64::MAX).is_err()
    );
    let mut bad = bytes.clone();
    bad[52..56].copy_from_slice(&0u32.to_le_bytes()); // enabled, zero width
    assert!(decode_wm_file_configuration(&bad, u64::MAX).is_err());
    for offset in [49, 59, 67, 71, 72, 79] {
        let mut bad = bytes.clone();
        bad[offset] = 0x80;
        assert!(
            decode_wm_file_configuration(&bad, u64::MAX).is_err(),
            "offset {offset}"
        );
    }
}

#[test]
fn complete_prefix_and_array_truncations_cannot_be_repaired_by_the_outer_length() {
    let bytes = encode_wm_file_projection(
        header(WmFileKind::Projection),
        &fixture::proposal(),
        u64::MAX,
    )
    .unwrap();
    for end in 32..bytes.len() {
        let mut truncated = bytes[..end].to_vec();
        truncated[..4].copy_from_slice(&u32::try_from(end).unwrap().to_le_bytes());
        assert!(
            decode_wm_file_projection(&truncated, u64::MAX).is_err(),
            "offset {end}"
        );
    }
    for offset in [32, 40, 48, 56, 66, 67, 68, 69, 70, 71] {
        let mut bad = bytes.clone();
        bad[offset] = if offset < 66 { 0 } else { 1 };
        assert!(decode_wm_file_projection(&bad, u64::MAX).is_err());
    }
}

#[test]
fn complete_snapshot_and_projection_require_an_output_section() {
    for (bytes, prefix, count_offset, kind) in [
        (
            encode_wm_file_snapshot(header(WmFileKind::Snapshot), &snapshot(), u64::MAX).unwrap(),
            WM_FILE_SNAPSHOT_PREFIX_BYTES,
            24,
            WmFileKind::Snapshot,
        ),
        (
            encode_wm_file_projection(
                header(WmFileKind::Projection),
                &fixture::proposal(),
                u64::MAX,
            )
            .unwrap(),
            WM_FILE_PROJECTION_PREFIX_BYTES,
            32,
            WmFileKind::Projection,
        ),
    ] {
        let record = decode_wm_file_record(&bytes, wm_file_class(kind)).unwrap();
        let mut body = record.body[..prefix].to_vec();
        body[count_offset..count_offset + 2].copy_from_slice(&0u16.to_le_bytes());
        let empty = encode_wm_file_record(record.header, &body).unwrap();
        let error = if kind == WmFileKind::Snapshot {
            decode_wm_file_snapshot(&empty, u64::MAX).unwrap_err()
        } else {
            decode_wm_file_projection(&empty, u64::MAX).unwrap_err()
        };
        assert_eq!(
            error,
            WmFilePayloadError::Envelope(WmFileCodecError::Sections)
        );
    }
    let mut bytes =
        encode_wm_file_snapshot(header(WmFileKind::Snapshot), &snapshot(), u64::MAX).unwrap();
    bytes[48..56].copy_from_slice(&99u64.to_le_bytes());
    assert_eq!(
        decode_wm_file_snapshot(&bytes, u64::MAX),
        Err(WmFilePayloadError::Identity)
    );
    let mut proposal = fixture::proposal();
    proposal.outputs.clear();
    assert!(
        encode_wm_file_projection(header(WmFileKind::Projection), &proposal, u64::MAX).is_err()
    );
}

#[test]
fn the_same_class_does_not_substitute_a_different_payload_kind_or_epoch() {
    let bytes = encode_wm_file_projection(
        header(WmFileKind::Projection),
        &fixture::proposal(),
        u64::MAX,
    )
    .unwrap();
    assert!(matches!(
        decode_wm_file_configuration(&bytes, u64::MAX),
        Err(WmFilePayloadError::Envelope(WmFileCodecError::Kind))
    ));
    let mut wrong_epoch = header(WmFileKind::Projection);
    wrong_epoch.connection_epoch += 1;
    assert_eq!(
        encode_wm_file_projection(wrong_epoch, &fixture::proposal(), u64::MAX),
        Err(WmFilePayloadError::Identity)
    );
    assert!(
        encode_wm_file_configuration(header(WmFileKind::Projection), &configuration(), u64::MAX)
            .is_err()
    );
}

#[test]
fn a_large_complete_array_has_no_legacy_chunk_boundaries() {
    let mut proposal = fixture::proposal();
    let presentation = proposal.presentation.as_mut().unwrap();
    let instance = presentation.instances[0];
    presentation.instances = (0..1024)
        .map(|i| PolicySurfaceInstance {
            id: i + 2,
            z_index: u16::try_from(i + 1).unwrap(),
            ..instance
        })
        .collect();
    let bytes =
        encode_wm_file_projection(header(WmFileKind::Projection), &proposal, u64::MAX).unwrap();
    assert!(bytes.len() > 65536);
    assert_eq!(
        decode_wm_file_projection(&bytes, u64::MAX).unwrap(),
        proposal
    );
    let body = decode_wm_file_record(&bytes, WmFileClass::Candidate)
        .unwrap()
        .body;
    let sections = decode_wm_file_sections(
        &body[40..],
        u16::from_le_bytes(body[32..34].try_into().unwrap()),
    )
    .unwrap();
    let instances = sections
        .iter()
        .filter(|s| s.kind == PROJECTION_SURFACE_INSTANCE_RECORD_KIND)
        .collect::<Vec<_>>();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].count, 1024);
}
