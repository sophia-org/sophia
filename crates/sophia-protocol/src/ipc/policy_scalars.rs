//! Scalar WM control semantics shared by every transport: projection requests
//! with all their causes, dirty requests, session operations and their
//! outcomes, and presentation receipts. These owners see only typed domain
//! values; no transport wrapper, byte layout, reserved field or legacy cause
//! numbering lives here.
//!
//! The public validators are strict and are what a new transport calls. The
//! legacy wrappers reach the same checks through the finer `pub(super)`
//! pieces, and keep privately the two historical acceptances this module does
//! not grant: a Focus or Interaction target decoded with an invalid index, and
//! a session-operation target the legacy encoder never checked.
use std::collections::BTreeSet;

use super::*;
use crate::{
    OutputId, PolicyDirtyRequest, PolicyInteractionAxis, PolicyInteractionKind,
    PolicyInteractionPhase, PolicyPresentationOutcome, PolicyPresentationReceipt,
    PolicyProjectionOutcome, PolicyProjectionRequest, PolicyRequestCause,
    PolicySessionOperationOutcome, PolicySessionOperationRequest,
};

fn invalid(field: &'static str, value: u32) -> IpcCodecError {
    IpcCodecError::InvalidEnum { field, value }
}

/// A complete, strictly valid projection request, whatever its cause.
pub fn validate_policy_projection_request(
    request: &PolicyProjectionRequest,
) -> Result<(), IpcCodecError> {
    validate_projection_request_identity(
        request.connection_epoch,
        request.request_id,
        request.scene_generation,
        request.policy_generation,
    )?;
    validate_policy_affected_outputs(&request.affected_outputs)?;
    validate_request_cause_scalars(&request.cause, &request.affected_outputs)?;
    validate_request_cause_targets(&request.cause)
}

/// The capabilities a connection must have negotiated to receive a cause.
/// Scene changes and plain focus need none.
pub const fn policy_request_cause_capabilities(cause: &PolicyRequestCause) -> u64 {
    match cause {
        PolicyRequestCause::SceneChanged | PolicyRequestCause::Focus { .. } => 0,
        PolicyRequestCause::Action { .. } => SOPHIA_WM_CAPABILITY_ACTIONS,
        PolicyRequestCause::OutputAction { .. } => {
            SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS
        }
        PolicyRequestCause::PresentationAction { .. } => {
            SOPHIA_WM_CAPABILITY_ACTIONS
                | SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
                | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS
        }
        PolicyRequestCause::PointerFocus { .. } => SOPHIA_WM_CAPABILITY_POINTER_FOCUS,
        PolicyRequestCause::Interaction { .. } => SOPHIA_WM_CAPABILITY_POINTER_INTERACTIONS,
    }
}

/// One to sixteen valid, distinct outputs.
pub fn validate_policy_affected_outputs(outputs: &[OutputId]) -> Result<(), IpcCodecError> {
    if outputs.is_empty() || outputs.len() > crate::POLICY_MAX_OUTPUTS {
        return Err(IpcCodecError::CountTooLarge {
            count: outputs.len(),
            max: crate::POLICY_MAX_OUTPUTS,
        });
    }
    let mut seen = BTreeSet::new();
    for output in outputs {
        if !output.is_valid() || !seen.insert(*output) {
            return Err(invalid("affected_output", output.raw() as u32));
        }
    }
    Ok(())
}

pub fn validate_policy_dirty_request(request: &PolicyDirtyRequest) -> Result<(), IpcCodecError> {
    validate_dirty_identity(request.connection_epoch, request.policy_generation)?;
    validate_policy_affected_outputs(&request.affected_outputs)
}

/// A strict session-operation request: its target, when present, is valid.
pub fn validate_policy_session_operation_request(
    request: &PolicySessionOperationRequest,
) -> Result<(), IpcCodecError> {
    validate_session_operation_identity(
        request.connection_epoch,
        request.request_id,
        request.operation,
    )?;
    if request.target.is_some_and(|target| !target.is_valid()) {
        return Err(invalid("session_operation_target", 0));
    }
    Ok(())
}

