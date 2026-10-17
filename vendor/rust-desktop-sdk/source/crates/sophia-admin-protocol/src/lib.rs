//! Passive records for the pinned administrative file contract.
//! Parsing grants no authority and does not extend a command deadline.

pub const LINE_MAX_BYTES: usize = 512;

mod invocation;
pub use invocation::*;
mod status;
pub use status::*;
mod catalog;
pub use catalog::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordError;

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid administrative file record")
    }
}
impl std::error::Error for RecordError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Info {
    pub session_low: u64,
    pub session_high: u64,
    pub connection: u64,
    pub command_ms: u32,
    pub frame_ms: u32,
    pub idle_ms: u32,
}

fn decimal(text: &str) -> Result<u64, RecordError> {
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(RecordError);
    }
    text.parse().map_err(|_| RecordError)
}

/// Decode one complete info snapshot, including its terminating LF.
/// Uses no heap storage; callers must establish EOF separately.
pub fn decode_info(bytes: &[u8]) -> Result<Info, RecordError> {
    if bytes.len() > LINE_MAX_BYTES {
        return Err(RecordError);
    }
    let line = std::str::from_utf8(bytes).map_err(|_| RecordError)?;
    let body = line.strip_suffix('\n').ok_or(RecordError)?;
    let mut fields = body.split(' ');
    if fields.next() != Some("v1") || fields.next() != Some("info") {
        return Err(RecordError);
    }
    let mut words = [0; 6];
    for word in &mut words {
        *word = decimal(fields.next().ok_or(RecordError)?)?;
    }
    if fields.next().is_some()
        || words[0] | words[1] == 0
        || words[2] == 0
        || words[3..].iter().any(|&v| v == 0 || v > u32::MAX as u64)
    {
        return Err(RecordError);
    }
    Ok(Info {
        session_low: words[0],
        session_high: words[1],
        connection: words[2],
        command_ms: words[3] as u32,
        frame_ms: words[4] as u32,
        idle_ms: words[5] as u32,
    })
}
