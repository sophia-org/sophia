use crate::{LINE_MAX_BYTES, RecordError, decimal};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Committed,
    Completed,
    Unchanged,
    Rejected,
    Stale,
    Denied,
    Unavailable,
    Overloaded,
    TimedOut,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status<'a> {
    Ready {
        connection: u64,
    },
    Pending {
        connection: u64,
        request: u64,
        generation: u64,
    },
    Outcome {
        connection: u64,
        request: u64,
        generation: u64,
        outcome: Outcome,
        detail: &'a str,
    },
}

fn identity(text: Option<&str>) -> Result<u64, RecordError> {
    let n = decimal(text.ok_or(RecordError)?)?;
    if n == 0 { Err(RecordError) } else { Ok(n) }
}

/// Decode a complete status snapshot without allocation. Detail borrows input.
/// The client must separately verify EOF, identity and owner-specific outcomes.
pub fn decode_status(bytes: &[u8]) -> Result<Status<'_>, RecordError> {
    if bytes.len() > LINE_MAX_BYTES {
        return Err(RecordError);
    }
    let line = std::str::from_utf8(bytes).map_err(|_| RecordError)?;
    let body = line.strip_suffix('\n').ok_or(RecordError)?;
    if body.chars().any(char::is_control) {
        return Err(RecordError);
    }
    let mut fields = body.splitn(7, ' ');
    if fields.next() != Some("v1") {
        return Err(RecordError);
    }
    let kind = fields.next().ok_or(RecordError)?;
    let connection = identity(fields.next())?;
    if kind == "ready" {
        if fields.next().is_some() {
            return Err(RecordError);
        }
        return Ok(Status::Ready { connection });
    }
    let request = identity(fields.next())?;
    let generation = identity(fields.next())?;
    if kind == "pending" {
        if fields.next().is_some() {
            return Err(RecordError);
        }
        return Ok(Status::Pending {
            connection,
            request,
            generation,
        });
    }
    if kind != "outcome" {
        return Err(RecordError);
    }
    let outcome = match fields.next() {
        Some("committed") => Outcome::Committed,
        Some("completed") => Outcome::Completed,
        Some("unchanged") => Outcome::Unchanged,
        Some("rejected") => Outcome::Rejected,
        Some("stale") => Outcome::Stale,
        Some("denied") => Outcome::Denied,
        Some("unavailable") => Outcome::Unavailable,
        Some("overloaded") => Outcome::Overloaded,
        Some("timed-out") => Outcome::TimedOut,
        Some("indeterminate") => Outcome::Indeterminate,
        _ => return Err(RecordError),
    };
    let (count, detail) = fields
        .next()
        .ok_or(RecordError)?
        .split_once(':')
        .ok_or(RecordError)?;
    if detail.len() > 256 || decimal(count)? != detail.len() as u64 {
        return Err(RecordError);
    }
    Ok(Status::Outcome {
        connection,
        request,
        generation,
        outcome,
        detail,
    })
}
