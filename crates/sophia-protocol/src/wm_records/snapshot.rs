//! Scene snapshot rows: outputs, surfaces, actions, session operations and
//! the capability-gated extensions. A gated kind is encoded only when the
//! caller passes the capability set it selected for the peer.
use std::collections::BTreeSet;

use crate::wm_rows::{
    SNAPSHOT_ACTION_RECORD_KIND, SNAPSHOT_OUTPUT_RECORD_KIND,
    SNAPSHOT_SESSION_OPERATION_RECORD_KIND, SNAPSHOT_SURFACE_RECORD_KIND,
    SOPHIA_WM_CAPABILITY_ACTIONS, SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
    SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
    WmV1SnapshotActionRecord, WmV1SnapshotOutputRecord, WmV1SnapshotSessionOperationRecord,
    WmV1SnapshotSurfaceRecord, decode_wm_v1_snapshot_action_records,
    decode_wm_v1_snapshot_output_records, decode_wm_v1_snapshot_session_operation_records,
    decode_wm_v1_snapshot_surface_records, encode_wm_v1_snapshot_action_records,
    encode_wm_v1_snapshot_output_records, encode_wm_v1_snapshot_session_operation_records,
    encode_wm_v1_snapshot_surface_records,
};
use crate::{
    BinaryCodecError, LayoutNodeCapabilities, OutputId, PolicyActionRegistration,
    PolicyOutputSnapshot, PolicySceneSnapshot, PolicySessionOperation, PolicySurfaceClassification,
    PolicySurfaceKind, PolicySurfaceSnapshot, Rect, SurfaceConstraints, SurfaceId,
};

use super::configuration::{decode_policy_action_rows, encode_action_name};
use super::values::{
    decode_optional_size, decode_optional_surface, decode_presentation, encode_optional_size,
    encode_presentation, invalid, push_policy_section, require_count,
};
use super::{
    PolicyDecodedSnapshot, PolicyRecordContext, PolicyRecordSection, PolicyRecordSectionRef,
    PolicySnapshotMetadata, SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND,
    SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND, apply_policy_output_key_records,
    decode_wm_launch_context_records, encode_policy_launch_contexts_records,
    encode_policy_output_key_records, encode_wm_launch_context_records,
    validate_policy_record_sections,
};

/// First capability-gated snapshot extension. It deliberately lives outside
/// the generated ordinary-record range; see the forward-compatibility rule.
pub const SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND: u16 = 0xFF00;
const SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE: usize = 16;

pub const POLICY_SURFACE_CAPABILITY_MOVABLE: u16 = 1 << 0;
pub const POLICY_SURFACE_CAPABILITY_RESIZABLE: u16 = 1 << 1;
pub const POLICY_SURFACE_CAPABILITY_FOCUSABLE: u16 = 1 << 2;
pub const POLICY_SURFACE_CAPABILITY_CLOSABLE: u16 = 1 << 3;
pub const POLICY_SURFACE_CAPABILITY_FULLSCREENABLE: u16 = 1 << 4;
const POLICY_SURFACE_CAPABILITY_SUPPORTED: u16 = POLICY_SURFACE_CAPABILITY_MOVABLE
    | POLICY_SURFACE_CAPABILITY_RESIZABLE
    | POLICY_SURFACE_CAPABILITY_FOCUSABLE
    | POLICY_SURFACE_CAPABILITY_CLOSABLE
    | POLICY_SURFACE_CAPABILITY_FULLSCREENABLE;
const POLICY_SESSION_OPERATION_SURFACE_TARGET: u16 = 1 << 0;

// The row type keeps its historical name: it describes the row, not the
// legacy envelope that once carried it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmV1SnapshotSurfaceClassificationRecord {
    pub surface_index: u32,
    pub surface_generation: u32,
    pub classification: u64,
}