pub fn validate_policy_session_operation_outcome(
    outcome: &PolicySessionOperationOutcome,
) -> Result<(), IpcCodecError> {
    if outcome.connection_epoch == 0 || outcome.request_id == 0 {
        return Err(invalid("session_operation_outcome_identity", 0));
    }
    Ok(())
}

/// Identity shape only; the presented owner still matches it to the actual
/// publication and output epoch.
pub fn validate_policy_presentation_receipt(
    receipt: &PolicyPresentationReceipt,
) -> Result<(), IpcCodecError> {
    if receipt.connection_epoch == 0
        || receipt.publication_generation == 0
        || !receipt.output.is_valid()
        || receipt.output_generation == 0
        || receipt.presentation_epoch == 0
    {
        return Err(invalid("presentation_receipt", 0));
    }
    Ok(())
}

pub const fn policy_projection_outcome_code(outcome: PolicyProjectionOutcome) -> u16 {
    match outcome {
        PolicyProjectionOutcome::Committed => SOPHIA_WM_OUTCOME_COMMITTED,
        PolicyProjectionOutcome::RejectedStale => SOPHIA_WM_OUTCOME_REJECTED_STALE,
        PolicyProjectionOutcome::RejectedInvalid => SOPHIA_WM_OUTCOME_REJECTED_INVALID,
        PolicyProjectionOutcome::TimedOut => SOPHIA_WM_OUTCOME_TIMED_OUT,
        PolicyProjectionOutcome::Disconnected => SOPHIA_WM_OUTCOME_DISCONNECTED,
    }
}

/// `None` for a code that names no outcome; each transport reports that in
/// its own terms.
pub const fn policy_projection_outcome_from_code(code: u16) -> Option<PolicyProjectionOutcome> {
    Some(match code {
        SOPHIA_WM_OUTCOME_COMMITTED => PolicyProjectionOutcome::Committed,
        SOPHIA_WM_OUTCOME_REJECTED_STALE => PolicyProjectionOutcome::RejectedStale,
        SOPHIA_WM_OUTCOME_REJECTED_INVALID => PolicyProjectionOutcome::RejectedInvalid,
        SOPHIA_WM_OUTCOME_TIMED_OUT => PolicyProjectionOutcome::TimedOut,
        SOPHIA_WM_OUTCOME_DISCONNECTED => PolicyProjectionOutcome::Disconnected,
        _ => return None,
    })
}

pub const fn policy_presentation_outcome_code(outcome: PolicyPresentationOutcome) -> u16 {
    outcome as u16
}

pub const fn policy_presentation_outcome_from_code(code: u16) -> Option<PolicyPresentationOutcome> {
    Some(match code {
        1 => PolicyPresentationOutcome::Presented,
        2 => PolicyPresentationOutcome::Revoked,
        3 => PolicyPresentationOutcome::Withdrawn,
        _ => return None,
    })
}

pub const fn policy_interaction_phase_code(phase: PolicyInteractionPhase) -> u16 {
    phase as u16
}

pub const fn policy_interaction_phase_from_code(code: u16) -> Option<PolicyInteractionPhase> {
    Some(match code {
        1 => PolicyInteractionPhase::Begin,
        2 => PolicyInteractionPhase::Update,
        3 => PolicyInteractionPhase::End,
        4 => PolicyInteractionPhase::Cancel,
        _ => return None,
    })
}

pub const fn policy_interaction_kind_code(kind: PolicyInteractionKind) -> u16 {
    kind as u16
}

pub const fn policy_interaction_kind_from_code(code: u16) -> Option<PolicyInteractionKind> {
    Some(match code {
        1 => PolicyInteractionKind::Move,
        2 => PolicyInteractionKind::Resize,
        3 => PolicyInteractionKind::Drag,
        4 => PolicyInteractionKind::Scroll,
        _ => return None,
    })
}

