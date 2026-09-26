//! Every legacy scalar WM wrapper, over each cause and outcome, framed. The
//! bytes were captured at f5235c04, before the scalar semantics moved to their
//! neutral owner, and must not change.
#![allow(dead_code)]

use sophia_protocol::*;

pub const TRANSACTION: TransactionId = TransactionId::from_raw(5);

pub fn outputs() -> Vec<OutputId> {
    vec![OutputId::from_raw(1), OutputId::from_raw(2)]
}

pub fn request(cause: PolicyRequestCause) -> PolicyProjectionRequest {
    PolicyProjectionRequest {
        connection_epoch: 3,
        request_id: 17,
        scene_generation: 11,
        policy_generation: 13,
        affected_outputs: outputs(),
        cause,
    }
}

pub fn identity(target: bool) -> PolicyPresentationIdentity {
    PolicyPresentationIdentity {
        publication_generation: 21,
        output: OutputId::from_raw(2),
        output_generation: 23,
        presentation_epoch: 25,
        target_id: if target { 27 } else { 0 },
        target_generation: if target { 29 } else { 0 },
    }
}

/// Causes the ordinary scalar request carries.
pub fn ordinary_causes() -> Vec<PolicyRequestCause> {
    let action = WmActionId::from_raw(31);
    vec![
        PolicyRequestCause::SceneChanged,
        PolicyRequestCause::Action {
            activation_serial: 33,
            action,
        },
        PolicyRequestCause::Focus {
            target: SurfaceId::new(4, 1),
        },
        PolicyRequestCause::PointerFocus {
            output: OutputId::from_raw(2),
            target: None,
        },
        PolicyRequestCause::PointerFocus {
            output: OutputId::from_raw(1),
            target: Some(SurfaceId::new(6, 2)),
        },
        PolicyRequestCause::Interaction {
            phase: PolicyInteractionPhase::Update,
            kind: PolicyInteractionKind::Resize,
            axis: PolicyInteractionAxis::None,
            target: SurfaceId::new(8, 3),
            geometry: Rect {
                x: -5,
                y: 7,
                width: 640,
                height: 480,
            },
        },
        PolicyRequestCause::Interaction {
            phase: PolicyInteractionPhase::Cancel,
            kind: PolicyInteractionKind::Scroll,
            axis: PolicyInteractionAxis::Vertical,
            target: SurfaceId::new(8, 3),
            geometry: Rect::default(),
        },
    ]
}

pub fn output_action() -> PolicyProjectionRequest {
    request(PolicyRequestCause::OutputAction {
        activation_serial: 35,
        action: WmActionId::from_raw(37),
        output: OutputId::from_raw(2),
        output_generation: 39,
    })
}

pub fn presentation_action(target: bool) -> PolicyProjectionRequest {
    request(PolicyRequestCause::PresentationAction {
        activation_serial: 41,
        action: WmActionId::from_raw(43),
        identity: identity(target),
    })
}

pub const OUTCOMES: [PolicyProjectionOutcome; 5] = [
    PolicyProjectionOutcome::Committed,
    PolicyProjectionOutcome::RejectedStale,
    PolicyProjectionOutcome::RejectedInvalid,
    PolicyProjectionOutcome::TimedOut,
    PolicyProjectionOutcome::Disconnected,
];

pub const PRESENTATION_OUTCOMES: [PolicyPresentationOutcome; 3] = [
    PolicyPresentationOutcome::Presented,
    PolicyPresentationOutcome::Revoked,
    PolicyPresentationOutcome::Withdrawn,
];

pub fn dirty() -> PolicyDirtyRequest {
    PolicyDirtyRequest {
        connection_epoch: 3,
        policy_generation: 45,
        affected_outputs: outputs(),
    }
}

pub fn session_operation(target: Option<SurfaceId>) -> PolicySessionOperationRequest {
    PolicySessionOperationRequest {
        connection_epoch: 3,
        request_id: 47,
        operation: 49,
        target,
    }
}

pub fn receipt(outcome: PolicyPresentationOutcome) -> PolicyPresentationReceipt {
    PolicyPresentationReceipt {
        connection_epoch: 3,
        publication_generation: 51,
        output: OutputId::from_raw(1),
        output_generation: 53,
        presentation_epoch: 55,
        outcome,
    }
}

/// Session-operation targets, including the one the legacy encoder accepts
/// although its own decoder refuses it (generation zero).
pub fn session_targets() -> [Option<SurfaceId>; 3] {
    [None, Some(SurfaceId::new(9, 4)), Some(SurfaceId::new(5, 0))]
}

pub fn legacy_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for cause in ordinary_causes() {
        let wire = encode_wm_v1_policy_projection_request(&request(cause)).unwrap();
        bytes.extend(encode_wm_v1_projection_request_frame(TRANSACTION, &wire).unwrap());
    }
    let wire = encode_wm_output_action_request(&output_action()).unwrap();
    bytes.extend(encode_wm_v1_output_action_request_frame(TRANSACTION, &wire).unwrap());
    for target in [false, true] {
        let wire = encode_wm_presentation_action_request(&presentation_action(target)).unwrap();
        bytes.extend(encode_wm_v1_presentation_action_request_frame(TRANSACTION, &wire).unwrap());
    }
    for outcome in OUTCOMES {
        let wire = encode_wm_v1_policy_projection_outcome(3, 17, 11, outcome).unwrap();
        bytes.extend(encode_wm_v1_projection_outcome_frame(TRANSACTION, &wire).unwrap());
        let wire = encode_wm_v1_policy_session_operation_outcome(PolicySessionOperationOutcome {
            connection_epoch: 3,
            request_id: 47,
            outcome,
        })
        .unwrap();
        bytes.extend(encode_wm_v1_session_operation_outcome_frame(TRANSACTION, &wire).unwrap());
    }
    let wire = encode_wm_v1_policy_dirty(&dirty()).unwrap();
    bytes.extend(encode_wm_v1_policy_dirty_frame(TRANSACTION, &wire).unwrap());
    for target in session_targets() {
        let wire =
            encode_wm_v1_policy_session_operation_request(session_operation(target)).unwrap();
        bytes.extend(encode_wm_v1_session_operation_request_frame(TRANSACTION, &wire).unwrap());
    }
    for outcome in PRESENTATION_OUTCOMES {
        let wire = encode_wm_presentation_receipt(receipt(outcome)).unwrap();
        bytes.extend(encode_wm_v1_presentation_outcome_frame(TRANSACTION, &wire).unwrap());
    }
    bytes
}
