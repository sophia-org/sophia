use std::collections::BTreeSet;

use super::{IpcCodecError, WmV1ProjectionChunk};
use crate::{
    OutputId, POLICY_MAX_OVERVIEW_PLACEMENTS, POLICY_MAX_OVERVIEW_WORKSPACES,
    PolicyOverviewPlacement, PolicyOverviewWorkspace, Rect, SurfaceId,
};

pub fn encode_wm_overview_request(
    request: &crate::PolicyProjectionRequest,
) -> Result<super::WmV1OverviewRequest, IpcCodecError> {
    let mut base = request.clone();
    base.cause = crate::PolicyRequestCause::SceneChanged;
    let checked = super::encode_wm_v1_policy_projection_request(&base)?;
    let mut result = super::WmV1OverviewRequest {
        connection_epoch: checked.connection_epoch,
        request_id: checked.request_id,
        scene_generation: checked.scene_generation,
        policy_generation: checked.policy_generation,
        activation_serial: 0,
        output: 0,
        output_generation: 0,
        workspace: 0,
        target_index: 0,
        target_generation: 0,
        operation: 0,
        affected_output_count: checked.affected_output_count,
        affected_outputs: checked.affected_outputs,
    };
    match request.cause {
        crate::PolicyRequestCause::OverviewQuery => {}
        crate::PolicyRequestCause::OverviewSelection {
            activation_serial,
            output,
            output_generation,
            workspace,
            target,
        } => {
            if activation_serial == 0
                || !output.is_valid()
                || output_generation == 0
                || workspace == 0
                || !request.affected_outputs.contains(&output)
                || target.is_some_and(|surface| !surface.is_valid())
            {
                return Err(invalid());
            }
            result.operation = 1;
            result.activation_serial = activation_serial;
            result.output = output.raw();
            result.output_generation = output_generation;
            result.workspace = workspace;
            if let Some(target) = target {
                result.target_index = target.index();
                result.target_generation = target.generation();
            }
        }
        _ => return Err(invalid()),
    }
    Ok(result)
}

pub fn decode_wm_overview_request(
    wire: &super::WmV1OverviewRequest,
    capabilities: u64,
) -> Result<crate::PolicyProjectionRequest, IpcCodecError> {
    if capabilities & super::SOPHIA_WM_CAPABILITY_OVERVIEW == 0 {
        return Err(invalid());
    }
    let base = super::WmV1ProjectionRequest {
        connection_epoch: wire.connection_epoch,
        request_id: wire.request_id,
        scene_generation: wire.scene_generation,
        policy_generation: wire.policy_generation,
        cause_kind: 0,
        interaction_phase: 0,
        interaction_kind: 0,
        interaction_axis: 0,
        activation_serial: 0,
        action: 0,
        target_index: 0,
        target_generation: 0,
        interaction_x: 0,
        interaction_y: 0,
        interaction_width: 0,
        interaction_height: 0,
        affected_output_count: wire.affected_output_count,
        affected_outputs: wire.affected_outputs.clone(),
    };
    let mut request = super::decode_wm_v1_policy_projection_request(&base)?;
    request.cause = match wire.operation {
        0 => crate::PolicyRequestCause::OverviewQuery,
        1 => crate::PolicyRequestCause::OverviewSelection {
            activation_serial: wire.activation_serial,
            output: OutputId::from_raw(wire.output),
            output_generation: wire.output_generation,
            workspace: wire.workspace,
            target: if wire.target_generation == 0 && wire.target_index == 0 {
                None
            } else {
                Some(SurfaceId::new(wire.target_index, wire.target_generation))
            },
        },
        _ => return Err(invalid()),
    };
    // Exact re-encoding also enforces zero query fields and canonical nulls.
    if encode_wm_overview_request(&request)? != *wire {
        return Err(invalid());
    }
    Ok(request)
}

pub const PROJECTION_OVERVIEW_WORKSPACE_RECORD_KIND: u16 = 0xff09;
pub const PROJECTION_OVERVIEW_PLACEMENT_RECORD_KIND: u16 = 0xff0a;
pub const PROJECTION_OVERVIEW_WORKSPACE_RECORD_LEN: usize = 48;
pub const PROJECTION_OVERVIEW_PLACEMENT_RECORD_LEN: usize = 40;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidRecord("wm_overview")
}

pub fn validate_wm_overview(workspaces: &[PolicyOverviewWorkspace]) -> Result<(), IpcCodecError> {
    if workspaces.len() > POLICY_MAX_OVERVIEW_WORKSPACES {
        return Err(invalid());
    }
    let mut keys = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut count = 0usize;
    for workspace in workspaces {
        if !workspace.output.is_valid()
            || workspace.workspace == 0
            || workspace.bounds.is_empty()
            || !keys.insert((workspace.output, workspace.workspace))
            || workspace.active && !active.insert(workspace.output)
        {
            return Err(invalid());
        }
        outputs.insert(workspace.output);
        count = count
            .checked_add(workspace.placements.len())
            .ok_or_else(invalid)?;
        if count > POLICY_MAX_OVERVIEW_PLACEMENTS {
            return Err(invalid());
        }
        let mut surfaces = BTreeSet::new();
        for placement in &workspace.placements {
            if !placement.surface.is_valid()
                || placement.geometry.is_empty()
                || !surfaces.insert(placement.surface)
            {
                return Err(invalid());
            }
        }
        if workspace
            .focus
            .is_some_and(|focus| !surfaces.contains(&focus))
        {
            return Err(invalid());
        }
    }
    if outputs != active {
        return Err(invalid());
    }
    Ok(())
}

