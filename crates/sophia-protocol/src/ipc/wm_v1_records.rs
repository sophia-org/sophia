//! Legacy `sophia_wm_v1` socket adapters. Only this layer knows scalar
//! messages, Begin/Chunk/End transfers, chunk ordinals and the counts a Begin
//! declares. The rows and their semantics belong to `crate::wm_records`.
use core::mem::size_of;

use crate::wm_records::{
    PROJECTION_LAUNCH_CONTEXT_RECORD_KIND, PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
    PROJECTION_PRESENTATION_BINDING_RECORD_KIND, PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND,
    PROJECTION_PRESENTATION_RECORD_KIND, PROJECTION_PRESENTATION_REGION_RECORD_KIND,
    PROJECTION_SURFACE_INSTANCE_RECORD_KIND, PROJECTION_TAB_GROUP_RECORD_KIND,
    PROJECTION_TAB_GROUP_RECORD_LEN, PROJECTION_TAB_MEMBER_RECORD_KIND,
    PROJECTION_TAB_MEMBER_RECORD_LEN, PROJECTION_TRANSLATION_GROUP_RECORD_KIND,
    PROJECTION_TRANSLATION_GROUP_RECORD_LEN, PROJECTION_TRANSLATION_MEMBER_RECORD_KIND,
    PROJECTION_TRANSLATION_MEMBER_RECORD_LEN, PolicyDecodedSnapshot, PolicyProjectionMetadata,
    PolicyRecordSectionRef, PolicySnapshotMetadata, SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND,
    SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
    decode_optional_surface, decode_policy_action_rows, decode_projection_sections,
    decode_snapshot_sections, encode_policy_configuration_records,
    encode_policy_projection_records, encode_policy_snapshot_records, require_count,
    validate_policy_configuration, wm_presentation_record_layout,
};
use crate::{
    OutputId, PolicyActionRegistration, PolicyConfiguration, PolicyDirtyRequest,
    PolicyProjectionOutcome, PolicyProjectionProposal, PolicyRequestCause, PolicySceneSnapshot,
    PolicySessionOperationOutcome, PolicySessionOperationRequest, PolicySurfaceClassification,
    Rect, TransactionId, WmActionId, WmChromePolicy, WmFocusRingStyle, WmFrameStyle, WmRgb8,
    valid_policy_interaction_payload,
};

use super::{
    IpcCodecError, PROJECTION_INDICATOR_RECORD_KIND, PROJECTION_OUTPUT_RECORD_KIND,
    PROJECTION_OUTPUT_STATUS_RECORD_KIND, PROJECTION_PLACEMENT_RECORD_KIND,
    SNAPSHOT_ACTION_RECORD_KIND, SNAPSHOT_OUTPUT_RECORD_KIND,
    SNAPSHOT_SESSION_OPERATION_RECORD_KIND, SNAPSHOT_SURFACE_RECORD_KIND, WmV1PolicyConfiguration,
    WmV1PolicyDirty, WmV1ProjectionBegin, WmV1ProjectionChunk, WmV1ProjectionEnd,
    WmV1ProjectionOutcome, WmV1ProjectionRequest, WmV1SessionOperationOutcome,
    WmV1SessionOperationRequest, WmV1SnapshotBegin, WmV1SnapshotChunk, WmV1SnapshotEnd,
    decode_wm_v1_snapshot_action_records,
};

const OUTPUT_ID_WIRE_SIZE: usize = size_of::<u64>();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmV1SnapshotTransfer {
    pub transaction: TransactionId,
    pub begin: WmV1SnapshotBegin,
    pub chunks: Vec<WmV1SnapshotChunk>,
    pub end: WmV1SnapshotEnd,
}

pub type WmV1DecodedSnapshot = PolicyDecodedSnapshot;

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
        ordinal: legacy_record_count_u16(chunks.len())?,
        record_kind,
        item_count: u32::try_from(count).map_err(|_| IpcCodecError::CountTooLarge {
            count,
            max: u32::MAX as usize,
        })?,
        data,
    });
    Ok(())
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
    super::policy_projection_outcome_code(outcome)
}

fn decode_outcome(outcome: u16) -> Result<PolicyProjectionOutcome, IpcCodecError> {
    super::policy_projection_outcome_from_code(outcome)
        .ok_or_else(|| invalid("policy_outcome", u32::from(outcome)))
}

fn encode_output_ids(outputs: &[OutputId]) -> Result<Vec<u8>, IpcCodecError> {
    super::validate_policy_affected_outputs(outputs)?;
    let mut encoded = Vec::with_capacity(outputs.len() * OUTPUT_ID_WIRE_SIZE);
    for output in outputs {
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
    let outputs = bytes
        .chunks_exact(OUTPUT_ID_WIRE_SIZE)
        .map(|bytes| {
            OutputId::from_raw(u64::from_le_bytes(
                bytes.try_into().expect("fixed output-id chunk"),
            ))
        })
        .collect::<Vec<_>>();
    super::validate_policy_affected_outputs(&outputs)?;
    Ok(outputs)
}

fn invalid(field: &'static str, value: u32) -> IpcCodecError {
    IpcCodecError::InvalidEnum { field, value }
}

fn legacy_record_count_u16(count: usize) -> Result<u16, IpcCodecError> {
    u16::try_from(count).map_err(|_| IpcCodecError::CountTooLarge {
        count,
        max: u16::MAX as usize,
    })
}
