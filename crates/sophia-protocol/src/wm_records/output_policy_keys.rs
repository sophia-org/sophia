//! Opaque configured output affinity. The key is policy-owned; Sophia only
//! checks that it names a current output generation exactly once.
use std::collections::BTreeSet;

use super::{PolicyRecordSection, PolicyRecordSectionRef};
use crate::wm_rows::SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS;
use crate::{BinaryCodecError, PolicyOutputSnapshot};

pub const SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND: u16 = 0xff07;

// The label predates the split from output actions; errors keep it.
fn invalid() -> BinaryCodecError {
    BinaryCodecError::InvalidEnum {
        field: "output_action_or_policy_key",
        value: 0,
    }
}

pub fn encode_policy_output_key_records(
    outputs: &[PolicyOutputSnapshot],
    capabilities: u64,
) -> Result<Vec<PolicyRecordSection>, BinaryCodecError> {
    if capabilities & SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS == 0 {
        return Ok(Vec::new());
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
    Ok(if data.is_empty() {
        Vec::new()
    } else {
        vec![PolicyRecordSection {
            kind: SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND,
            count: keys.len() as u32,
            bytes: data,
        }]
    })
}

pub fn apply_policy_output_key_records(
    sections: &[PolicyRecordSectionRef<'_>],
    outputs: &mut [PolicyOutputSnapshot],
) -> Result<(), BinaryCodecError> {
    let mut keys = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for chunk in sections
        .iter()
        .filter(|c| c.kind == SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND)
    {
        if chunk.count == 0
            || chunk.count as usize > crate::POLICY_MAX_OUTPUTS
            || chunk.bytes.len() != chunk.count as usize * 24
        {
            return Err(invalid());
        }
        for record in chunk.bytes.chunks_exact(24) {
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