pub const fn policy_interaction_axis_code(axis: PolicyInteractionAxis) -> u16 {
    axis as u16
}

pub const fn policy_interaction_axis_from_code(code: u16) -> Option<PolicyInteractionAxis> {
    Some(match code {
        0 => PolicyInteractionAxis::None,
        1 => PolicyInteractionAxis::Horizontal,
        2 => PolicyInteractionAxis::Vertical,
        _ => return None,
    })
}

// The pieces below are shared with the legacy wrappers, which call them in
// their historical order. They are not a permissive public mode.

pub(super) fn validate_projection_request_identity(
    connection_epoch: u64,
    request_id: u64,
    scene_generation: u64,
    policy_generation: u64,
) -> Result<(), IpcCodecError> {
    if connection_epoch == 0 || request_id == 0 || scene_generation == 0 || policy_generation == 0 {
        return Err(invalid("projection_request_identity", 0));
    }
    Ok(())
}

pub(super) fn validate_dirty_identity(
    connection_epoch: u64,
    policy_generation: u64,
) -> Result<(), IpcCodecError> {
    if connection_epoch == 0 || policy_generation == 0 {
        return Err(invalid("policy_dirty_identity", 0));
    }
    Ok(())
}

pub(super) fn validate_session_operation_identity(
    connection_epoch: u64,
    request_id: u64,
    operation: u64,
) -> Result<(), IpcCodecError> {
    if connection_epoch == 0 || request_id == 0 || operation == 0 {
        return Err(invalid("session_operation_identity", 0));
    }
    Ok(())
}

/// Every cause check except the validity of a Focus or Interaction target.
pub(super) fn validate_request_cause_scalars(
    cause: &PolicyRequestCause,
    affected_outputs: &[OutputId],
) -> Result<(), IpcCodecError> {
    let action = |activation_serial: u64, action: crate::WmActionId| {
        if activation_serial == 0 || !action.is_valid() {
            return Err(invalid("action_cause", 0));
        }
        Ok(())
    };
    match *cause {
        PolicyRequestCause::SceneChanged | PolicyRequestCause::Focus { .. } => Ok(()),
        PolicyRequestCause::Action {
            activation_serial,
            action: id,
        } => action(activation_serial, id),
        PolicyRequestCause::OutputAction {
            activation_serial,
            action: id,
            output,
            output_generation,
        } => {
            if !output.is_valid() || output_generation == 0 || !affected_outputs.contains(&output) {
                return Err(invalid("output_action_cause", 0));
            }
            action(activation_serial, id)
        }
        PolicyRequestCause::PresentationAction {
            activation_serial,
            action: id,
            identity,
        } => {
            if !crate::valid_policy_presentation_identity(identity)
                || !affected_outputs.contains(&identity.output)
            {
                return Err(invalid("presentation_action_cause", 0));
            }
            action(activation_serial, id)
        }
        PolicyRequestCause::PointerFocus { output, target } => {
            if !output.is_valid()
                || !affected_outputs.contains(&output)
                || target.is_some_and(|target| !target.is_valid())
            {
                return Err(invalid("pointer_focus_cause", 0));
            }
            Ok(())
        }
        PolicyRequestCause::Interaction {
            phase,
            kind,
            axis,
            geometry,
            ..
        } => {
            if !crate::valid_policy_interaction_payload(phase, kind, axis, geometry) {
                return Err(invalid("interaction_cause", 0));
            }
            Ok(())
        }
    }
}

/// The target validity the legacy decoder does not require.
pub(super) fn validate_request_cause_targets(
    cause: &PolicyRequestCause,
) -> Result<(), IpcCodecError> {
    match *cause {
        PolicyRequestCause::Focus { target } if !target.is_valid() => {
            Err(invalid("focus_cause", 0))
        }
        PolicyRequestCause::Interaction { target, .. } if !target.is_valid() => {
            Err(invalid("interaction_cause", 0))
        }
        _ => Ok(()),
    }
}
