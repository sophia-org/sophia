//! Reduced presented actions and lifecycle receipts. Wire shape validation
//! grants no input authority; the presented owner supplies that identity.
use super::*;
use crate::{
    OutputId, PolicyPresentationIdentity, PolicyPresentationOutcome, PolicyPresentationReceipt,
    PolicyProjectionRequest, PolicyRequestCause, WmActionId,
};

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "presentation_action_or_receipt",
        value: 0,
    }
}

pub fn encode_wm_presentation_action_request(
    request: &PolicyProjectionRequest,
) -> Result<WmV1PresentationActionRequest, IpcCodecError> {
    let PolicyRequestCause::PresentationAction {
        activation_serial,
        action,
        identity,
    } = request.cause
    else {
        return Err(invalid());
    };
    if !crate::valid_policy_presentation_identity(identity)
        || !request.affected_outputs.contains(&identity.output)
    {
        return Err(invalid());
    }
    let mut legacy = request.clone();
    legacy.cause = PolicyRequestCause::Action {
        activation_serial,
        action,
    };
    let wire = encode_wm_v1_policy_projection_request(&legacy)?;
    Ok(WmV1PresentationActionRequest {
        connection_epoch: wire.connection_epoch,
        request_id: wire.request_id,
        scene_generation: wire.scene_generation,
        policy_generation: wire.policy_generation,
        activation_serial,
        action: action.raw(),
        output: identity.output.raw(),
        output_generation: identity.output_generation,
        publication_generation: identity.publication_generation,
        presentation_epoch: identity.presentation_epoch,
        target_id: identity.target_id,
        target_generation: identity.target_generation,
        affected_output_count: wire.affected_output_count,
        affected_outputs: wire.affected_outputs,
    })
}

pub fn decode_wm_presentation_action_request(
    wire: &WmV1PresentationActionRequest,
    capabilities: u64,
) -> Result<PolicyProjectionRequest, IpcCodecError> {
    let required =
        SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
    if capabilities & required != required {
        return Err(invalid());
    }
    let mut request = decode_wm_v1_policy_projection_request(&WmV1ProjectionRequest {
        connection_epoch: wire.connection_epoch,
        request_id: wire.request_id,
        scene_generation: wire.scene_generation,
        policy_generation: wire.policy_generation,
        cause_kind: 1,
        interaction_phase: 0,
        interaction_kind: 0,
        interaction_axis: 0,
        activation_serial: wire.activation_serial,
        action: wire.action,
        target_index: 0,
        target_generation: 0,
        interaction_x: 0,
        interaction_y: 0,
        interaction_width: 0,
        interaction_height: 0,
        affected_output_count: wire.affected_output_count,
        affected_outputs: wire.affected_outputs.clone(),
    })?;
    request.cause = PolicyRequestCause::PresentationAction {
        activation_serial: wire.activation_serial,
        action: WmActionId::from_raw(wire.action),
        identity: PolicyPresentationIdentity {
            publication_generation: wire.publication_generation,
            output: OutputId::from_raw(wire.output),
            output_generation: wire.output_generation,
            presentation_epoch: wire.presentation_epoch,
            target_id: wire.target_id,
            target_generation: wire.target_generation,
        },
    };
    encode_wm_presentation_action_request(&request)?;
    Ok(request)
}

pub fn encode_wm_presentation_receipt(
    receipt: PolicyPresentationReceipt,
) -> Result<WmV1PresentationOutcome, IpcCodecError> {
    if receipt.connection_epoch == 0
        || receipt.publication_generation == 0
        || !receipt.output.is_valid()
        || receipt.output_generation == 0
        || receipt.presentation_epoch == 0
    {
        return Err(invalid());
    }
    Ok(WmV1PresentationOutcome {
        connection_epoch: receipt.connection_epoch,
        publication_generation: receipt.publication_generation,
        output: receipt.output.raw(),
        output_generation: receipt.output_generation,
        presentation_epoch: receipt.presentation_epoch,
        outcome: receipt.outcome as u16,
    })
}

pub fn decode_wm_presentation_receipt(
    wire: &WmV1PresentationOutcome,
    capabilities: u64,
) -> Result<PolicyPresentationReceipt, IpcCodecError> {
    if capabilities & SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES == 0 {
        return Err(invalid());
    }
    let receipt = PolicyPresentationReceipt {
        connection_epoch: wire.connection_epoch,
        publication_generation: wire.publication_generation,
        output: OutputId::from_raw(wire.output),
        output_generation: wire.output_generation,
        presentation_epoch: wire.presentation_epoch,
        outcome: match wire.outcome {
            1 => PolicyPresentationOutcome::Presented,
            2 => PolicyPresentationOutcome::Revoked,
            3 => PolicyPresentationOutcome::Withdrawn,
            _ => return Err(invalid()),
        },
    };
    encode_wm_presentation_receipt(receipt)?;
    Ok(receipt)
}
