//! Explicit action targets and opaque configured output affinity. Neither is
//! inferred from the ordered list of outputs a projection must cover.
use std::collections::BTreeSet;

use super::*;
use crate::{OutputId, PolicyOutputSnapshot, PolicyProjectionRequest, PolicyRequestCause};

pub const SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND: u16 = 0xff07;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "output_action_or_policy_key",
        value: 0,
    }
}

pub fn encode_wm_output_action_request(
    request: &PolicyProjectionRequest,
) -> Result<WmV1OutputActionRequest, IpcCodecError> {
    let PolicyRequestCause::OutputAction {
        activation_serial,
        action,
        output,
        output_generation,
    } = request.cause
    else {
        return Err(invalid());
    };
    if !output.is_valid() || output_generation == 0 || !request.affected_outputs.contains(&output) {
        return Err(invalid());
    }
    let mut legacy = request.clone();
    legacy.cause = PolicyRequestCause::Action {
        activation_serial,
        action,
    };
    let wire = encode_wm_v1_policy_projection_request(&legacy)?;
    Ok(WmV1OutputActionRequest {
        connection_epoch: wire.connection_epoch,
        request_id: wire.request_id,
        scene_generation: wire.scene_generation,
        policy_generation: wire.policy_generation,
        activation_serial,
        action: action.raw(),
        output: output.raw(),
        output_generation,
        affected_output_count: wire.affected_output_count,
        affected_outputs: wire.affected_outputs,
    })
}

pub fn decode_wm_output_action_request(
    wire: &WmV1OutputActionRequest,
    capabilities: u64,
) -> Result<PolicyProjectionRequest, IpcCodecError> {
    if capabilities & SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS == 0 {
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
    request.cause = PolicyRequestCause::OutputAction {
        activation_serial: wire.activation_serial,
        action: crate::WmActionId::from_raw(wire.action),
        output: OutputId::from_raw(wire.output),
        output_generation: wire.output_generation,
    };
    encode_wm_output_action_request(&request)?;
    Ok(request)
}

pub fn append_wm_output_policy_keys(
    transfer: &mut WmV1SnapshotTransfer,
    outputs: &[PolicyOutputSnapshot],
    capabilities: u64,
) -> Result<(), IpcCodecError> {
    if capabilities & SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS == 0 {
        return Ok(());
    }
    if outputs.len() > crate::POLICY_MAX_OUTPUTS {
        return Err(invalid());
    }
    let mut keys = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut data = Vec::new();
    for output in outputs {
        if let Some(key) = output.policy_key {
            if key == 0
                || !output.output.is_valid()
                || output.generation == 0
                || !keys.insert(key)
                || !ids.insert(output.output)
            {
                return Err(invalid());
            }
            data.extend(output.output.raw().to_le_bytes());
            data.extend(output.generation.to_le_bytes());
            data.extend(key.to_le_bytes());
        }
    }
    if !data.is_empty() {
        transfer.chunks.push(WmV1SnapshotChunk {
            connection_epoch: transfer.begin.connection_epoch,
            ordinal: u16::try_from(transfer.chunks.len()).map_err(|_| invalid())?,
            record_kind: SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND,
            item_count: keys.len() as u32,
            data,
        });
    }
    Ok(())
}

pub fn apply_wm_output_policy_keys(
    transfer: &WmV1SnapshotTransfer,
    outputs: &mut [PolicyOutputSnapshot],
) -> Result<(), IpcCodecError> {
    let mut keys = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for chunk in transfer
        .chunks
        .iter()
        .filter(|c| c.record_kind == SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND)
    {
        if chunk.item_count == 0
            || chunk.item_count as usize > crate::POLICY_MAX_OUTPUTS
            || chunk.data.len() != chunk.item_count as usize * 24
        {
            return Err(invalid());
        }
        for record in chunk.data.chunks_exact(24) {
            let id = u64::from_le_bytes(record[0..8].try_into().unwrap());
            let generation = u64::from_le_bytes(record[8..16].try_into().unwrap());
            let key = u64::from_le_bytes(record[16..24].try_into().unwrap());
            if key == 0 || !keys.insert(key) || !ids.insert(id) {
                return Err(invalid());
            }
            let output = outputs
                .iter_mut()
                .find(|o| o.output.raw() == id && o.generation == generation)
                .ok_or_else(invalid)?;
            output.policy_key = Some(key);
        }
    }
    Ok(())
}
