use sophia_protocol::*;

fn request() -> PolicyProjectionRequest {
    PolicyProjectionRequest {
        connection_epoch: 3,
        request_id: 7,
        scene_generation: 8,
        policy_generation: 2,
        cause: PolicyRequestCause::OutputAction {
            activation_serial: 9,
            action: WmActionId::from_raw(15),
            output: OutputId::from_raw(20),
            output_generation: 4,
        },
        affected_outputs: vec![OutputId::from_raw(10), OutputId::from_raw(20)],
    }
}

#[test]
fn target_survives_wire_independently_of_coverage_order() {
    let original = request();
    let wire = encode_wm_output_action_request(&original).unwrap();
    let frame =
        encode_wm_v1_output_action_request_frame(TransactionId::from_raw(6), &wire).unwrap();
    let (transaction, decoded) = decode_wm_v1_output_action_request_frame(&frame).unwrap();
    assert_eq!(transaction, TransactionId::from_raw(6));
    assert_eq!(
        decode_wm_output_action_request(&decoded, SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS).unwrap(),
        original
    );
    assert!(decode_wm_output_action_request(&decoded, 0).is_err());
    assert!(encode_wm_v1_policy_projection_request(&original).is_err());
    let mut reversed = original.clone();
    reversed.affected_outputs.reverse();
    let decoded = decode_wm_output_action_request(
        &encode_wm_output_action_request(&reversed).unwrap(),
        SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS,
    )
    .unwrap();
    assert_eq!(decoded.cause, original.cause);
}

#[test]
fn malformed_targets_never_become_legacy_actions() {
    let valid = encode_wm_output_action_request(&request()).unwrap();
    for field in 0..5 {
        let mut bad = valid.clone();
        match field {
            0 => bad.output = 0,
            1 => bad.output_generation = 0,
            2 => bad.activation_serial = 0,
            3 => bad.action = 0,
            _ => bad.output = 99,
        }
        assert!(
            decode_wm_output_action_request(&bad, SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS).is_err()
        );
    }
}

#[test]
fn output_policy_keys_are_gated_and_resolve_exact_generations() {
    let bounds = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    };
    let scene = PolicySceneSnapshot {
        generation: 1,
        active_output: OutputId::from_raw(1),
        outputs: vec![PolicyOutputSnapshot {
            output: OutputId::from_raw(1),
            generation: 3,
            policy_key: Some(17),
            bounds,
            work_area: bounds,
            focus: None,
        }],
        surfaces: vec![],
        session_operations: vec![],
    };
    let legacy =
        encode_wm_v1_policy_snapshot(TransactionId::from_raw(1), 1, &scene, &[], &[], 0).unwrap();
    assert!(
        !legacy
            .chunks
            .iter()
            .any(|c| c.record_kind == SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND)
    );
    let valid = encode_wm_v1_policy_snapshot(
        TransactionId::from_raw(1),
        1,
        &scene,
        &[],
        &[],
        SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS,
    )
    .unwrap();
    for variant in 0..5 {
        let mut transfer = valid.clone();
        let chunk = transfer
            .chunks
            .iter_mut()
            .find(|c| c.record_kind == SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND)
            .unwrap();
        match variant {
            0 => {}
            1 => chunk.data[8..16].copy_from_slice(&4_u64.to_le_bytes()),
            2 => chunk.data[16..24].fill(0),
            3 => {
                chunk.item_count = 2;
                chunk.data.extend(chunk.data.clone());
            }
            _ => {
                chunk.data.pop();
            }
        }
        let mut outputs = scene.outputs.clone();
        outputs[0].policy_key = None;
        let result = apply_wm_output_policy_keys(&transfer, &mut outputs);
        assert_eq!(result.is_ok(), variant == 0);
        if variant == 0 {
            assert_eq!(outputs[0].policy_key, Some(17));
        }
    }
}
