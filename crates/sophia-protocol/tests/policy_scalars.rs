//! The neutral scalar WM semantics, directly, and the legacy wrappers that
//! reach them: unchanged bytes, and the two legacy acceptances that are
//! preserved deliberately rather than inherited from the strict owner.
#[path = "support/policy_scalar_fixture.rs"]
mod fixture;

use fixture::*;
use sophia_protocol::*;

const INVALID_TARGET: SurfaceId = SurfaceId::new(u32::MAX, 1);

fn field(result: Result<(), IpcCodecError>) -> &'static str {
    match result {
        Err(IpcCodecError::InvalidEnum { field, .. }) => field,
        Err(IpcCodecError::CountTooLarge { .. }) => "count",
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn every_cause_is_valid_as_built() {
    let mut requests = ordinary_causes()
        .into_iter()
        .map(request)
        .collect::<Vec<_>>();
    requests.extend([
        output_action(),
        presentation_action(false),
        presentation_action(true),
    ]);
    for request in requests {
        assert_eq!(
            validate_policy_projection_request(&request),
            Ok(()),
            "{request:?}"
        );
    }
}

#[test]
fn request_identity_fields_must_all_be_nonzero() {
    for index in 0..4 {
        let mut request = request(PolicyRequestCause::SceneChanged);
        match index {
            0 => request.connection_epoch = 0,
            1 => request.request_id = 0,
            2 => request.scene_generation = 0,
            _ => request.policy_generation = 0,
        }
        assert_eq!(
            field(validate_policy_projection_request(&request)),
            "projection_request_identity"
        );
    }
}

#[test]
fn affected_outputs_are_one_to_sixteen_valid_and_distinct() {
    assert_eq!(field(validate_policy_affected_outputs(&[])), "count");
    let seventeen = (1..=17).map(OutputId::from_raw).collect::<Vec<_>>();
    assert_eq!(field(validate_policy_affected_outputs(&seventeen)), "count");
    assert_eq!(validate_policy_affected_outputs(&seventeen[..16]), Ok(()));
    let zero = [OutputId::from_raw(1), OutputId::from_raw(0)];
    assert_eq!(
        field(validate_policy_affected_outputs(&zero)),
        "affected_output"
    );
    let duplicate = [OutputId::from_raw(1), OutputId::from_raw(1)];
    assert_eq!(
        field(validate_policy_affected_outputs(&duplicate)),
        "affected_output"
    );
}

#[test]
fn each_cause_refuses_its_own_invalid_values() {
    let action = WmActionId::from_raw(31);
    let refused = [
        (
            PolicyRequestCause::Action {
                activation_serial: 0,
                action,
            },
            "action_cause",
        ),
        (
            PolicyRequestCause::Action {
                activation_serial: 1,
                action: WmActionId::from_raw(0),
            },
            "action_cause",
        ),
        (
            PolicyRequestCause::Focus {
                target: INVALID_TARGET,
            },
            "focus_cause",
        ),
        (
            PolicyRequestCause::PointerFocus {
                output: OutputId::from_raw(3),
                target: None,
            },
            "pointer_focus_cause",
        ),
        (
            PolicyRequestCause::PointerFocus {
                output: OutputId::from_raw(1),
                target: Some(INVALID_TARGET),
            },
            "pointer_focus_cause",
        ),
        (
            PolicyRequestCause::Interaction {
                phase: PolicyInteractionPhase::Begin,
                kind: PolicyInteractionKind::Move,
                axis: PolicyInteractionAxis::None,
                target: INVALID_TARGET,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            },
            "interaction_cause",
        ),
        (
            PolicyRequestCause::Interaction {
                phase: PolicyInteractionPhase::Begin,
                kind: PolicyInteractionKind::Move,
                axis: PolicyInteractionAxis::None,
                target: SurfaceId::new(8, 3),
                geometry: Rect::default(),
            },
            "interaction_cause",
        ),
        (
            PolicyRequestCause::OutputAction {
                activation_serial: 1,
                action,
                output: OutputId::from_raw(3),
                output_generation: 1,
            },
            "output_action_cause",
        ),
        (
            PolicyRequestCause::OutputAction {
                activation_serial: 1,
                action,
                output: OutputId::from_raw(2),
                output_generation: 0,
            },
            "output_action_cause",
        ),
        (
            PolicyRequestCause::OutputAction {
                activation_serial: 0,
                action,
                output: OutputId::from_raw(2),
                output_generation: 1,
            },
            "action_cause",
        ),
        (
            PolicyRequestCause::PresentationAction {
                activation_serial: 1,
                action,
                identity: PolicyPresentationIdentity {
                    output: OutputId::from_raw(3),
                    ..identity(false)
                },
            },
            "presentation_action_cause",
        ),
        (
            PolicyRequestCause::PresentationAction {
                activation_serial: 1,
                action,
                identity: PolicyPresentationIdentity {
                    target_generation: 1,
                    ..identity(false)
                },
            },
            "presentation_action_cause",
        ),
        (
            PolicyRequestCause::PresentationAction {
                activation_serial: 1,
                action: WmActionId::from_raw(0),
                identity: identity(true),
            },
            "action_cause",
        ),
    ];
    for (cause, expected) in refused {
        assert_eq!(
            field(validate_policy_projection_request(&request(cause))),
            expected,
            "{cause:?}"
        );
    }
}

#[test]
fn cause_capabilities_are_the_file_path_map() {
    let action = WmActionId::from_raw(1);
    let target = SurfaceId::new(1, 1);
    let cases = [
        (PolicyRequestCause::SceneChanged, 0),
        (PolicyRequestCause::Focus { target }, 0),
        (
            PolicyRequestCause::Action {
                activation_serial: 1,
                action,
            },
            SOPHIA_WM_CAPABILITY_ACTIONS,
        ),
        (
            output_action().cause,
            SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS,
        ),
        (
            presentation_action(true).cause,
            SOPHIA_WM_CAPABILITY_ACTIONS
                | SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
                | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        ),
        (
            PolicyRequestCause::PointerFocus {
                output: OutputId::from_raw(1),
                target: None,
            },
            SOPHIA_WM_CAPABILITY_POINTER_FOCUS,
        ),
        (
            ordinary_causes()[5],
            SOPHIA_WM_CAPABILITY_POINTER_INTERACTIONS,
        ),
    ];
    for (cause, bits) in cases {
        assert_eq!(policy_request_cause_capabilities(&cause), bits, "{cause:?}");
    }
}

#[test]
fn dirty_session_operation_and_receipt_validators() {
    assert_eq!(validate_policy_dirty_request(&dirty()), Ok(()));
    let mut request = dirty();
    request.policy_generation = 0;
    assert_eq!(
        field(validate_policy_dirty_request(&request)),
        "policy_dirty_identity"
    );
    request = dirty();
    request.affected_outputs.clear();
    assert_eq!(field(validate_policy_dirty_request(&request)), "count");

    for target in [None, Some(SurfaceId::new(9, 4))] {
        assert_eq!(
            validate_policy_session_operation_request(&session_operation(target)),
            Ok(())
        );
    }
    for target in [SurfaceId::new(5, 0), INVALID_TARGET] {
        assert_eq!(
            field(validate_policy_session_operation_request(
                &session_operation(Some(target))
            )),
            "session_operation_target"
        );
    }
    let mut operation = session_operation(None);
    operation.operation = 0;
    assert_eq!(
        field(validate_policy_session_operation_request(&operation)),
        "session_operation_identity"
    );

    let outcome = PolicySessionOperationOutcome {
        connection_epoch: 3,
        request_id: 0,
        outcome: PolicyProjectionOutcome::Committed,
    };
    assert_eq!(
        field(validate_policy_session_operation_outcome(&outcome)),
        "session_operation_outcome_identity"
    );

    for outcome in PRESENTATION_OUTCOMES {
        assert_eq!(
            validate_policy_presentation_receipt(&receipt(outcome)),
            Ok(())
        );
    }
    let mut bad = receipt(PolicyPresentationOutcome::Presented);
    bad.presentation_epoch = 0;
    assert_eq!(
        field(validate_policy_presentation_receipt(&bad)),
        "presentation_receipt"
    );
    bad = receipt(PolicyPresentationOutcome::Presented);
    bad.output = OutputId::from_raw(0);
    assert_eq!(
        field(validate_policy_presentation_receipt(&bad)),
        "presentation_receipt"
    );
}

#[test]
fn every_scalar_code_round_trips_and_no_other_code_is_accepted() {
    for outcome in OUTCOMES {
        let code = policy_projection_outcome_code(outcome);
        assert_eq!(policy_projection_outcome_from_code(code), Some(outcome));
    }
    for outcome in PRESENTATION_OUTCOMES {
        let code = policy_presentation_outcome_code(outcome);
        assert_eq!(policy_presentation_outcome_from_code(code), Some(outcome));
    }
    for phase in [
        PolicyInteractionPhase::Begin,
        PolicyInteractionPhase::Update,
        PolicyInteractionPhase::End,
        PolicyInteractionPhase::Cancel,
    ] {
        assert_eq!(
            policy_interaction_phase_from_code(policy_interaction_phase_code(phase)),
            Some(phase)
        );
    }
    for kind in [
        PolicyInteractionKind::Move,
        PolicyInteractionKind::Resize,
        PolicyInteractionKind::Drag,
        PolicyInteractionKind::Scroll,
    ] {
        assert_eq!(
            policy_interaction_kind_from_code(policy_interaction_kind_code(kind)),
            Some(kind)
        );
    }
    for axis in [
        PolicyInteractionAxis::None,
        PolicyInteractionAxis::Horizontal,
        PolicyInteractionAxis::Vertical,
    ] {
        assert_eq!(
            policy_interaction_axis_from_code(policy_interaction_axis_code(axis)),
            Some(axis)
        );
    }
    let known_outcomes = OUTCOMES.map(policy_projection_outcome_code);
    for code in 0..=u16::MAX {
        assert_eq!(
            policy_projection_outcome_from_code(code).is_some(),
            known_outcomes.contains(&code)
        );
        assert_eq!(
            policy_presentation_outcome_from_code(code).is_some(),
            (1..=3).contains(&code)
        );
        assert_eq!(
            policy_interaction_phase_from_code(code).is_some(),
            (1..=4).contains(&code)
        );
        assert_eq!(
            policy_interaction_kind_from_code(code).is_some(),
            (1..=4).contains(&code)
        );
        assert_eq!(policy_interaction_axis_from_code(code).is_some(), code <= 2);
    }
}

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
        Some(IpcCodecError::InvalidEnum {
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
