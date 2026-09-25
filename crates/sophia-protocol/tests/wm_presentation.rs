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

#[test]
fn repeated_source_roundtrips_with_independent_instance_identity_and_geometry() {
    let p = presentation();
    let chunks = encode_wm_presentation(Some(&p), 4, 2).unwrap();
    assert_eq!(chunks[0].ordinal, 2);
    assert_eq!(decode_wm_presentation(&chunks).unwrap(), Some(p));
    assert_eq!(encode_wm_presentation(None, 4, 2).unwrap(), Vec::new());
    assert_eq!(decode_wm_presentation(&[]).unwrap(), None);
}

#[test]
fn reserved_bytes_missing_records_and_truncation_fail_closed() {
    let chunks = encode_wm_presentation(Some(&presentation()), 4, 0).unwrap();
    for (index, reserved) in [(0, 28), (1, 34), (1, 36), (2, 68), (3, 60)] {
        let mut bad = chunks.clone();
        bad[index].data[reserved] = 1;
        assert!(
            decode_wm_presentation(&bad).is_err(),
            "reserved {index}/{reserved}"
        );
    }
    for index in 0..chunks.len() {
        let mut bad = chunks.clone();
        bad[index].data.pop();
        assert!(decode_wm_presentation(&bad).is_err());
        let mut bad = chunks.clone();
        bad.remove(index);
        assert!(decode_wm_presentation(&bad).is_err());
        let mut bad = chunks.clone();
        bad[index].item_count = u32::MAX;
        assert!(decode_wm_presentation(&bad).is_err());
    }
}

#[test]
fn shape_rejects_duplicate_targets_order_and_unbounded_or_invisible_modal_regions() {
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
            encode_wm_presentation(Some(&p), 4, 0).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn chunk_splitting_preserves_counts_and_rejects_reordered_or_duplicated_headers() {
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
    let chunks = encode_wm_presentation(Some(&p), 4, 0).unwrap();
    assert_eq!(
        chunks
            .iter()
            .filter(|c| c.record_kind == PROJECTION_SURFACE_INSTANCE_RECORD_KIND)
            .count(),
        2
    );
    assert_eq!(decode_wm_presentation(&chunks).unwrap(), Some(p));
    let mut bad = chunks.clone();
    bad.insert(1, bad[0].clone());
    assert!(decode_wm_presentation(&bad).is_err());
    let mut bad = chunks.clone();
    bad.swap(0, 1);
    assert!(decode_wm_presentation(&bad).is_err());
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

#[test]
fn reduced_action_and_receipt_roundtrip_only_with_complete_identities_and_capabilities() {
    let caps = SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
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
        let wire = encode_wm_presentation_action_request(&request).unwrap();
        let frame =
            encode_wm_v1_presentation_action_request_frame(TransactionId::from_raw(13), &wire)
                .unwrap();
        let (_, decoded) = decode_wm_v1_presentation_action_request_frame(&frame).unwrap();
        assert_eq!(
            decode_wm_presentation_action_request(&decoded, caps).unwrap(),
            request
        );
        assert!(encode_wm_v1_policy_projection_request(&request).is_err());
        for partial in [
            0,
            SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES,
            SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        ] {
            assert!(decode_wm_presentation_action_request(&wire, partial).is_err());
        }
        for case in 0..7 {
            let mut bad = wire.clone();
            match case {
                0 => bad.publication_generation = 0,
                1 => bad.presentation_epoch = 0,
                2 => bad.output_generation = 0,
                3 => bad.output = 2,
                4 => {
                    bad.target_id = 0;
                    bad.target_generation = 1;
                }
                5 => {
                    bad.target_id = 1;
                    bad.target_generation = 0;
                }
                _ => bad.activation_serial = 0,
            }
            assert!(
                decode_wm_presentation_action_request(&bad, caps).is_err(),
                "case {case}"
            );
        }
    }
    for outcome in [
        PolicyPresentationOutcome::Presented,
        PolicyPresentationOutcome::Revoked,
        PolicyPresentationOutcome::Withdrawn,
    ] {
        let receipt = PolicyPresentationReceipt {
            connection_epoch: 1,
            publication_generation: 2,
            output: OutputId::from_raw(3),
            output_generation: 4,
            presentation_epoch: 5,
            outcome,
        };
        let wire = encode_wm_presentation_receipt(receipt).unwrap();
        let frame =
            encode_wm_v1_presentation_outcome_frame(TransactionId::from_raw(6), &wire).unwrap();
        let (_, decoded) = decode_wm_v1_presentation_outcome_frame(&frame).unwrap();
        assert_eq!(
            decode_wm_presentation_receipt(&decoded, caps).unwrap(),
            receipt
        );
        assert!(decode_wm_presentation_receipt(&wire, 0).is_err());
        let mut bad = wire.clone();
        bad.outcome = 4;
        assert!(decode_wm_presentation_receipt(&bad, caps).is_err());
        bad = wire;
        bad.presentation_epoch = 0;
        assert!(decode_wm_presentation_receipt(&bad, caps).is_err());
    }
}
