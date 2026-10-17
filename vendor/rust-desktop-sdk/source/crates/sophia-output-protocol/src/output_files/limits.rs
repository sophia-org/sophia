use super::{OUTPUT_FILE_MAX_CANDIDATE_BYTES, invalid, reserved};
use crate::byte_cursor::{Cursor, push_u16, push_u32};
use crate::{
    BinaryCodecError, MAX_OUTPUT_AUTHORITY_GROUPS, MAX_OUTPUT_AUTHORITY_HEADS,
    MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP, MAX_OUTPUT_AUTHORITY_LABEL_BYTES,
    MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD, SOPHIA_OUTPUT_INTERFACE_REVISION,
};

pub const OUTPUT_FILE_MAX_JOURNAL_RECORDS: u32 = 64;
pub const OUTPUT_FILE_MAX_JOURNAL_BYTES: u32 = 16_384;
pub const OUTPUT_FILE_MAX_DOMAIN_TRANSACTIONS: u32 = 4_096;
pub const OUTPUT_FILE_ASSEMBLY_TIMEOUT_MILLIS: u32 = 12_000;
pub const OUTPUT_FILE_ACK_TIMEOUT_MILLIS: u32 = 2_000;

/// Advertised per-epoch resources. Domain replay history is separate from
/// journal retention: acknowledgements cannot make a transaction reusable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFileLimits {
    pub journal_records: u32,
    pub journal_bytes: u32,
    pub staging_bytes: u32,
    pub assembly_timeout_millis: u32,
    pub ack_progress_timeout_millis: u32,
    pub max_domain_transactions: u32,
}

impl Default for OutputFileLimits {
    fn default() -> Self {
        Self {
            journal_records: OUTPUT_FILE_MAX_JOURNAL_RECORDS,
            journal_bytes: OUTPUT_FILE_MAX_JOURNAL_BYTES,
            staging_bytes: OUTPUT_FILE_MAX_CANDIDATE_BYTES as u32,
            assembly_timeout_millis: OUTPUT_FILE_ASSEMBLY_TIMEOUT_MILLIS,
            ack_progress_timeout_millis: OUTPUT_FILE_ACK_TIMEOUT_MILLIS,
            max_domain_transactions: OUTPUT_FILE_MAX_DOMAIN_TRANSACTIONS,
        }
    }
}

impl OutputFileLimits {
    pub fn validate(self) -> Result<(), BinaryCodecError> {
        if !(8..=OUTPUT_FILE_MAX_JOURNAL_RECORDS).contains(&self.journal_records)
            || !(2_048..=OUTPUT_FILE_MAX_JOURNAL_BYTES).contains(&self.journal_bytes)
            || self.staging_bytes != OUTPUT_FILE_MAX_CANDIDATE_BYTES as u32
            || !(1..=OUTPUT_FILE_ASSEMBLY_TIMEOUT_MILLIS).contains(&self.assembly_timeout_millis)
            || !(1..=OUTPUT_FILE_ACK_TIMEOUT_MILLIS).contains(&self.ack_progress_timeout_millis)
            || !(1..=OUTPUT_FILE_MAX_DOMAIN_TRANSACTIONS).contains(&self.max_domain_transactions)
        {
            return Err(invalid("output_limits"));
        }
        Ok(())
    }
}

const FIXED: [u16; 7] = [
    SOPHIA_OUTPUT_INTERFACE_REVISION,
    MAX_OUTPUT_AUTHORITY_HEADS as u16,
    MAX_OUTPUT_AUTHORITY_GROUPS as u16,
    MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD as u16,
    MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP as u16,
    MAX_OUTPUT_AUTHORITY_LABEL_BYTES as u16,
    (MAX_OUTPUT_AUTHORITY_HEADS * MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD) as u16,
];

pub fn decode_output_file_limits(bytes: &[u8]) -> Result<OutputFileLimits, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    for expected in FIXED {
        if cursor.u16()? != expected {
            return Err(invalid("output_fixed_limit"));
        }
    }
    reserved(&mut cursor, 2)?;
    let limits = OutputFileLimits {
        journal_records: cursor.u32()?,
        journal_bytes: cursor.u32()?,
        staging_bytes: cursor.u32()?,
        assembly_timeout_millis: cursor.u32()?,
        ack_progress_timeout_millis: cursor.u32()?,
        max_domain_transactions: cursor.u32()?,
    };
    cursor.finish()?;
    limits.validate()?;
    Ok(limits)
}

pub fn encode_output_file_limits(limits: OutputFileLimits) -> Result<Vec<u8>, BinaryCodecError> {
    limits.validate()?;
    let mut bytes = Vec::with_capacity(40);
    for value in FIXED {
        push_u16(&mut bytes, value);
    }
    push_u16(&mut bytes, 0);
    for value in [
        limits.journal_records,
        limits.journal_bytes,
        limits.staging_bytes,
        limits.assembly_timeout_millis,
        limits.ack_progress_timeout_millis,
        limits.max_domain_transactions,
    ] {
        push_u32(&mut bytes, value);
    }
    Ok(bytes)
}
