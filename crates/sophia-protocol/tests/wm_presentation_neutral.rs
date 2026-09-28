//! Presentation records, shape and action semantics of `wm_presentation.rs`
//! without the socket codecs: the shared record sections, the WM file cycle
//! and the neutral validators. The legacy chunk ordinals, 65520-byte chunk
//! split and v1 frames stay in `wm_presentation.rs` and retire with the
//! socket wire (t269). Receipts are covered by `wm_file_controls.rs`.
use sophia_protocol::wm_files::*;
use sophia_protocol::*;

fn rect(x: i32, width: i32) -> Rect {
    Rect {
        x,
        y: 0,
        width,
        height: 100,
    }
}

fn presentation() -> PolicyPresentation {
    let output = OutputId::from_raw(1);
    PolicyPresentation {
        generation: 1,
        keyboard_output: Some(output),
        outputs: vec![PolicyPresentationOutput {
            output,
            generation: 1,
            coverage: rect(0, 400),
            mode: PolicyPresentationMode::ReplaceApplications,
        }],
        instances: vec![
            PolicySurfaceInstance {
                id: 2,
                generation: 1,
                output,
                source: SurfaceId::new(1, 1),
                destination: rect(0, 100),
                clip: rect(0, 100),
                opacity_millis: 1000,
                z_index: 1,
                action: Some(WmActionId::from_raw(5)),
            },
            PolicySurfaceInstance {
                id: 3,
                generation: 1,
                output,
                source: SurfaceId::new(1, 1),
                destination: rect(150, 100),
                clip: rect(150, 100),
                opacity_millis: 500,
                z_index: 2,
                action: None,
            },
        ],
        regions: vec![PolicyPresentationRegion {
            id: 1,
            generation: 1,
            output,
            geometry: rect(0, 400),
            clip: rect(0, 400),
            z_index: 0,
            role: PolicyPresentationRegionRole::Backdrop,
            action: None,
        }],
        bindings: vec![PolicyPresentationBinding {
            action: WmActionId::from_raw(5),
            keycode: 28,
            modifiers: WmModifierMask { bits: 0 },
        }],
    }
}

fn refs(sections: &[PolicyRecordSection]) -> Vec<PolicyRecordSectionRef<'_>> {
    sections.iter().map(PolicyRecordSection::as_ref).collect()
}

fn decode(
    sections: &[PolicyRecordSection],
) -> Result<Option<PolicyPresentation>, BinaryCodecError> {
    decode_policy_presentation_records(&refs(sections))
}

/// `repeated_source_roundtrips_with_independent_instance_identity_and_geometry`.
#[test]
fn repeated_source_records_round_trip_with_independent_instance_identity_and_geometry() {
    let p = presentation();
    let sections = encode_policy_presentation_records(Some(&p), 4).unwrap();
    assert_eq!(decode(&sections).unwrap(), Some(p));
    assert_eq!(
        encode_policy_presentation_records(None, 4).unwrap(),
        Vec::new()
    );
    assert_eq!(decode(&[]).unwrap(), None);
}

/// `reserved_bytes_missing_records_and_truncation_fail_closed`, on sections.
#[test]
fn reserved_bytes_missing_sections_and_truncation_fail_closed() {
    let sections = encode_policy_presentation_records(Some(&presentation()), 4).unwrap();
    assert_eq!(
        sections.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [
            PROJECTION_PRESENTATION_RECORD_KIND,
            PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND,
            PROJECTION_SURFACE_INSTANCE_RECORD_KIND,
            PROJECTION_PRESENTATION_REGION_RECORD_KIND,
            PROJECTION_PRESENTATION_BINDING_RECORD_KIND,
        ]
    );
    for (index, reserved) in [(0, 28), (1, 34), (1, 36), (2, 68), (3, 60)] {
        let mut bad = sections.clone();
        bad[index].bytes[reserved] = 1;
        assert!(decode(&bad).is_err(), "reserved {index}/{reserved}");
    }
    for index in 0..sections.len() {
        let mut bad = sections.clone();
        bad[index].bytes.pop();
        assert!(decode(&bad).is_err());
        let mut bad = sections.clone();
        bad.remove(index);
        assert!(decode(&bad).is_err());
        let mut bad = sections.clone();
        bad[index].count = u32::MAX;
        assert!(decode(&bad).is_err());
    }
}

/// `shape_rejects_duplicate_targets_order_and_unbounded_or_invisible_modal_regions`.
#[test]
fn shape_rejects_duplicate_targets_order_and_unbounded_or_invisible_modal_records() {
    for case in 0..10 {
        let mut p = presentation();
        match case {
            0 => p.instances[1].id = p.regions[0].id,
            1 => p.instances[1].z_index = p.instances[0].z_index,
            2 => p.instances[0].clip.x = 450,
            3 => p.instances[0].destination.x = i32::MAX,
            4 => p.instances[0].opacity_millis = 0,
            5 => p.regions.clear(),
            6 => p.outputs[0].mode = PolicyPresentationMode::Overlay,
            7 => p.keyboard_output = None,
            8 => p.bindings[0].modifiers.bits = WmModifierMask::CONTROL | WmModifierMask::ALT,
            _ => p.instances[0].source = SurfaceId::new(1, 0),
        }
        if case == 8 {
            p.bindings[0].keycode = 14;
        }
        assert!(
            encode_policy_presentation_records(Some(&p), 4).is_err(),
            "case {case}"
        );
    }
}

