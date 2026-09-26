use core::mem::size_of;

use crate::{
    LayoutNodeCapabilities, OutputId, PolicyActionRegistration, PolicyConfiguration,
    PolicyDirtyRequest, PolicyInteractionAxis, PolicyInteractionKind, PolicyInteractionPhase,
    PolicyOutputProjection, PolicyOutputSnapshot, PolicyPresentationState,
    PolicyProjectionIndicator, PolicyProjectionOutcome, PolicyProjectionOutputStatus,
    PolicyProjectionProposal, PolicyRequestCause, PolicySceneSnapshot, PolicySessionOperation,
    PolicySessionOperationOutcome, PolicySessionOperationRequest, PolicySurfaceClassification,
    PolicySurfaceKind, PolicySurfacePlacement, PolicySurfaceSnapshot, PolicyTransform, Rect,
    SOPHIA_WM_CAPABILITY_ACTIONS, SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT,
    SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS, Size, SurfaceConstraints, SurfaceId, TransactionId,
    WmActionId, WmChromePolicy, WmFocusRingStyle, WmFrameStyle, WmRgb8,
    valid_policy_interaction_payload,
};

use super::{
    IpcCodecError, PROJECTION_INDICATOR_RECORD_KIND, PROJECTION_OUTPUT_RECORD_KIND,
    PROJECTION_OUTPUT_STATUS_RECORD_KIND, PROJECTION_PLACEMENT_RECORD_KIND,
    SNAPSHOT_ACTION_RECORD_KIND, SNAPSHOT_OUTPUT_RECORD_KIND,
    SNAPSHOT_SESSION_OPERATION_RECORD_KIND, SNAPSHOT_SURFACE_RECORD_KIND,
    SOPHIA_WM_OUTCOME_COMMITTED, SOPHIA_WM_OUTCOME_DISCONNECTED,
    SOPHIA_WM_OUTCOME_REJECTED_INVALID, SOPHIA_WM_OUTCOME_REJECTED_STALE,
    SOPHIA_WM_OUTCOME_TIMED_OUT, WmV1PolicyConfiguration, WmV1PolicyDirty, WmV1ProjectionBegin,
    WmV1ProjectionChunk, WmV1ProjectionEnd, WmV1ProjectionIndicatorRecord, WmV1ProjectionOutcome,
    WmV1ProjectionOutputRecord, WmV1ProjectionOutputStatusRecord, WmV1ProjectionPlacementRecord,
    WmV1ProjectionRequest, WmV1SessionOperationOutcome, WmV1SessionOperationRequest,
    WmV1SnapshotActionRecord, WmV1SnapshotBegin, WmV1SnapshotChunk, WmV1SnapshotEnd,
    WmV1SnapshotOutputRecord, WmV1SnapshotSessionOperationRecord, WmV1SnapshotSurfaceRecord,
    decode_wm_v1_projection_indicator_records, decode_wm_v1_projection_output_records,
    decode_wm_v1_projection_output_status_records, decode_wm_v1_projection_placement_records,
    decode_wm_v1_snapshot_action_records, decode_wm_v1_snapshot_output_records,
    decode_wm_v1_snapshot_session_operation_records, decode_wm_v1_snapshot_surface_records,
    encode_wm_v1_projection_indicator_records, encode_wm_v1_projection_output_records,
    encode_wm_v1_projection_output_status_records, encode_wm_v1_projection_placement_records,
    encode_wm_v1_snapshot_action_records, encode_wm_v1_snapshot_output_records,
    encode_wm_v1_snapshot_session_operation_records, encode_wm_v1_snapshot_surface_records,
};

const OUTPUT_ID_WIRE_SIZE: usize = size_of::<u64>();

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
const POLICY_PRESENTATION_FULLSCREEN: u16 = 1 << 0;
const POLICY_PRESENTATION_MAXIMIZED: u16 = 1 << 1;
const POLICY_PRESENTATION_MINIMIZED: u16 = 1 << 2;
const POLICY_PRESENTATION_SUPPORTED: u16 =
    POLICY_PRESENTATION_FULLSCREEN | POLICY_PRESENTATION_MAXIMIZED | POLICY_PRESENTATION_MINIMIZED;
