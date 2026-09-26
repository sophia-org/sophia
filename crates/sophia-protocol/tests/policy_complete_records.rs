use sophia_protocol::*;
#[path = "support/policy_record_fixture.rs"]
mod fixture;
fn refs(sections: &[PolicyRecordSection]) -> Vec<PolicyRecordSectionRef<'_>> {
    sections.iter().map(PolicyRecordSection::as_ref).collect()
}
fn snapshot_meta() -> PolicySnapshotMetadata {
    PolicySnapshotMetadata {
        connection_epoch: 2,
        scene_generation: 7,
        active_output: OutputId::from_raw(1),
    }
}
fn projection_meta() -> PolicyProjectionMetadata {
    let p = fixture::proposal();
    PolicyProjectionMetadata {
        transaction: p.transaction,
        connection_epoch: p.connection_epoch,
        request_id: p.request_id,
        base_generation: p.base_generation,
        active_output: p.active_output,
    }
}
fn snapshot() -> Vec<PolicyRecordSection> {
    encode_policy_snapshot_records(
        2,
        &fixture::scene(),
        &fixture::actions(),
        &fixture::classifications(),
        &fixture::origins(),
        u64::MAX,
    )
    .unwrap()
}
fn projection() -> Vec<PolicyRecordSection> {
    encode_policy_projection_records(&fixture::proposal()).unwrap()
}
#[test]
fn complete_snapshot_and_all_projection_extensions_roundtrip() {
    let decoded = decode_policy_snapshot_records(snapshot_meta(), &refs(&snapshot())).unwrap();
    assert_eq!(decoded.scene, fixture::scene());
    assert_eq!(decoded.actions, fixture::actions());
    assert_eq!(decoded.classifications, fixture::classifications());
    assert_eq!(decoded.launch_origins, fixture::origins());
    assert_eq!(
        decode_policy_projection_records(projection_meta(), &refs(&projection())).unwrap(),
        fixture::proposal()
    );
}
#[test]
fn current_ipc_bytes_equal_the_separately_built_pre_extraction_source() {
    assert_eq!(
        fixture::legacy_bytes(),
        include_bytes!("fixtures/policy-records-95b39662.bin").as_slice()
    );
}
#[test]
fn every_section_checks_length_count_and_unknown_kind_before_decode() {
    for (context, sections) in [
        (PolicyRecordContext::Snapshot, snapshot()),
        (PolicyRecordContext::Projection, projection()),
    ] {
        for index in 0..sections.len() {
            let mut malformed = sections.clone();
            malformed[index].bytes.pop();
            assert!(validate_policy_record_sections(context, &refs(&malformed)).is_err());
            let mut malformed = sections.clone();
            malformed[index].count = u32::MAX;
            assert!(coalesce_policy_record_sections(context, &refs(&malformed)).is_err());
            let mut malformed = sections.clone();
            malformed[index].kind = 0xabcd;
            assert!(coalesce_policy_record_sections(context, &refs(&malformed)).is_err());
        }
    }
}
#[test]
fn coalescing_checks_aggregate_not_only_individual_chunks() {
    let sections = snapshot();
    let s = &sections[0];
    let repeated = vec![s.as_ref(); 17];
    assert!(matches!(
        coalesce_policy_record_sections(PolicyRecordContext::Snapshot, &repeated),
        Err(IpcCodecError::CountTooLarge { count: 17, max: 16 })
    ));
    assert!(decode_policy_snapshot_records(snapshot_meta(), &repeated).is_err());
    let joined =
        coalesce_policy_record_sections(PolicyRecordContext::Snapshot, &[s.as_ref(), s.as_ref()])
            .unwrap();
    assert_eq!(joined[0].count, 2);
    assert_eq!(joined[0].bytes, [s.bytes.clone(), s.bytes.clone()].concat());
}

