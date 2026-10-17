//! Native metadata-broker file records, defined by
//! `protocol/sophia-broker-files-v1.kdl`. These values confer no admission.
//! The export owns custody, request correlation and atomic effect preparation.
use crate::BinaryCodecError;
mod request;
mod response;
pub use request::BrokerFileRequest;
pub use response::{BrokerFileResponseRow, BrokerFileResponseSet, decode_broker_file_response};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokerFileLimits {
    pub max_surfaces: u32,
    pub max_label_bytes: u16,
    pub journal_records: u32,
    pub journal_bytes: u32,
    pub staging_bytes: u32,
    pub response_timeout_ms: u32,
    pub ack_progress_timeout_ms: u32,
    pub assembly_timeout_ms: u32,
}
impl Default for BrokerFileLimits {
    fn default() -> Self {
        Self {
            max_surfaces: 1024,
            max_label_bytes: 128,
            journal_records: 16,
            journal_bytes: 4096,
            staging_bytes: 512,
            response_timeout_ms: 5000,
            ack_progress_timeout_ms: 2000,
            assembly_timeout_ms: 12000,
        }
    }
}
fn invalid() -> BinaryCodecError {
    BinaryCodecError::InvalidRecord("broker file record")
}
impl BrokerFileLimits {
    pub fn validate(self) -> Result<(), BinaryCodecError> {
        if !(1..=1024).contains(&self.max_surfaces)
            || !(1..=128).contains(&self.max_label_bytes)
            || !(4..=16).contains(&self.journal_records)
            || !(352..=4096).contains(&self.journal_bytes)
            || !(448..=512).contains(&self.staging_bytes)
            || !(1..=5000).contains(&self.response_timeout_ms)
            || !(1..=2000).contains(&self.ack_progress_timeout_ms)
            || !(1..=12000).contains(&self.assembly_timeout_ms)
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn encode(self, epoch: u64) -> Result<[u8; 72], BinaryCodecError> {
        self.validate()?;
        let mut bytes = header::<72>(1, epoch, 0, 0)?;
        put(&mut bytes, 32, 2, 2);
        put(&mut bytes, 36, u64::from(self.max_surfaces), 4);
        put(&mut bytes, 40, u64::from(self.max_label_bytes), 2);
        put(&mut bytes, 42, 2, 2);
        for (at, value) in [
            (44, self.journal_records),
            (48, self.journal_bytes),
            (52, self.staging_bytes),
            (56, self.response_timeout_ms),
            (60, self.ack_progress_timeout_ms),
            (64, self.assembly_timeout_ms),
        ] {
            put(&mut bytes, at, u64::from(value), 4);
        }
        Ok(bytes)
    }
    pub fn negotiated(self, epoch: u64, sequence: u64) -> Result<[u8; 56], BinaryCodecError> {
        self.validate()?;
        let mut bytes = header::<56>(16, epoch, 0, sequence)?;
        put(&mut bytes, 32, 2, 2);
        put(&mut bytes, 36, u64::from(self.max_surfaces), 4);
        put(&mut bytes, 40, epoch, 8);
        put(&mut bytes, 48, u64::from(self.max_label_bytes), 2);
        put(&mut bytes, 50, 2, 2);
        Ok(bytes)
    }
}
fn put(bytes: &mut [u8], at: usize, value: u64, width: usize) {
    bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
}
fn get(bytes: &[u8], at: usize, width: usize) -> u64 {
    bytes[at..at + width]
        .iter()
        .enumerate()
        .fold(0, |n, (i, b)| n | (u64::from(*b) << (i * 8)))
}
fn header<const N: usize>(
    kind: u16,
    epoch: u64,
    id: u64,
    sequence: u64,
) -> Result<[u8; N], BinaryCodecError> {
    let valid = match kind {
        1 => N == 72 && id == 0 && sequence == 0,
        16 => N == 56 && id == 0 && sequence > 0,
        17 => N == 40 && id == 0 && sequence > 0,
        18 => N == 48 && id == 0 && sequence > 0,
        32 => N == 200 && id == 0 && sequence > 0,
        _ => false,
    };
    if epoch == 0 || !valid {
        return Err(invalid());
    }
    let mut bytes = [0; N];
    put(&mut bytes, 0, N as u64, 4);
    put(&mut bytes, 4, 1, 2);
    put(&mut bytes, 6, u64::from(kind), 2);
    put(&mut bytes, 8, epoch, 8);
    put(&mut bytes, 16, id, 8);
    put(&mut bytes, 24, sequence, 8);
    Ok(bytes)
}
pub fn encode_broker_file_refused(epoch: u64, sequence: u64) -> Result<[u8; 40], BinaryCodecError> {
    let mut bytes = header::<40>(17, epoch, 0, sequence)?;
    put(&mut bytes, 32, 1, 2);
    Ok(bytes)
}
pub fn encode_broker_file_submitted(
    epoch: u64,
    sequence: u64,
    id: u64,
    kind: u16,
) -> Result<[u8; 48], BinaryCodecError> {
    if id == 0 || !matches!(kind, 256 | 257) {
        return Err(invalid());
    }
    let mut bytes = header::<48>(18, epoch, 0, sequence)?;
    put(&mut bytes, 32, id, 8);
    put(&mut bytes, 40, u64::from(kind), 2);
    Ok(bytes)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokerFileNegotiate {
    pub epoch: u64,
    pub submission: u64,
    pub minimum: u16,
    pub maximum: u16,
}
/// Unsupported or inverted ranges are valid candidates that produce Refused.
pub fn decode_broker_file_negotiate(bytes: &[u8]) -> Result<BrokerFileNegotiate, BinaryCodecError> {
    if bytes.len() != 40
        || get(bytes, 0, 4) != 40
        || get(bytes, 4, 2) != 1
        || get(bytes, 6, 2) != 256
        || get(bytes, 8, 8) == 0
        || get(bytes, 16, 8) == 0
        || get(bytes, 24, 8) != 0
        || get(bytes, 36, 4) != 0
    {
        return Err(invalid());
    }
    Ok(BrokerFileNegotiate {
        epoch: get(bytes, 8, 8),
        submission: get(bytes, 16, 8),
        minimum: get(bytes, 32, 2) as u16,
        maximum: get(bytes, 34, 2) as u16,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokerFileSubmit {
    pub epoch: u64,
    pub submission: u64,
    pub length: u32,
}
/// Structural control validation only. The owner matches the staged candidate,
/// checks its entire body, and reserves effects and journal space before custody.
pub fn decode_broker_file_submit(bytes: &[u8]) -> Result<BrokerFileSubmit, BinaryCodecError> {
    if bytes.len() != 24
        || get(bytes, 0, 8) == 0
        || get(bytes, 8, 8) == 0
        || !(40..=448).contains(&get(bytes, 16, 4))
        || get(bytes, 20, 4) != 0
    {
        return Err(invalid());
    }
    Ok(BrokerFileSubmit {
        epoch: get(bytes, 0, 8),
        submission: get(bytes, 8, 8),
        length: get(bytes, 16, 4) as u32,
    })
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokerFileAck {
    pub epoch: u64,
    pub sequence: u64,
}
pub fn decode_broker_file_ack(bytes: &[u8]) -> Result<BrokerFileAck, BinaryCodecError> {
    if bytes.len() != 16 || get(bytes, 0, 8) == 0 || get(bytes, 8, 8) == 0 {
        return Err(invalid());
    }
    Ok(BrokerFileAck {
        epoch: get(bytes, 0, 8),
        sequence: get(bytes, 8, 8),
    })
}
