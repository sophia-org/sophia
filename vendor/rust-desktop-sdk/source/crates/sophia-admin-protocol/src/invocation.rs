use crate::{LINE_MAX_BYTES, RecordError, decimal};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Owner {
    Policy,
    Session,
}

impl Owner {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Session => "session",
        }
    }
}

/// The name borrows its backing record. Parsing does not prove catalog membership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Invocation<'a> {
    pub connection: u64,
    pub request: u64,
    pub generation: u64,
    pub owner: Owner,
    pub name: &'a str,
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'_' | b'.'))
}

pub fn decode_invocation(bytes: &[u8]) -> Result<Invocation<'_>, RecordError> {
    if bytes.len() > LINE_MAX_BYTES {
        return Err(RecordError);
    }
    let line = std::str::from_utf8(bytes).map_err(|_| RecordError)?;
    let mut fields = line.strip_suffix('\n').ok_or(RecordError)?.splitn(7, ' ');
    if fields.next() != Some("v1") || fields.next() != Some("invoke") {
        return Err(RecordError);
    }
    let connection = decimal(fields.next().ok_or(RecordError)?)?;
    let request = decimal(fields.next().ok_or(RecordError)?)?;
    let generation = decimal(fields.next().ok_or(RecordError)?)?;
    let owner = match fields.next() {
        Some("policy") => Owner::Policy,
        Some("session") => Owner::Session,
        _ => return Err(RecordError),
    };
    let (count, name) = fields
        .next()
        .ok_or(RecordError)?
        .split_once(':')
        .ok_or(RecordError)?;
    if connection == 0
        || request == 0
        || generation == 0
        || decimal(count)? != name.len() as u64
        || !valid_name(name)
    {
        return Err(RecordError);
    }
    Ok(Invocation {
        connection,
        request,
        generation,
        owner,
        name,
    })
}

struct Buffer {
    bytes: [u8; LINE_MAX_BYTES],
    len: usize,
}
impl Write for Buffer {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(std::fmt::Error)?;
        let dst = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        dst.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Encode into caller storage without allocation. Failure leaves storage unchanged.
/// The result is one complete offset-zero ctl write, never a sequence of fragments.
pub fn encode_invocation(value: Invocation<'_>, out: &mut [u8]) -> Result<usize, RecordError> {
    if value.connection == 0
        || value.request == 0
        || value.generation == 0
        || !valid_name(value.name)
    {
        return Err(RecordError);
    }
    let mut buffer = Buffer {
        bytes: [0; LINE_MAX_BYTES],
        len: 0,
    };
    writeln!(
        buffer,
        "v1 invoke {} {} {} {} {}:{}",
        value.connection,
        value.request,
        value.generation,
        value.owner.name(),
        value.name.len(),
        value.name
    )
    .map_err(|_| RecordError)?;
    let dst = out.get_mut(..buffer.len).ok_or(RecordError)?;
    dst.copy_from_slice(&buffer.bytes[..buffer.len]);
    Ok(buffer.len)
}