#[test]
fn maximum_u32_count_with_four_row_bytes_is_refused_before_row_allocation() {
    let bytes = [0_u8; 4];
    let sections = [PolicyRecordSectionRef {
        kind: SNAPSHOT_OUTPUT_RECORD_KIND,
        count: u32::MAX,
        bytes: &bytes,
    }];
    // Pin the neutral preflight error, not a downstream array decoder error.
    // Both coalescing and domain conversion must take this path before sizing
    // any row vector from the untrusted count.
    let expected = IpcCodecError::InvalidEnum {
        field: "policy_record_section",
        value: u32::from(SNAPSHOT_OUTPUT_RECORD_KIND),
    };
    assert_eq!(
        validate_policy_record_sections(PolicyRecordContext::Snapshot, &sections),
        Err(expected.clone())
    );
    assert_eq!(
        coalesce_policy_record_sections(PolicyRecordContext::Snapshot, &sections),
        Err(expected.clone())
    );
    assert_eq!(
        decode_policy_snapshot_records(snapshot_meta(), &sections),
        Err(expected)
    );
}
#[test]
fn cross_array_identity_and_count_failures_share_the_legacy_validators() {
    for kind in [
        SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
        SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND,
        SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND,
    ] {
        let mut s = snapshot();
        let row = s.iter_mut().find(|s| s.kind == kind).unwrap();
        row.bytes[0..4].copy_from_slice(&999_u32.to_le_bytes());
        assert!(
            decode_policy_snapshot_records(snapshot_meta(), &refs(&s)).is_err(),
            "kind {kind}"
        );
    }
    for (kind, offset) in [
        (PROJECTION_OUTPUT_RECORD_KIND, 8),
        (PROJECTION_TAB_MEMBER_RECORD_KIND, 8),
        (PROJECTION_TRANSLATION_MEMBER_RECORD_KIND, 8),
        (PROJECTION_LAUNCH_CONTEXT_RECORD_KIND, 8),
        (PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND, 16),
        (PROJECTION_PRESENTATION_RECORD_KIND, 20),
    ] {
        let mut s = projection();
        let row = s.iter_mut().find(|s| s.kind == kind).unwrap();
        row.bytes[offset..offset + 4].copy_from_slice(&999_u32.to_le_bytes());
        assert!(
            decode_policy_projection_records(projection_meta(), &refs(&s)).is_err(),
            "kind {kind}"
        );
    }
}
#[test]
fn presentation_order_is_checked_before_any_caller_coalesces() {
    let mut s = projection();
    let a = s
        .iter()
        .position(|s| s.kind == PROJECTION_PRESENTATION_RECORD_KIND)
        .unwrap();
    s.swap(a, a + 1);
    assert!(decode_policy_projection_records(projection_meta(), &refs(&s)).is_err());
}

#[test]
fn configuration_records_share_catalog_and_chrome_validation() {
    let configuration = PolicyConfiguration {
        connection_epoch: 2,
        generation: 3,
        actions: fixture::actions(),
        chrome: WmChromePolicy::default(),
    };
    let metadata = PolicyConfigurationMetadata {
        connection_epoch: configuration.connection_epoch,
        generation: configuration.generation,
        chrome: configuration.chrome,
    };
    let sections = encode_policy_configuration_records(&configuration).unwrap();
    assert_eq!(
        decode_policy_configuration_records(metadata, &refs(&sections)).unwrap(),
        configuration
    );
    let legacy = encode_wm_v1_policy_configuration(&configuration).unwrap();
    assert_eq!(legacy.actions, sections[0].bytes);
    assert_eq!(u32::from(legacy.action_count), sections[0].count);
    let mut duplicate = sections.clone();
    duplicate[0].count *= 2;
    duplicate[0].bytes.extend_from_slice(&sections[0].bytes);
    assert!(decode_policy_configuration_records(metadata, &refs(&duplicate)).is_err());
    let mut bad_style = metadata;
    bad_style.chrome.focus_ring.enabled = true;
    bad_style.chrome.focus_ring.width = 0;
    assert!(decode_policy_configuration_records(bad_style, &refs(&sections)).is_err());
    let mut wrong_kind = sections;
    wrong_kind[0].kind = SNAPSHOT_SURFACE_RECORD_KIND;
    assert!(decode_policy_configuration_records(metadata, &refs(&wrong_kind)).is_err());
}
