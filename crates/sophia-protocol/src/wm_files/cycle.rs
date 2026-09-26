//! A cycle names its immutable snapshot and a complete semantic request.
//! Cause bodies are compact file records, not legacy scalar frames.
use super::codec::{u16_at, u32_at, u64_at};
use super::payload::*;
use super::*;
use crate::*;

fn encode_cause(cause: PolicyRequestCause) -> (u16, Vec<u8>) {
    let mut bytes = Vec::new();
    let kind = match cause {
        PolicyRequestCause::SceneChanged => 0,
        PolicyRequestCause::Action {
            activation_serial,
            action,
        } => {
            bytes.extend(activation_serial.to_le_bytes());
            bytes.extend(action.raw().to_le_bytes());
            1
        }
        PolicyRequestCause::Focus { target } => {
            push_surface(&mut bytes, Some(target));
            2
        }
        PolicyRequestCause::PointerFocus { output, target } => {
            bytes.extend(output.raw().to_le_bytes());
            push_surface(&mut bytes, target);
            3
        }
        PolicyRequestCause::Interaction {
            phase,
            kind,
            axis,
            target,
            geometry,
        } => {
            bytes.extend(policy_interaction_phase_code(phase).to_le_bytes());
            bytes.extend(policy_interaction_kind_code(kind).to_le_bytes());
            bytes.extend(policy_interaction_axis_code(axis).to_le_bytes());
            bytes.extend([0; 2]);
            push_surface(&mut bytes, Some(target));
            bytes.extend(geometry.x.to_le_bytes());
            bytes.extend(geometry.y.to_le_bytes());
            bytes.extend(geometry.width.to_le_bytes());
            bytes.extend(geometry.height.to_le_bytes());
            4
        }
        PolicyRequestCause::OutputAction {
            activation_serial,
            action,
            output,
            output_generation,
        } => {
            for value in [
                activation_serial,
                action.raw(),
                output.raw(),
                output_generation,
            ] {
                bytes.extend(value.to_le_bytes());
            }
            5
        }
        PolicyRequestCause::PresentationAction {
            activation_serial,
            action,
            identity,
        } => {
            for value in [
                activation_serial,
                action.raw(),
                identity.publication_generation,
                identity.output.raw(),
                identity.output_generation,
                identity.presentation_epoch,
                identity.target_id,
                identity.target_generation,
            ] {
                bytes.extend(value.to_le_bytes());
            }
            6
        }
    };
    (kind, bytes)
}

fn decode_cause(kind: u16, bytes: &[u8]) -> Result<PolicyRequestCause, WmFilePayloadError> {
    let size = match kind {
        0 => 0,
        1 | 3 => 16,
        2 => 8,
        4 | 5 => 32,
        6 => 64,
        _ => return Err(WmFilePayloadError::Value),
    };
    if bytes.len() != size {
        return Err(WmFileCodecError::Length.into());
    }
    Ok(match kind {
        0 => PolicyRequestCause::SceneChanged,
        1 => PolicyRequestCause::Action {
            activation_serial: u64_at(bytes, 0)?,
            action: WmActionId::from_raw(u64_at(bytes, 8)?),
        },
        2 => PolicyRequestCause::Focus {
            target: surface(bytes, 0)?,
        },
        3 => PolicyRequestCause::PointerFocus {
            output: OutputId::from_raw(u64_at(bytes, 0)?),
            target: optional_surface(bytes, 8)?,
        },
        4 => {
            reserved(&bytes[6..8])?;
            PolicyRequestCause::Interaction {
                phase: policy_interaction_phase_from_code(u16_at(bytes, 0)?)
                    .ok_or(WmFilePayloadError::Value)?,
                kind: policy_interaction_kind_from_code(u16_at(bytes, 2)?)
                    .ok_or(WmFilePayloadError::Value)?,
                axis: policy_interaction_axis_from_code(u16_at(bytes, 4)?)
                    .ok_or(WmFilePayloadError::Value)?,
                target: surface(bytes, 8)?,
                geometry: Rect {
                    x: i32::from_le_bytes(u32_at(bytes, 16)?.to_le_bytes()),
                    y: i32::from_le_bytes(u32_at(bytes, 20)?.to_le_bytes()),
                    width: i32::from_le_bytes(u32_at(bytes, 24)?.to_le_bytes()),
                    height: i32::from_le_bytes(u32_at(bytes, 28)?.to_le_bytes()),
                },
            }
        }
        5 => PolicyRequestCause::OutputAction {
            activation_serial: u64_at(bytes, 0)?,
            action: WmActionId::from_raw(u64_at(bytes, 8)?),
            output: OutputId::from_raw(u64_at(bytes, 16)?),
            output_generation: u64_at(bytes, 24)?,
        },
        6 => PolicyRequestCause::PresentationAction {
            activation_serial: u64_at(bytes, 0)?,
            action: WmActionId::from_raw(u64_at(bytes, 8)?),
            identity: PolicyPresentationIdentity {
                publication_generation: u64_at(bytes, 16)?,
                output: OutputId::from_raw(u64_at(bytes, 24)?),
                output_generation: u64_at(bytes, 32)?,
                presentation_epoch: u64_at(bytes, 40)?,
                target_id: u64_at(bytes, 48)?,
                target_generation: u64_at(bytes, 56)?,
            },
        },
        _ => return Err(WmFilePayloadError::Value),
    })
}

