//! Opaque placement bookmarks use the existing uncounted extension envelope.
use std::collections::BTreeSet;

use super::{IpcCodecError, WmV1ProjectionChunk, WmV1SnapshotChunk, WmV1SnapshotTransfer};
use crate::{POLICY_MAX_SURFACES, PolicyLaunchContext, SurfaceId};

pub const PROJECTION_LAUNCH_CONTEXT_RECORD_KIND: u16 = 0xff05;
pub const SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND: u16 = 0xff06;
pub const LAUNCH_CONTEXT_RECORD_LEN: usize = 24;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "launch_origin",
        value: 0,
    }
}

pub fn encode_wm_launch_context_records(
    records: &[PolicyLaunchContext],
) -> Result<Vec<u8>, IpcCodecError> {
    if records.len() > POLICY_MAX_SURFACES {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    let mut bytes = Vec::with_capacity(records.len() * LAUNCH_CONTEXT_RECORD_LEN);
    for record in records {
        if !record.surface.is_valid()
            || record.epoch == 0
            || record.token == 0
            || !seen.insert(record.surface)
        {
            return Err(invalid());
        }
        bytes.extend(record.surface.index().to_le_bytes());
        bytes.extend(record.surface.generation().to_le_bytes());
        bytes.extend(record.epoch.to_le_bytes());
        bytes.extend(record.token.to_le_bytes());
    }
    Ok(bytes)
}

pub fn decode_wm_launch_context_records(
    bytes: &[u8],
    count: u32,
) -> Result<Vec<PolicyLaunchContext>, IpcCodecError> {
    if count as usize > POLICY_MAX_SURFACES
        || bytes.len() != count as usize * LAUNCH_CONTEXT_RECORD_LEN
    {
        return Err(invalid());
    }
    let mut records = Vec::with_capacity(count as usize);
    for b in bytes.chunks_exact(LAUNCH_CONTEXT_RECORD_LEN) {
        records.push(PolicyLaunchContext {
            surface: SurfaceId::new(
                u32::from_le_bytes(b[0..4].try_into().unwrap()),
                u32::from_le_bytes(b[4..8].try_into().unwrap()),
            ),
            epoch: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            token: u64::from_le_bytes(b[16..24].try_into().unwrap()),
        });
    }
    // Apply the same identity, duplicate and zero checks in either direction.
    encode_wm_launch_context_records(&records)?;
    Ok(records)
}

pub fn encode_wm_launch_contexts(
    records: &[PolicyLaunchContext],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    Ok(encode_policy_launch_contexts_records(records, epoch)?
        .into_iter()
        .map(|section| WmV1ProjectionChunk {
            connection_epoch: epoch,
            ordinal,
            record_kind: section.kind,
            item_count: section.count,
            data: section.bytes,
        })
        .collect())
}

pub fn decode_wm_launch_contexts(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyLaunchContext>, IpcCodecError> {
    let mut records = Vec::new();
    for chunk in chunks
        .iter()
        .filter(|c| c.record_kind == PROJECTION_LAUNCH_CONTEXT_RECORD_KIND)
    {
        records.extend(decode_policy_launch_contexts_records(
            chunk.connection_epoch,
            &[super::PolicyRecordSectionRef {
                kind: chunk.record_kind,
                count: chunk.item_count,
                bytes: &chunk.data,
            }],
        )?);
    }
    encode_wm_launch_context_records(&records)?;
    Ok(records)
}

/// Preserve the frozen counted prefix; origin records are only sent to a peer
/// which selected the capability. A previous-epoch bookmark is not replayed.
pub fn append_wm_launch_origins(
    transfer: &mut WmV1SnapshotTransfer,
    records: &[PolicyLaunchContext],
    capabilities: u64,
) -> Result<(), IpcCodecError> {
    if capabilities & super::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN == 0 || records.is_empty() {
        return Ok(());
    }
    if records
        .iter()
        .any(|r| r.epoch != transfer.begin.connection_epoch)
    {
        return Err(invalid());
    }
    transfer.chunks.push(WmV1SnapshotChunk {
        connection_epoch: transfer.begin.connection_epoch,
        ordinal: u16::try_from(transfer.chunks.len()).map_err(|_| invalid())?,
        record_kind: SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND,
        item_count: records.len() as u32,
        data: encode_wm_launch_context_records(records)?,
    });
    Ok(())
}

/// Complete sections carry the epoch in metadata, never an IPC chunk envelope.
pub fn decode_policy_launch_contexts_records(
    epoch: u64,
    sections: &[super::PolicyRecordSectionRef<'_>],
) -> Result<Vec<PolicyLaunchContext>, IpcCodecError> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|s| s.kind == PROJECTION_LAUNCH_CONTEXT_RECORD_KIND)
    {
        if section.count == 0 {
            return Err(invalid());
        }
        records.extend(decode_wm_launch_context_records(
            section.bytes,
            section.count,
        )?);
    }
    if records.iter().any(|r| r.epoch != epoch) {
        return Err(invalid());
    }
    encode_wm_launch_context_records(&records)?;
    Ok(records)
}

pub fn encode_policy_launch_contexts_records(
    records: &[PolicyLaunchContext],
    epoch: u64,
) -> Result<Vec<super::PolicyRecordSection>, IpcCodecError> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    if records.iter().any(|r| r.epoch != epoch) {
        return Err(invalid());
    }
    Ok(vec![super::PolicyRecordSection {
        kind: PROJECTION_LAUNCH_CONTEXT_RECORD_KIND,
        count: records.len() as u32,
        bytes: encode_wm_launch_context_records(records)?,
    }])
}