pub fn encode_wm_v1_snapshot_surface_classification_records(
    records: &[WmV1SnapshotSurfaceClassificationRecord],
) -> Result<Vec<u8>, BinaryCodecError> {
    if records.len() > crate::POLICY_MAX_SURFACES {
        return Err(BinaryCodecError::CountTooLarge {
            count: records.len(),
            max: crate::POLICY_MAX_SURFACES,
        });
    }
    let mut data = Vec::with_capacity(records.len() * SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE);
    for record in records {
        data.extend_from_slice(&record.surface_index.to_le_bytes());
        data.extend_from_slice(&record.surface_generation.to_le_bytes());
        data.extend_from_slice(&record.classification.to_le_bytes());
    }
    Ok(data)
}

pub fn decode_wm_v1_snapshot_surface_classification_records(
    data: &[u8],
    item_count: u32,
) -> Result<Vec<WmV1SnapshotSurfaceClassificationRecord>, BinaryCodecError> {
    let count = item_count as usize;
    if count > crate::POLICY_MAX_SURFACES {
        return Err(BinaryCodecError::CountTooLarge {
            count,
            max: crate::POLICY_MAX_SURFACES,
        });
    }
    let expected = count
        .checked_mul(SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE)
        .ok_or(BinaryCodecError::CountTooLarge {
            count,
            max: crate::POLICY_MAX_SURFACES,
        })?;
    if data.len() < expected {
        return Err(BinaryCodecError::Truncated);
    }
    if data.len() > expected {
        return Err(BinaryCodecError::TrailingBytes(data.len() - expected));
    }
    let mut records = Vec::with_capacity(count);
    for record in data.chunks_exact(SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE) {
        records.push(WmV1SnapshotSurfaceClassificationRecord {
            surface_index: u32::from_le_bytes(record[0..4].try_into().expect("fixed record")),
            surface_generation: u32::from_le_bytes(record[4..8].try_into().expect("fixed record")),
            classification: u64::from_le_bytes(record[8..16].try_into().expect("fixed record")),
        });
    }
    Ok(records)
}

pub fn encode_policy_snapshot_records(
    connection_epoch: u64,
    scene: &PolicySceneSnapshot,
    actions: &[PolicyActionRegistration],
    classifications: &[PolicySurfaceClassification],
    launch_origins: &[crate::PolicyLaunchContext],
    selected_capabilities: u64,
) -> Result<Vec<PolicyRecordSection>, BinaryCodecError> {
    if !scene.active_output.is_valid()
        || !scene
            .outputs
            .iter()
            .any(|output| output.output == scene.active_output)
    {
        return Err(invalid("snapshot_active_output", 0));
    }
    validate_snapshot_focus(scene)?;
    let outputs = scene
        .outputs
        .iter()
        .map(|output| {
            let (focus_index, focus_generation) = output
                .focus
                .map(|surface| (surface.index(), surface.generation()))
                .unwrap_or((0, 0));
            WmV1SnapshotOutputRecord {
                output: output.output.raw(),
                generation: output.generation,
                focus_index,
                focus_generation,
                x: output.bounds.x,
                y: output.bounds.y,
                width: output.bounds.width,
                height: output.bounds.height,
                work_x: output.work_area.x,
                work_y: output.work_area.y,
                work_width: output.work_area.width,
                work_height: output.work_area.height,
            }
        })
        .collect::<Vec<_>>();
    let surfaces = scene
        .surfaces
        .iter()
        .map(encode_surface_record)
        .collect::<Vec<_>>();
    let actions = if selected_capabilities & SOPHIA_WM_CAPABILITY_ACTIONS == 0 {
        Vec::new()
    } else {
        actions
            .iter()
            .map(|action| {
                let (name_len, name) = encode_action_name(&action.name)?;
                Ok(WmV1SnapshotActionRecord {
                    action: action.action.raw(),
                    session_operation_slot: action.session_operation_slot.unwrap_or(0),
                    name_len,
                    name,
                })
            })
            .collect::<Result<Vec<_>, BinaryCodecError>>()?
    };
    let session_operations = if selected_capabilities & SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS == 0
    {
        Vec::new()
    } else {
        scene
            .session_operations
            .iter()
            .map(|operation| WmV1SnapshotSessionOperationRecord {
                operation: operation.token,
                slot: operation.slot,
                target_bits: u16::from(operation.permits_surface_target)
                    * POLICY_SESSION_OPERATION_SURFACE_TARGET,
            })
            .collect::<Vec<_>>()
    };
    let classifications = if selected_capabilities & SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT == 0 {
        Vec::new()
    } else {
        let live_surfaces = scene
            .surfaces
            .iter()
            .map(|surface| surface.surface)
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        classifications
            .iter()
            .map(|classification| {
                if !classification.surface.is_valid()
                    || classification.classification == 0
                    || !live_surfaces.contains(&classification.surface)
                    || !seen.insert(classification.surface)
                {
                    return Err(invalid(
                        "surface_classification",
                        classification.surface.index(),
                    ));
                }
                Ok(WmV1SnapshotSurfaceClassificationRecord {
                    surface_index: classification.surface.index(),
                    surface_generation: classification.surface.generation(),
                    classification: classification.classification,
                })
            })
            .collect::<Result<Vec<_>, BinaryCodecError>>()?
    };
    let mut sections = Vec::new();
    push_policy_section(
        &mut sections,
        SNAPSHOT_OUTPUT_RECORD_KIND,
        outputs.len(),
        encode_wm_v1_snapshot_output_records(&outputs)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_SURFACE_RECORD_KIND,
        surfaces.len(),
        encode_wm_v1_snapshot_surface_records(&surfaces)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_ACTION_RECORD_KIND,
        actions.len(),
        encode_wm_v1_snapshot_action_records(&actions)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_SESSION_OPERATION_RECORD_KIND,
        session_operations.len(),
        encode_wm_v1_snapshot_session_operation_records(&session_operations)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
        classifications.len(),
        encode_wm_v1_snapshot_surface_classification_records(&classifications)?,
    )?;
    sections.extend(encode_policy_output_key_records(
        &scene.outputs,
        selected_capabilities,
    )?);
    if selected_capabilities & SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN != 0 {
        let mut origins = encode_policy_launch_contexts_records(launch_origins, connection_epoch)?;
        for section in &mut origins {
            section.kind = SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND;
        }
        sections.extend(origins);
    }
    sections.sort_by_key(|s| s.kind);
    Ok(sections)
}