const POLICY_SESSION_OPERATION_SURFACE_TARGET: u16 = 1 << 0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmV1SnapshotTransfer {
    pub transaction: TransactionId,
    pub begin: WmV1SnapshotBegin,
    pub chunks: Vec<WmV1SnapshotChunk>,
    pub end: WmV1SnapshotEnd,
}

pub type WmV1DecodedSnapshot = super::PolicyDecodedSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmV1SnapshotSurfaceClassificationRecord {
    pub surface_index: u32,
    pub surface_generation: u32,
    pub classification: u64,
}

pub fn encode_wm_v1_snapshot_surface_classification_records(
    records: &[WmV1SnapshotSurfaceClassificationRecord],
) -> Result<Vec<u8>, IpcCodecError> {
    if records.len() > crate::POLICY_MAX_SURFACES {
        return Err(IpcCodecError::CountTooLarge {
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
) -> Result<Vec<WmV1SnapshotSurfaceClassificationRecord>, IpcCodecError> {
    let count = item_count as usize;
    if count > crate::POLICY_MAX_SURFACES {
        return Err(IpcCodecError::CountTooLarge {
            count,
            max: crate::POLICY_MAX_SURFACES,
        });
    }
    let expected = count
        .checked_mul(SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE)
        .ok_or(IpcCodecError::CountTooLarge {
            count,
            max: crate::POLICY_MAX_SURFACES,
        })?;
    if data.len() < expected {
        return Err(IpcCodecError::Truncated);
    }
    if data.len() > expected {
        return Err(IpcCodecError::TrailingBytes(data.len() - expected));
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmV1ProjectionTransfer {
    pub transaction: TransactionId,
    pub begin: WmV1ProjectionBegin,
    pub chunks: Vec<WmV1ProjectionChunk>,
    pub end: WmV1ProjectionEnd,
}

include!("wm_v1_records/control.rs");

include!("wm_v1_records/snapshot.rs");

include!("wm_v1_records/projection.rs");

fn encode_indicator_text(
    text: &str,
    field: &'static str,
) -> Result<(u16, [u8; 32]), IpcCodecError> {
    if text.is_empty() || text.len() > 32 || text.chars().any(char::is_control) {
        return Err(invalid(field, text.len() as u32));
    }
    let mut bytes = [0; 32];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    Ok((text.len() as u16, bytes))
}

fn decode_indicator_text(
    length: u16,
    bytes: &[u8; 32],
    field: &'static str,
) -> Result<String, IpcCodecError> {
    let length = usize::from(length);
    if length == 0 || length > bytes.len() || bytes[length..].iter().any(|byte| *byte != 0) {
        return Err(invalid(field, length as u32));
    }
    let text = core::str::from_utf8(&bytes[..length]).map_err(|_| invalid(field, length as u32))?;
    if text.chars().any(char::is_control) {
        return Err(invalid(field, length as u32));
    }
    Ok(text.to_owned())
}

fn push_policy_section(
    sections: &mut Vec<super::PolicyRecordSection>,
    record_kind: u16,
    count: usize,
    data: Vec<u8>,
) -> Result<(), IpcCodecError> {
    if count == 0 {
        return Ok(());
    }
    sections.push(super::PolicyRecordSection {
        kind: record_kind,
        count: u32::try_from(count).map_err(|_| IpcCodecError::CountTooLarge {
            count,
            max: u32::MAX as usize,
        })?,
        bytes: data,
    });
    Ok(())
}

fn push_projection_chunk(
    chunks: &mut Vec<WmV1ProjectionChunk>,
    connection_epoch: u64,
    record_kind: u16,
    count: usize,
    data: Vec<u8>,
) -> Result<(), IpcCodecError> {
    if count == 0 {
        return Ok(());
    }
    chunks.push(WmV1ProjectionChunk {
        connection_epoch,
        ordinal: chunks.len() as u16,
        record_kind,
        item_count: u32::try_from(count).map_err(|_| IpcCodecError::CountTooLarge {
            count,
            max: u32::MAX as usize,
        })?,
        data,
    });
    Ok(())
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
) -> Result<PolicySurfaceSnapshot, IpcCodecError> {
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

fn encode_placement_record(placement: &PolicySurfacePlacement) -> WmV1ProjectionPlacementRecord {
    let (requested_width, requested_height) = encode_optional_size(placement.requested_size);
    let crop = placement.crop.unwrap_or_default();
    WmV1ProjectionPlacementRecord {
        surface_index: placement.surface.index(),
        surface_generation: placement.surface.generation(),
        state_generation: placement.surface_generation,
        x: placement.geometry.x,
        y: placement.geometry.y,
        width: placement.geometry.width,
        height: placement.geometry.height,
        requested_width,
        requested_height,
        crop_x: crop.x,
        crop_y: crop.y,
        crop_width: crop.width,
        crop_height: crop.height,
        transform: placement.transform as u16,
        presentation_bits: encode_presentation(placement.presentation),
    }
}

fn decode_placement_record(
    record: WmV1ProjectionPlacementRecord,
) -> Result<PolicySurfacePlacement, IpcCodecError> {
    let crop = if record.crop_width == 0 && record.crop_height == 0 {
        if record.crop_x != 0 || record.crop_y != 0 {
            return Err(invalid("crop", 0));
        }
        None
    } else if record.crop_width > 0 && record.crop_height > 0 {
        Some(Rect {
            x: record.crop_x,
            y: record.crop_y,
            width: record.crop_width,
            height: record.crop_height,
        })
    } else {
        return Err(invalid("crop", 0));
    };
    Ok(PolicySurfacePlacement {
        surface: SurfaceId::new(record.surface_index, record.surface_generation),
        surface_generation: record.state_generation,
        geometry: Rect {
            x: record.x,
            y: record.y,
            width: record.width,
            height: record.height,
        },
        requested_size: decode_optional_size(
            record.requested_width,
            record.requested_height,
            "requested_size",
        )?,
        crop,
        transform: match record.transform {
            1 => PolicyTransform::Identity,
            other => return Err(invalid("policy_transform", u32::from(other))),
        },
        presentation: decode_presentation(record.presentation_bits, "presentation")?,
    })
}

fn encode_presentation(state: PolicyPresentationState) -> u16 {
    (u16::from(state.fullscreen) * POLICY_PRESENTATION_FULLSCREEN)
        | (u16::from(state.maximized) * POLICY_PRESENTATION_MAXIMIZED)
        | (u16::from(state.minimized) * POLICY_PRESENTATION_MINIMIZED)
}

fn encode_rgb(color: WmRgb8) -> u32 {
    0xff00_0000 | u32::from(color.red) << 16 | u32::from(color.green) << 8 | u32::from(color.blue)
}

fn decode_rgb(value: u32, field: &'static str) -> Result<WmRgb8, IpcCodecError> {
    if value >> 24 != 0xff {
        return Err(invalid(field, value));
    }
    Ok(WmRgb8 {
        red: (value >> 16) as u8,
        green: (value >> 8) as u8,
        blue: value as u8,
    })
}

fn encode_outcome(outcome: PolicyProjectionOutcome) -> u16 {
    match outcome {
        PolicyProjectionOutcome::Committed => SOPHIA_WM_OUTCOME_COMMITTED,
        PolicyProjectionOutcome::RejectedStale => SOPHIA_WM_OUTCOME_REJECTED_STALE,
        PolicyProjectionOutcome::RejectedInvalid => SOPHIA_WM_OUTCOME_REJECTED_INVALID,
        PolicyProjectionOutcome::TimedOut => SOPHIA_WM_OUTCOME_TIMED_OUT,
        PolicyProjectionOutcome::Disconnected => SOPHIA_WM_OUTCOME_DISCONNECTED,
    }
}

fn decode_outcome(outcome: u16) -> Result<PolicyProjectionOutcome, IpcCodecError> {
    match outcome {
        SOPHIA_WM_OUTCOME_COMMITTED => Ok(PolicyProjectionOutcome::Committed),
        SOPHIA_WM_OUTCOME_REJECTED_STALE => Ok(PolicyProjectionOutcome::RejectedStale),
        SOPHIA_WM_OUTCOME_REJECTED_INVALID => Ok(PolicyProjectionOutcome::RejectedInvalid),
        SOPHIA_WM_OUTCOME_TIMED_OUT => Ok(PolicyProjectionOutcome::TimedOut),
        SOPHIA_WM_OUTCOME_DISCONNECTED => Ok(PolicyProjectionOutcome::Disconnected),
        other => Err(invalid("policy_outcome", u32::from(other))),
    }
}

fn encode_output_ids(outputs: &[OutputId]) -> Result<Vec<u8>, IpcCodecError> {
    if outputs.is_empty() || outputs.len() > crate::POLICY_MAX_OUTPUTS {
        return Err(IpcCodecError::CountTooLarge {
            count: outputs.len(),
            max: crate::POLICY_MAX_OUTPUTS,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut encoded = Vec::with_capacity(outputs.len() * OUTPUT_ID_WIRE_SIZE);
    for output in outputs {
        if !output.is_valid() || !seen.insert(*output) {
            return Err(invalid("affected_output", output.raw() as u32));
        }
        encoded.extend_from_slice(&output.raw().to_le_bytes());
    }
    Ok(encoded)
}

fn decode_output_ids(count: u16, bytes: &[u8]) -> Result<Vec<OutputId>, IpcCodecError> {
    let count = usize::from(count);
    if count == 0 || count > crate::POLICY_MAX_OUTPUTS || bytes.len() != count * OUTPUT_ID_WIRE_SIZE
    {
        return Err(invalid("affected_output_bytes", bytes.len() as u32));
    }
    let mut seen = std::collections::BTreeSet::new();
    bytes
        .chunks_exact(OUTPUT_ID_WIRE_SIZE)
        .map(|bytes| {
            let output = OutputId::from_raw(u64::from_le_bytes(
                bytes.try_into().expect("fixed output-id chunk"),
            ));
            if !output.is_valid() || !seen.insert(output) {
                return Err(invalid("affected_output", output.raw() as u32));
            }
            Ok(output)
        })
        .collect()
}

fn decode_presentation(
    bits: u16,
    field: &'static str,
) -> Result<PolicyPresentationState, IpcCodecError> {
    if bits & !POLICY_PRESENTATION_SUPPORTED != 0 {
        return Err(invalid(field, u32::from(bits)));
    }
    Ok(PolicyPresentationState {
        fullscreen: bits & POLICY_PRESENTATION_FULLSCREEN != 0,
        maximized: bits & POLICY_PRESENTATION_MAXIMIZED != 0,
        minimized: bits & POLICY_PRESENTATION_MINIMIZED != 0,
    })
}

fn encode_optional_size(size: Option<Size>) -> (i32, i32) {
    size.map(|size| (size.width, size.height)).unwrap_or((0, 0))
}

fn decode_optional_size(
    width: i32,
    height: i32,
    field: &'static str,
) -> Result<Option<Size>, IpcCodecError> {
    if width == 0 && height == 0 {
        Ok(None)
    } else if width > 0 && height > 0 {
        Ok(Some(Size { width, height }))
    } else {
        Err(invalid(field, 0))
    }
}

fn decode_optional_surface(
    index: u32,
    generation: u32,
    field: &'static str,
) -> Result<Option<SurfaceId>, IpcCodecError> {
    if index == 0 && generation == 0 {
        Ok(None)
    } else if generation != 0 {
        Ok(Some(SurfaceId::new(index, generation)))
    } else {
        Err(invalid(field, index))
    }
}

fn require_count(actual: usize, expected: usize) -> Result<(), IpcCodecError> {
    if actual == expected {
        Ok(())
    } else {
        Err(invalid("record_count", actual as u32))
    }
}

fn invalid(field: &'static str, value: u32) -> IpcCodecError {
    IpcCodecError::InvalidEnum { field, value }
}

include!("wm_v1_records/snapshot_records.rs");

include!("wm_v1_records/projection_records.rs");

include!("wm_v1_records/control_records.rs");
