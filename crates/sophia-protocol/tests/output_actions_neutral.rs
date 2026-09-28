//! The WM output-action cause and output policy keys of `output_actions.rs`
//! without the socket codecs: the WM file cycle, the scalar validator and the
//! shared snapshot records. The legacy wire fields and v1 frames stay in
//! `output_actions.rs` and retire with the socket wire (t269). This is the WM
//! role's output action, not the output role.
use sophia_protocol::wm_files::*;
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

// The file path's own map for this cause (`policy_scalars_neutral.rs`
// `cause_capabilities_are_the_file_path_map`).
const CAPS: u64 = SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS;

fn cycle_bytes(request: &PolicyProjectionRequest) -> Result<Vec<u8>, WmFilePayloadError> {
    encode_wm_file_cycle(
        WmFileHeader {
            kind: WmFileKind::Cycle,
            connection_epoch: request.connection_epoch,
            submission_id: 0,
            sequence: if wm_file_class(WmFileKind::Cycle) == WmFileClass::Event {
                203
            } else {
                0
            },
        },
        &WmFileCycle {
            snapshot_transaction: TransactionId::from_raw(5),
            request_transaction: TransactionId::from_raw(6),
            request: request.clone(),
        },
        CAPS,
    )
}

/// `target_survives_wire_independently_of_coverage_order`.
#[test]
fn target_survives_the_file_cycle_independently_of_coverage_order() {
    let original = request();
    let bytes = cycle_bytes(&original).unwrap();
    assert_eq!(
        decode_wm_file_cycle(&bytes, CAPS).unwrap().request,
        original
    );
    assert!(decode_wm_file_cycle(&bytes, 0).is_err());
    assert!(decode_wm_file_cycle(&bytes, SOPHIA_WM_CAPABILITY_ACTIONS).is_err());
    let mut reversed = original.clone();
    reversed.affected_outputs.reverse();
    let decoded = decode_wm_file_cycle(&cycle_bytes(&reversed).unwrap(), CAPS).unwrap();
    assert_eq!(decoded.request.cause, original.cause);
}

/// `malformed_targets_never_become_legacy_actions`.
#[test]
fn malformed_output_action_targets_are_refused() {
    assert_eq!(validate_policy_projection_request(&request()), Ok(()));
    for field in 0..5 {
        let mut bad = request();
        let PolicyRequestCause::OutputAction {
            activation_serial,
            action,
            output,
            output_generation,
        } = &mut bad.cause
        else {
            unreachable!()
        };
        match field {
            0 => *output = OutputId::from_raw(0),
            1 => *output_generation = 0,
            2 => *activation_serial = 0,
            3 => *action = WmActionId::from_raw(0),
            _ => *output = OutputId::from_raw(99),
        }
        assert!(
            validate_policy_projection_request(&bad).is_err(),
            "field {field}"
        );
        assert!(cycle_bytes(&bad).is_err(), "field {field}");
    }
}

/// `output_policy_keys_are_gated_and_resolve_exact_generations`.
#[test]
fn output_policy_key_records_are_gated_and_resolve_exact_generations() {
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
    let ungated = encode_policy_snapshot_records(1, &scene, &[], &[], &[], 0).unwrap();
    assert!(
        !ungated
            .iter()
            .any(|s| s.kind == SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND)
    );
    let valid = encode_policy_snapshot_records(
        1,
        &scene,
        &[],
        &[],
        &[],
        SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS,
    )
    .unwrap();
    for variant in 0..5 {
        let mut sections = valid.clone();
        let section = sections
            .iter_mut()
            .find(|s| s.kind == SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND)
            .unwrap();
        match variant {
            0 => {}
            1 => section.bytes[8..16].copy_from_slice(&4_u64.to_le_bytes()),
            2 => section.bytes[16..24].fill(0),
            3 => {
                section.count = 2;
                section.bytes.extend(section.bytes.clone());
            }
            _ => {
                section.bytes.pop();
            }
        }
        let refs = sections
            .iter()
            .map(PolicyRecordSection::as_ref)
            .collect::<Vec<_>>();
        let mut outputs = scene.outputs.clone();
        outputs[0].policy_key = None;
        let result = apply_policy_output_key_records(&refs, &mut outputs);
        assert_eq!(result.is_ok(), variant == 0, "variant {variant}");
        if variant == 0 {
            assert_eq!(outputs[0].policy_key, Some(17));
        }
    }
}
