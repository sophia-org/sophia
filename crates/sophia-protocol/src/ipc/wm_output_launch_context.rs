//! Complete, capability-gated output bookmarks. No workspace identity crosses
//! this boundary; the issuing WM alone interprets the token.
use super::{IpcCodecError, WmV1ProjectionChunk};
use crate::{OutputId, POLICY_MAX_OUTPUTS, PolicyOutputLaunchContext};
use std::collections::BTreeSet;

pub const PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND: u16 = 0xff08;
pub const OUTPUT_LAUNCH_CONTEXT_RECORD_LEN: usize = 32;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "output_launch_context",
        value: 0,
    }
}

pub fn encode_wm_output_launch_contexts(
    records: &[PolicyOutputLaunchContext],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    Ok(
        encode_policy_output_launch_contexts_records(records, epoch)?
            .into_iter()
            .map(|s| WmV1ProjectionChunk {
                connection_epoch: epoch,
                ordinal,
                record_kind: s.kind,
                item_count: s.count,
                data: s.bytes,
            })
            .collect(),
    )
}

pub fn encode_policy_output_launch_contexts_records(
    records: &[PolicyOutputLaunchContext],
    epoch: u64,
) -> Result<Vec<super::PolicyRecordSection>, IpcCodecError> {
    if records.len() > POLICY_MAX_OUTPUTS {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    let mut data = Vec::with_capacity(records.len() * OUTPUT_LAUNCH_CONTEXT_RECORD_LEN);
    for r in records {
        if !r.output.is_valid()
            || r.output_generation == 0
            || r.epoch == 0
            || r.epoch != epoch
            || r.token == 0
            || !seen.insert(r.output)
        {
            return Err(invalid());
        }
        for value in [r.output.raw(), r.output_generation, r.epoch, r.token] {
            data.extend(value.to_le_bytes());
        }
    }
    Ok(if records.is_empty() {
        Vec::new()
    } else {
        vec![super::PolicyRecordSection {
            kind: PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
            count: records.len() as u32,
            bytes: data,
        }]
    })
}

pub fn decode_wm_output_launch_contexts(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyOutputLaunchContext>, IpcCodecError> {
    let mut epoch = None;
    for c in chunks
        .iter()
        .filter(|c| c.record_kind == PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND)
    {
        if epoch.is_some_and(|e| e != c.connection_epoch) {
            return Err(invalid());
        }
        epoch = Some(c.connection_epoch);
    }
    decode_policy_output_launch_contexts_records(
        epoch.unwrap_or(0),
        &super::wm_record_sections::projection_sections(chunks),
    )
}

pub fn decode_policy_output_launch_contexts_records(
    epoch: u64,
    sections: &[super::PolicyRecordSectionRef<'_>],
) -> Result<Vec<PolicyOutputLaunchContext>, IpcCodecError> {
    let mut records = Vec::new();
    for c in sections
        .iter()
        .filter(|c| c.kind == PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND)
    {
        if c.count == 0
            || c.count as usize > POLICY_MAX_OUTPUTS.saturating_sub(records.len())
            || c.bytes.len() != c.count as usize * OUTPUT_LAUNCH_CONTEXT_RECORD_LEN
        {
            return Err(invalid());
        }
        for b in c.bytes.chunks_exact(OUTPUT_LAUNCH_CONTEXT_RECORD_LEN) {
            let read = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
            records.push(PolicyOutputLaunchContext {
                output: OutputId::from_raw(read(0)),
                output_generation: read(8),
                epoch: read(16),
                token: read(24),
            });
        }
    }
    encode_policy_output_launch_contexts_records(&records, epoch)?;
    Ok(records)
}