pub fn encode_wm_overview(
    workspaces: &[PolicyOverviewWorkspace],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    validate_wm_overview(workspaces)?;
    if epoch == 0 {
        return Err(invalid());
    }
    let mut headers = Vec::new();
    let mut placements = Vec::new();
    for workspace in workspaces {
        headers.extend(workspace.output.raw().to_le_bytes());
        headers.extend(workspace.workspace.to_le_bytes());
        push_rect(&mut headers, workspace.bounds);
        let focus = workspace.focus.unwrap_or(SurfaceId::INVALID);
        headers.extend(focus.index().to_le_bytes());
        headers.extend(focus.generation().to_le_bytes());
        headers.extend((workspace.placements.len() as u32).to_le_bytes());
        headers.extend(u32::from(workspace.active).to_le_bytes());
        for placement in &workspace.placements {
            placements.extend(workspace.output.raw().to_le_bytes());
            placements.extend(workspace.workspace.to_le_bytes());
            placements.extend(placement.surface.index().to_le_bytes());
            placements.extend(placement.surface.generation().to_le_bytes());
            push_rect(&mut placements, placement.geometry);
        }
    }
    let mut chunks = Vec::new();
    for (kind, size, data) in [
        (
            PROJECTION_OVERVIEW_WORKSPACE_RECORD_KIND,
            PROJECTION_OVERVIEW_WORKSPACE_RECORD_LEN,
            headers,
        ),
        (
            PROJECTION_OVERVIEW_PLACEMENT_RECORD_KIND,
            PROJECTION_OVERVIEW_PLACEMENT_RECORD_LEN,
            placements,
        ),
    ] {
        for bytes in data.chunks((65520 / size) * size) {
            chunks.push(WmV1ProjectionChunk {
                connection_epoch: epoch,
                ordinal: ordinal
                    .checked_add(u16::try_from(chunks.len()).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?,
                record_kind: kind,
                item_count: (bytes.len() / size) as u32,
                data: bytes.to_vec(),
            });
        }
    }
    Ok(chunks)
}

pub fn decode_wm_overview(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyOverviewWorkspace>, IpcCodecError> {
    let mut workspaces = Vec::new();
    let mut counts = Vec::new();
    let mut placements = Vec::new();
    for chunk in chunks {
        let size = match chunk.record_kind {
            PROJECTION_OVERVIEW_WORKSPACE_RECORD_KIND => PROJECTION_OVERVIEW_WORKSPACE_RECORD_LEN,
            PROJECTION_OVERVIEW_PLACEMENT_RECORD_KIND => PROJECTION_OVERVIEW_PLACEMENT_RECORD_LEN,
            _ => continue,
        };
        if chunk.item_count == 0 || chunk.data.len() != chunk.item_count as usize * size {
            return Err(invalid());
        }
        for bytes in chunk.data.chunks_exact(size) {
            if size == PROJECTION_OVERVIEW_WORKSPACE_RECORD_LEN {
                if workspaces.len() >= POLICY_MAX_OVERVIEW_WORKSPACES || u32_at(bytes, 44) > 1 {
                    return Err(invalid());
                }
                let focus = SurfaceId::new(u32_at(bytes, 32), u32_at(bytes, 36));
                if focus != SurfaceId::INVALID && !focus.is_valid() {
                    return Err(invalid());
                }
                workspaces.push(PolicyOverviewWorkspace {
                    output: OutputId::from_raw(u64_at(bytes, 0)),
                    workspace: u64_at(bytes, 8),
                    bounds: rect_at(bytes, 16),
                    active: u32_at(bytes, 44) == 1,
                    focus: (focus != SurfaceId::INVALID).then_some(focus),
                    placements: Vec::new(),
                });
                counts.push(u32_at(bytes, 40) as usize);
            } else {
                if placements.len() >= POLICY_MAX_OVERVIEW_PLACEMENTS {
                    return Err(invalid());
                }
                placements.push((
                    u64_at(bytes, 0),
                    u64_at(bytes, 8),
                    PolicyOverviewPlacement {
                        surface: SurfaceId::new(u32_at(bytes, 16), u32_at(bytes, 20)),
                        geometry: rect_at(bytes, 24),
                    },
                ));
            }
        }
    }
    let mut cursor = placements.into_iter();
    for (workspace, count) in workspaces.iter_mut().zip(counts) {
        if count > POLICY_MAX_OVERVIEW_PLACEMENTS {
            return Err(invalid());
        }
        for _ in 0..count {
            let (output, token, placement) = cursor.next().ok_or_else(invalid)?;
            if output != workspace.output.raw() || token != workspace.workspace {
                return Err(invalid());
            }
            workspace.placements.push(placement);
        }
    }
    if cursor.next().is_some() {
        return Err(invalid());
    }
    validate_wm_overview(&workspaces)?;
    Ok(workspaces)
}

fn push_rect(bytes: &mut Vec<u8>, rect: Rect) {
    for value in [rect.x, rect.y, rect.width, rect.height] {
        bytes.extend(value.to_le_bytes());
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn rect_at(bytes: &[u8], offset: usize) -> Rect {
    Rect {
        x: u32_at(bytes, offset) as i32,
        y: u32_at(bytes, offset + 4) as i32,
        width: u32_at(bytes, offset + 8) as i32,
        height: u32_at(bytes, offset + 12) as i32,
    }
}