pub fn decode_policy_snapshot_records(
    metadata: PolicySnapshotMetadata,
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<PolicyDecodedSnapshot, BinaryCodecError> {
    if !metadata.active_output.is_valid() {
        return Err(invalid("snapshot_transfer", 0));
    }
    validate_policy_record_sections(PolicyRecordContext::Snapshot, sections)?;
    decode_snapshot_sections(metadata, sections, None)
}

/// Decode complete sections. `declared` holds the output, surface, action and
/// session-operation counts when an envelope announces them apart from its
/// rows; they are checked after the rows decode, before any value is built.
pub(crate) fn decode_snapshot_sections(
    metadata: PolicySnapshotMetadata,
    sections: &[PolicyRecordSectionRef<'_>],
    declared: Option<[usize; 4]>,
) -> Result<PolicyDecodedSnapshot, BinaryCodecError> {
    let mut outputs = Vec::new();
    let mut surfaces = Vec::new();
    let mut actions = Vec::new();
    let mut session_operations = Vec::new();
    let mut classifications = Vec::new();
    for chunk in sections {
        match chunk.kind {
            SNAPSHOT_OUTPUT_RECORD_KIND => outputs.extend(decode_wm_v1_snapshot_output_records(
                chunk.bytes,
                chunk.count,
            )?),
            SNAPSHOT_SURFACE_RECORD_KIND => surfaces.extend(decode_wm_v1_snapshot_surface_records(
                chunk.bytes,
                chunk.count,
            )?),
            SNAPSHOT_ACTION_RECORD_KIND => actions.extend(decode_wm_v1_snapshot_action_records(
                chunk.bytes,
                chunk.count,
            )?),
            SNAPSHOT_SESSION_OPERATION_RECORD_KIND => session_operations.extend(
                decode_wm_v1_snapshot_session_operation_records(chunk.bytes, chunk.count)?,
            ),
            SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND => classifications.extend(
                decode_wm_v1_snapshot_surface_classification_records(chunk.bytes, chunk.count)?,
            ),
            SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND | SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND => {}
            other => return Err(invalid("snapshot_record_kind", u32::from(other))),
        }
    }
    if let Some(counts) = declared {
        require_count(outputs.len(), counts[0])?;
        require_count(surfaces.len(), counts[1])?;
        require_count(actions.len(), counts[2])?;
        require_count(session_operations.len(), counts[3])?;
    }
    let mut scene = PolicySceneSnapshot {
        generation: metadata.scene_generation,
        active_output: metadata.active_output,
        outputs: outputs
            .into_iter()
            .map(|record| {
                Ok(PolicyOutputSnapshot {
                    policy_key: None,
                    output: OutputId::from_raw(record.output),
                    generation: record.generation,
                    focus: decode_optional_surface(
                        record.focus_index,
                        record.focus_generation,
                        "output_focus",
                    )?,
                    bounds: Rect {
                        x: record.x,
                        y: record.y,
                        width: record.width,
                        height: record.height,
                    },
                    work_area: Rect {
                        x: record.work_x,
                        y: record.work_y,
                        width: record.work_width,
                        height: record.work_height,
                    },
                })
            })
            .collect::<Result<Vec<_>, BinaryCodecError>>()?,
        surfaces: surfaces
            .into_iter()
            .map(decode_surface_record)
            .collect::<Result<Vec<_>, _>>()?,
        session_operations: session_operations
            .into_iter()
            .map(|record| {
                if record.operation == 0
                    || record.slot == 0
                    || record.target_bits & !POLICY_SESSION_OPERATION_SURFACE_TARGET != 0
                {
                    return Err(invalid("session_operation", record.target_bits.into()));
                }
                Ok(PolicySessionOperation {
                    token: record.operation,
                    slot: record.slot,
                    permits_surface_target: record.target_bits
                        & POLICY_SESSION_OPERATION_SURFACE_TARGET
                        != 0,
                })
            })
            .collect::<Result<Vec<_>, BinaryCodecError>>()?,
    };
    apply_policy_output_key_records(sections, &mut scene.outputs)?;
    validate_snapshot_focus(&scene)?;
    let live_surfaces = scene
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<BTreeSet<_>>();
    let mut seen_classifications = BTreeSet::new();
    let classifications = classifications
        .into_iter()
        .map(|record| {
            let surface = SurfaceId::new(record.surface_index, record.surface_generation);
            if !surface.is_valid()
                || record.classification == 0
                || !live_surfaces.contains(&surface)
                || !seen_classifications.insert(surface)
            {
                return Err(invalid("surface_classification", record.surface_index));
            }
            Ok(PolicySurfaceClassification {
                surface,
                classification: record.classification,
            })
        })
        .collect::<Result<Vec<_>, BinaryCodecError>>()?;
    let mut launch_origins = Vec::new();
    for chunk in sections
        .iter()
        .filter(|c| c.kind == SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND)
    {
        if chunk.count == 0 {
            return Err(invalid("launch_origin_capability", 0));
        }
        launch_origins.extend(decode_wm_launch_context_records(chunk.bytes, chunk.count)?);
    }
    encode_wm_launch_context_records(&launch_origins)?;
    for origin in &launch_origins {
        if origin.epoch != metadata.connection_epoch || !live_surfaces.contains(&origin.surface) {
            return Err(invalid("launch_origin_surface", 0));
        }
    }
    Ok(PolicyDecodedSnapshot {
        launch_origins,
        scene,
        actions: decode_policy_action_rows(actions)?,
        classifications,
    })
}

fn encode_surface_record(surface: &PolicySurfaceSnapshot) -> WmV1SnapshotSurfaceRecord {
    let mut capability_bits = 0;
    capability_bits |= u16::from(surface.capabilities.movable) * POLICY_SURFACE_CAPABILITY_MOVABLE;
    capability_bits |=
        u16::from(surface.capabilities.resizable) * POLICY_SURFACE_CAPABILITY_RESIZABLE;
    capability_bits |=
        u16::from(surface.capabilities.focusable) * POLICY_SURFACE_CAPABILITY_FOCUSABLE;
    capability_bits |=
        u16::from(surface.capabilities.closable) * POLICY_SURFACE_CAPABILITY_CLOSABLE;
    capability_bits |=
        u16::from(surface.capabilities.fullscreenable) * POLICY_SURFACE_CAPABILITY_FULLSCREENABLE;
    let (transient_index, transient_generation) = surface
        .transient_owner
        .map(|owner| (owner.index(), owner.generation()))
        .unwrap_or((0, 0));
    let (min_width, min_height) = encode_optional_size(surface.constraints.min_size);
    let (max_width, max_height) = encode_optional_size(surface.constraints.max_size);
    WmV1SnapshotSurfaceRecord {
        surface_index: surface.surface.index(),
        surface_generation: surface.surface.generation(),
        state_generation: surface.generation,
        current_output: surface.current_output.map_or(0, OutputId::raw),
        capability_bits,
        kind: surface.kind as u16,
        request_state_bits: encode_presentation(surface.requested_state),
        current_state_bits: encode_presentation(surface.current_state),
        transient_index,
        transient_generation,
        x: surface.geometry.x,
        y: surface.geometry.y,
        width: surface.geometry.width,
        height: surface.geometry.height,
        min_width,
        min_height,
        max_width,
        max_height,
        exact_width: surface.exact_size.map_or(0, |size| size.width),
        exact_height: surface.exact_size.map_or(0, |size| size.height),
    }
}

fn decode_surface_record(
    record: WmV1SnapshotSurfaceRecord,
) -> Result<PolicySurfaceSnapshot, BinaryCodecError> {
    if record.capability_bits & !POLICY_SURFACE_CAPABILITY_SUPPORTED != 0 {
        return Err(invalid(
            "surface_capabilities",
            u32::from(record.capability_bits),
        ));
    }
    Ok(PolicySurfaceSnapshot {
        surface: SurfaceId::new(record.surface_index, record.surface_generation),
        generation: record.state_generation,
        current_output: (record.current_output != 0)
            .then(|| OutputId::from_raw(record.current_output)),
        kind: match record.kind {
            1 => PolicySurfaceKind::Toplevel,
            2 => PolicySurfaceKind::Dialog,
            3 => PolicySurfaceKind::Utility,
            4 => PolicySurfaceKind::Popup,
            5 => PolicySurfaceKind::Unknown,
            other => return Err(invalid("surface_kind", u32::from(other))),
        },
        capabilities: LayoutNodeCapabilities {
            movable: record.capability_bits & POLICY_SURFACE_CAPABILITY_MOVABLE != 0,
            resizable: record.capability_bits & POLICY_SURFACE_CAPABILITY_RESIZABLE != 0,
            focusable: record.capability_bits & POLICY_SURFACE_CAPABILITY_FOCUSABLE != 0,
            closable: record.capability_bits & POLICY_SURFACE_CAPABILITY_CLOSABLE != 0,
            fullscreenable: record.capability_bits & POLICY_SURFACE_CAPABILITY_FULLSCREENABLE != 0,
        },
        constraints: SurfaceConstraints {
            min_size: decode_optional_size(record.min_width, record.min_height, "min_size")?,
            max_size: decode_optional_size(record.max_width, record.max_height, "max_size")?,
        },
        exact_size: decode_optional_size(record.exact_width, record.exact_height, "exact_size")?,
        requested_state: decode_presentation(record.request_state_bits, "requested_state")?,
        current_state: decode_presentation(record.current_state_bits, "current_state")?,
        transient_owner: decode_optional_surface(
            record.transient_index,
            record.transient_generation,
            "transient_owner",
        )?,
        geometry: Rect {
            x: record.x,
            y: record.y,
            width: record.width,
            height: record.height,
        },
    })
}

fn validate_snapshot_focus(scene: &PolicySceneSnapshot) -> Result<(), BinaryCodecError> {
    for output in &scene.outputs {
        let Some(focus) = output.focus else {
            continue;
        };
        if !scene.surfaces.iter().any(|surface| {
            surface.surface == focus
                && surface.current_output == Some(output.output)
                && surface.capabilities.focusable
                && !surface.current_state.minimized
        }) {
            return Err(invalid("snapshot_output_focus", focus.index()));
        }
    }
    Ok(())
}
