//! The legacy scalar wrappers that reach the neutral WM semantics: unchanged
//! bytes, and the two legacy acceptances that are preserved deliberately
//! rather than inherited from the strict owner. IPC-only: it retires with the
//! socket wire (t269). The neutral semantics are in `policy_scalars_neutral.rs`.
#[path = "support/policy_scalar_fixture.rs"]
mod fixture;
#[path = "support/policy_scalar_ipc_fixture.rs"]
mod fixture_ipc;

use fixture::*;
use fixture_ipc::*;
use sophia_protocol::*;

const INVALID_TARGET: SurfaceId = SurfaceId::new(u32::MAX, 1);

#[test]
fn legacy_scalar_bytes_are_unchanged() {
    assert_eq!(
        legacy_bytes(),
        include_bytes!("fixtures/policy-scalars-f5235c04.bin").as_slice()
    );
}

#[test]
fn legacy_wrappers_decode_to_the_values_they_encoded() {
    let capabilities = u64::MAX;
    for cause in ordinary_causes() {
        let request = request(cause);
        let wire = encode_wm_v1_policy_projection_request(&request).unwrap();
        assert_eq!(
            decode_wm_v1_policy_projection_request(&wire).unwrap(),
            request
        );
    }
    let wire = encode_wm_output_action_request(&output_action()).unwrap();
    assert_eq!(
        decode_wm_output_action_request(&wire, capabilities).unwrap(),
        output_action()
    );
    for target in [false, true] {
        let request = presentation_action(target);
        let wire = encode_wm_presentation_action_request(&request).unwrap();
        assert_eq!(
            decode_wm_presentation_action_request(&wire, capabilities).unwrap(),
            request
        );
    }
    let wire = encode_wm_v1_policy_dirty(&dirty()).unwrap();
    assert_eq!(decode_wm_v1_policy_dirty(&wire).unwrap(), dirty());
    for outcome in PRESENTATION_OUTCOMES {
        let wire = encode_wm_presentation_receipt(receipt(outcome)).unwrap();
        assert_eq!(
            decode_wm_presentation_receipt(&wire, capabilities).unwrap(),
            receipt(outcome)
        );
    }
}

#[test]
fn legacy_encoders_reach_the_strict_checks() {
    let invalid_focus = request(PolicyRequestCause::Focus {
        target: INVALID_TARGET,
    });
    assert_eq!(
        encode_wm_v1_policy_projection_request(&invalid_focus).err(),
        Some(BinaryCodecError::InvalidEnum {
            field: "focus_cause",
            value: 0
        })
    );
    let mut stray = output_action();
    if let PolicyRequestCause::OutputAction { output, .. } = &mut stray.cause {
        *output = OutputId::from_raw(3);
    }
    assert!(encode_wm_output_action_request(&stray).is_err());
    let mut bad = receipt(PolicyPresentationOutcome::Revoked);
    bad.output_generation = 0;
    assert!(encode_wm_presentation_receipt(bad).is_err());
    assert!(
        encode_wm_v1_policy_projection_request(&output_action()).is_err(),
        "a targeted action is never carried by the ordinary scalar request"
    );
}

/// Preserved legacy acceptance, not strict semantics: the scalar decoder
/// accepts a Focus or Interaction target with an invalid index, which the
/// strict validator refuses.
#[test]
fn legacy_decode_keeps_accepting_an_invalid_focus_or_interaction_target() {
    for cause in [ordinary_causes()[2], ordinary_causes()[5]] {
        let mut wire = encode_wm_v1_policy_projection_request(&request(cause)).unwrap();
        wire.target_index = u32::MAX;
        let decoded = decode_wm_v1_policy_projection_request(&wire).unwrap();
        assert!(validate_policy_projection_request(&decoded).is_err());
    }
}

/// Preserved legacy acceptance on both sides of the session-operation wire:
/// the encoder emits a target its decoder refuses, and the decoder accepts an
/// invalid index. The strict validator refuses both.
#[test]
fn legacy_session_operation_target_handling_is_characterized() {
    let unchecked = session_operation(Some(SurfaceId::new(5, 0)));
    let wire = encode_wm_v1_policy_session_operation_request(unchecked).unwrap();
    assert!(decode_wm_v1_policy_session_operation_request(&wire).is_err());
    assert!(validate_policy_session_operation_request(&unchecked).is_err());

    let mut wire = encode_wm_v1_policy_session_operation_request(session_operation(Some(
        SurfaceId::new(9, 4),
    )))
    .unwrap();
    wire.target_index = u32::MAX;
    let decoded = decode_wm_v1_policy_session_operation_request(&wire).unwrap();
    assert_eq!(decoded.target, Some(SurfaceId::new(u32::MAX, 4)));
    assert!(validate_policy_session_operation_request(&decoded).is_err());
}
