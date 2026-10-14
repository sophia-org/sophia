//! Bounded text records for the administrative file interface.
//!
//! Parsing establishes syntax only. Admission, catalog freshness, replay and
//! execution remain the responsibility of the administrative owner.
use crate::{ControlCommand, ControlOutcome, ControlOwner, validate_control_name};

pub const ADMIN_LINE_MAX_BYTES: usize = 512;
pub const ADMIN_DETAIL_MAX_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdminLineError;

impl std::fmt::Display for AdminLineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid administrative file record")
    }
}
impl std::error::Error for AdminLineError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminInvocation {
    pub connection: u64,
    pub request: u64,
    pub generation: u64,
    pub command: ControlCommand,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminOutcome {
    pub connection: u64,
    pub request: u64,
    pub generation: u64,
    pub outcome: ControlOutcome,
    pub detail: String,
}

fn fields<'a>(bytes: &'a [u8], kind: &str) -> Result<[&'a str; 5], AdminLineError> {
    if bytes.len() > ADMIN_LINE_MAX_BYTES {
        return Err(AdminLineError);
    }
    let line = std::str::from_utf8(bytes).map_err(|_| AdminLineError)?;
    let body = line.strip_suffix('\n').ok_or(AdminLineError)?;
    if body.chars().any(char::is_control) {
        return Err(AdminLineError);
    }
    let mut parts = body.splitn(7, ' ');
    if parts.next() != Some("v1") || parts.next() != Some(kind) {
        return Err(AdminLineError);
    }
    Ok([
        parts.next().ok_or(AdminLineError)?,
        parts.next().ok_or(AdminLineError)?,
        parts.next().ok_or(AdminLineError)?,
        parts.next().ok_or(AdminLineError)?,
        parts.next().ok_or(AdminLineError)?,
    ])
}

fn decimal(text: &str) -> Result<u64, AdminLineError> {
    if text.is_empty()
        || text.len() > 20
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(AdminLineError);
    }
    text.parse().map_err(|_| AdminLineError)
}

fn identity(text: &str) -> Result<u64, AdminLineError> {
    let value = decimal(text)?;
    if value == 0 {
        Err(AdminLineError)
    } else {
        Ok(value)
    }
}

fn counted(text: &str, maximum: usize) -> Result<&str, AdminLineError> {
    let (length, value) = text.split_once(':').ok_or(AdminLineError)?;
    let length = decimal(length)?;
    if length > maximum as u64 || length != value.len() as u64 {
        return Err(AdminLineError);
    }
    Ok(value)
}

pub fn decode_admin_invocation(bytes: &[u8]) -> Result<AdminInvocation, AdminLineError> {
    let [connection, request, generation, owner, name] = fields(bytes, "invoke")?;
    let owner = match owner {
        "policy" => ControlOwner::Policy,
        "session" => ControlOwner::Session,
        _ => return Err(AdminLineError),
    };
    let name = counted(name, 128)?;
    validate_control_name(name).map_err(|_| AdminLineError)?;
    Ok(AdminInvocation {
        connection: identity(connection)?,
        request: identity(request)?,
        generation: identity(generation)?,
        command: ControlCommand {
            owner,
            name: name.to_owned(),
        },
    })
}

pub fn encode_admin_invocation(value: &AdminInvocation) -> Result<Vec<u8>, AdminLineError> {
    validate_control_name(&value.command.name).map_err(|_| AdminLineError)?;
    if value.connection == 0 || value.request == 0 || value.generation == 0 {
        return Err(AdminLineError);
    }
    Ok(format!(
        "v1 invoke {} {} {} {} {}:{}\n",
        value.connection,
        value.request,
        value.generation,
        value.command.owner.name(),
        value.command.name.len(),
        value.command.name
    )
    .into_bytes())
}

pub fn decode_admin_outcome(bytes: &[u8]) -> Result<AdminOutcome, AdminLineError> {
    let [connection, request, generation, outcome, detail] = fields(bytes, "outcome")?;
    let outcome = match outcome {
        "committed" => ControlOutcome::Committed,
        "completed" => ControlOutcome::Completed,
        "unchanged" => ControlOutcome::Unchanged,
        "rejected" => ControlOutcome::Rejected,
        "stale" => ControlOutcome::Stale,
        "denied" => ControlOutcome::Denied,
        "unavailable" => ControlOutcome::Unavailable,
        "overloaded" => ControlOutcome::Overloaded,
        "timed-out" => ControlOutcome::TimedOut,
        "indeterminate" => ControlOutcome::Indeterminate,
        _ => return Err(AdminLineError),
    };
    Ok(AdminOutcome {
        connection: identity(connection)?,
        request: identity(request)?,
        generation: identity(generation)?,
        outcome,
        detail: counted(detail, ADMIN_DETAIL_MAX_BYTES)?.to_owned(),
    })
}

pub fn encode_admin_outcome(value: &AdminOutcome) -> Result<Vec<u8>, AdminLineError> {
    if value.connection == 0
        || value.request == 0
        || value.generation == 0
        || value.detail.len() > ADMIN_DETAIL_MAX_BYTES
        || value.detail.chars().any(char::is_control)
    {
        return Err(AdminLineError);
    }
    Ok(format!(
        "v1 outcome {} {} {} {} {}:{}\n",
        value.connection,
        value.request,
        value.generation,
        value.outcome.name(),
        value.detail.len(),
        value.detail
    )
    .into_bytes())
}
