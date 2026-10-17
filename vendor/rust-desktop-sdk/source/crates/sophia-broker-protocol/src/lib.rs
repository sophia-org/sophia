//! Passive records for the broker file API. No transport or authority.
mod controls;
mod events;
pub use controls::{encode_ack, encode_negotiate, encode_submit};
mod responses;
pub use events::{Event, EventRecord, Request, RequestKind, decode_event};
pub use responses::{ResponseRow, RowKind, encode_response};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Invalid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_surfaces: u32,
    pub max_label_bytes: u16,
    pub journal_records: u32,
    pub journal_bytes: u32,
    pub staging_bytes: u32,
    pub response_timeout_ms: u32,
    pub ack_progress_timeout_ms: u32,
    pub assembly_timeout_ms: u32,
}

pub fn decode_api(bytes: &[u8]) -> Result<u64, Invalid> {
    let digits = bytes
        .strip_prefix(b"sophia-broker-files version=1 role=metadata-broker epoch=")
        .and_then(|b| b.strip_suffix(b" fd_transfer=none\n"))
        .ok_or(Invalid)?;
    if digits.is_empty() || digits[0] == b'0' {
        return Err(Invalid);
    }
    digits.iter().try_fold(0u64, |v, b| {
        if !b.is_ascii_digit() {
            return Err(Invalid);
        }
        v.checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(b - b'0')))
            .ok_or(Invalid)
    })
}

pub fn decode_limits(bytes: &[u8], epoch: u64) -> Result<Limits, Invalid> {
    if bytes.len() != 72 || epoch == 0 {
        return Err(Invalid);
    }
    let u16_at = |i| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    if u32_at(0) != 72
        || u16_at(4) != 1
        || u16_at(6) != 1
        || bytes[8..16] != epoch.to_le_bytes()
        || bytes[16..32] != [0; 16]
        || u16_at(32) != 2
        || u16_at(34) != 0
        || u16_at(42) != 2
        || u32_at(68) != 0
    {
        return Err(Invalid);
    }
    let limits = Limits {
        max_surfaces: u32_at(36),
        max_label_bytes: u16_at(40),
        journal_records: u32_at(44),
        journal_bytes: u32_at(48),
        staging_bytes: u32_at(52),
        response_timeout_ms: u32_at(56),
        ack_progress_timeout_ms: u32_at(60),
        assembly_timeout_ms: u32_at(64),
    };
    if !(1..=1024).contains(&limits.max_surfaces)
        || !(1..=128).contains(&limits.max_label_bytes)
        || !(4..=16).contains(&limits.journal_records)
        || !(352..=4096).contains(&limits.journal_bytes)
        || !(448..=512).contains(&limits.staging_bytes)
        || !(1..=5000).contains(&limits.response_timeout_ms)
        || !(1..=2000).contains(&limits.ack_progress_timeout_ms)
        || !(1..=12000).contains(&limits.assembly_timeout_ms)
    {
        return Err(Invalid);
    }
    Ok(limits)
}
