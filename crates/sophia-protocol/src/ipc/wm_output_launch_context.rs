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
        vec![WmV1ProjectionChunk {
            connection_epoch: epoch,
            ordinal,
            record_kind: PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
            item_count: records.len() as u32,
            data,
        }]
    })
}

pub fn decode_wm_output_launch_contexts(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyOutputLaunchContext>, IpcCodecError> {
    let mut records = Vec::new();
    let mut epoch = None;
    for c in chunks
        .iter()
        .filter(|c| c.record_kind == PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND)
    {
        if c.item_count == 0
            || c.item_count as usize > POLICY_MAX_OUTPUTS.saturating_sub(records.len())
            || c.data.len() != c.item_count as usize * OUTPUT_LAUNCH_CONTEXT_RECORD_LEN
            || epoch.is_some_and(|e| e != c.connection_epoch)
        {
            return Err(invalid());
        }
        epoch = Some(c.connection_epoch);
        for b in c.data.chunks_exact(OUTPUT_LAUNCH_CONTEXT_RECORD_LEN) {
            let read = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
            records.push(PolicyOutputLaunchContext {
                output: OutputId::from_raw(read(0)),
                output_generation: read(8),
                epoch: read(16),
                token: read(24),
            });
        }
    }
    encode_wm_output_launch_contexts(&records, epoch.unwrap_or(0), 0)?;
    Ok(records)
}