pub fn encode_wm_file_cycle(
    header: WmFileHeader,
    cycle: &WmFileCycle,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Cycle)?;
    let request = &cycle.request;
    same_epoch(header, request.connection_epoch)?;
    identity(&[
        cycle.snapshot_transaction.raw(),
        cycle.request_transaction.raw(),
    ])?;
    validate_policy_projection_request(request)?;
    require_capabilities(
        capabilities,
        policy_request_cause_capabilities(&request.cause),
    )?;
    let (kind, cause) = encode_cause(request.cause);
    let mut bytes = Vec::new();
    for value in [
        cycle.snapshot_transaction.raw(),
        cycle.request_transaction.raw(),
        request.request_id,
        request.scene_generation,
        request.policy_generation,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(kind.to_le_bytes());
    bytes.extend(
        u16::try_from(request.affected_outputs.len())
            .map_err(|_| WmFileCodecError::Length)?
            .to_le_bytes(),
    );
    bytes.extend([0; 4]);
    for output in &request.affected_outputs {
        bytes.extend(output.raw().to_le_bytes());
    }
    bytes.extend(cause);
    Ok(encode_wm_file_record(header, &bytes)?)
}

pub fn decode_wm_file_cycle(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFileCycle, WmFilePayloadError> {
    let r = record(bytes, WmFileKind::Cycle, WM_FILE_CYCLE_PREFIX_BYTES)?;
    reserved(&r.body[44..48])?;
    let snapshot_transaction = u64_at(r.body, 0)?;
    let request_transaction = u64_at(r.body, 8)?;
    identity(&[snapshot_transaction, request_transaction])?;
    let count = u16_at(r.body, 42)?;
    let end = WM_FILE_CYCLE_PREFIX_BYTES + usize::from(count) * 8;
    let output_bytes = r
        .body
        .get(WM_FILE_CYCLE_PREFIX_BYTES..end)
        .ok_or(WmFileCodecError::Length)?;
    let request = PolicyProjectionRequest {
        connection_epoch: r.header.connection_epoch,
        request_id: u64_at(r.body, 16)?,
        scene_generation: u64_at(r.body, 24)?,
        policy_generation: u64_at(r.body, 32)?,
        affected_outputs: outputs(output_bytes, count)?,
        cause: decode_cause(u16_at(r.body, 40)?, &r.body[end..])?,
    };
    validate_policy_projection_request(&request)?;
    require_capabilities(
        capabilities,
        policy_request_cause_capabilities(&request.cause),
    )?;
    Ok(WmFileCycle {
        snapshot_transaction: TransactionId::from_raw(snapshot_transaction),
        request_transaction: TransactionId::from_raw(request_transaction),
        request,
    })
}