/// `chunk_splitting_preserves_counts_and_rejects_reordered_or_duplicated_headers`;
/// records carry the full instance count in one section.
#[test]
fn full_instance_records_round_trip_and_refuse_reordered_or_duplicated_headers() {
    let mut p = presentation();
    p.instances.clear();
    for index in 0..POLICY_MAX_SURFACE_INSTANCES {
        p.instances.push(PolicySurfaceInstance {
            id: index as u64 + 2,
            generation: 1,
            output: p.outputs[0].output,
            source: SurfaceId::new(1, 1),
            destination: rect(0, 100),
            clip: rect(0, 100),
            opacity_millis: 1000,
            z_index: index as u16 + 1,
            action: None,
        });
    }
    let sections = encode_policy_presentation_records(Some(&p), 4).unwrap();
    let instances = sections
        .iter()
        .find(|s| s.kind == PROJECTION_SURFACE_INSTANCE_RECORD_KIND)
        .unwrap();
    assert_eq!(instances.count as usize, POLICY_MAX_SURFACE_INSTANCES);
    assert_eq!(decode(&sections).unwrap(), Some(p));
    let mut bad = sections.clone();
    bad.insert(1, bad[0].clone());
    assert!(decode(&bad).is_err());
    let mut bad = sections.clone();
    bad.swap(0, 1);
    assert!(decode(&bad).is_err());
}

#[test]
fn action_catalog_excludes_unknown_actions_and_session_operations() {
    let p = presentation();
    assert!(validate_policy_presentation_actions(&p, &[]).is_err());
    let mut actions = vec![PolicyActionRegistration {
        action: WmActionId::from_raw(5),
        name: "opaque-policy-action".into(),
        session_operation_slot: Some(1),
    }];
    assert!(validate_policy_presentation_actions(&p, &actions).is_err());
    actions[0].session_operation_slot = None;
    assert!(validate_policy_presentation_actions(&p, &actions).is_ok());
    let mut p = p;
    p.regions[0].action = Some(WmActionId::from_raw(6));
    assert!(validate_policy_presentation_actions(&p, &actions).is_err());
}

/// `reduced_action_and_receipt_roundtrip_only_with_complete_identities_and_capabilities`,
/// the action half on the file cycle. The file path also requires the actions
/// capability for this cause (`policy_scalars_neutral.rs`
/// `cause_capabilities_are_the_file_path_map`); the legacy decoder did not.
#[test]
fn presentation_actions_need_complete_identities_and_every_capability() {
    let caps = SOPHIA_WM_CAPABILITY_ACTIONS
        | SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
        | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
    let header = WmFileHeader {
        kind: WmFileKind::Cycle,
        connection_epoch: 7,
        submission_id: 0,
        sequence: if wm_file_class(WmFileKind::Cycle) == WmFileClass::Event {
            203
        } else {
            0
        },
    };
    let cycle = |request: PolicyProjectionRequest| WmFileCycle {
        snapshot_transaction: TransactionId::from_raw(12),
        request_transaction: TransactionId::from_raw(13),
        request,
    };
    for target in [0, 3] {
        let request = PolicyProjectionRequest {
            connection_epoch: 7,
            request_id: 9,
            scene_generation: 11,
            policy_generation: 2,
            affected_outputs: vec![OutputId::from_raw(1)],
            cause: PolicyRequestCause::PresentationAction {
                activation_serial: 12,
                action: WmActionId::from_raw(5),
                identity: PolicyPresentationIdentity {
                    publication_generation: 4,
                    output: OutputId::from_raw(1),
                    output_generation: 6,
                    presentation_epoch: 8,
                    target_id: target,
                    target_generation: target,
                },
            },
        };
        let bytes = encode_wm_file_cycle(header, &cycle(request.clone()), caps).unwrap();
        assert_eq!(decode_wm_file_cycle(&bytes, caps).unwrap().request, request);
        for partial in [
            0,
            SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES,
            SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
            SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        ] {
            assert!(decode_wm_file_cycle(&bytes, partial).is_err());
        }
        for case in 0..7 {
            let mut bad = request.clone();
            let PolicyRequestCause::PresentationAction {
                activation_serial,
                identity,
                ..
            } = &mut bad.cause
            else {
                unreachable!()
            };
            match case {
                0 => identity.publication_generation = 0,
                1 => identity.presentation_epoch = 0,
                2 => identity.output_generation = 0,
                3 => identity.output = OutputId::from_raw(2),
                4 => {
                    identity.target_id = 0;
                    identity.target_generation = 1;
                }
                5 => {
                    identity.target_id = 1;
                    identity.target_generation = 0;
                }
                _ => *activation_serial = 0,
            }
            assert!(
                validate_policy_projection_request(&bad).is_err(),
                "case {case}"
            );
            assert!(
                encode_wm_file_cycle(header, &cycle(bad), caps).is_err(),
                "case {case}"
            );
        }
    }
}
