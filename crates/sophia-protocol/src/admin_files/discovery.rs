//! Bounded snapshots for administrative discovery and status.
use super::*;
use crate::control_records::{CONTROL_MAX_COMMANDS, ControlCatalog, validate_control_catalog};
use std::fmt::Write;

// A header fits in 64 bytes; each command has at most 128 name bytes.
pub const ADMIN_CATALOG_MAX_BYTES: usize = 64 + CONTROL_MAX_COMMANDS * 152;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdminInfo {
    pub session_id: [u64; 2],
    pub connection: u64,
    pub command_timeout_ms: u32,
    pub frame_timeout_ms: u32,
    pub idle_timeout_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdminStatus {
    Ready {
        connection: u64,
    },
    Pending {
        connection: u64,
        request: u64,
        generation: u64,
    },
    Outcome(AdminOutcome),
}

fn tokens<const N: usize>(bytes: &[u8]) -> Result<[&str; N], AdminLineError> {
    let mut parts = line(bytes)?.split(' ');
    let mut fields = [""; N];
    for field in &mut fields {
        *field = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(AdminLineError)?;
    }
    if parts.next().is_some() {
        return Err(AdminLineError);
    }
    Ok(fields)
}

pub fn decode_admin_info(bytes: &[u8]) -> Result<AdminInfo, AdminLineError> {
    let ["v1", "info", low, high, connection, command, frame, idle] = tokens(bytes)? else {
        return Err(AdminLineError);
    };
    let session_id = [decimal(low)?, decimal(high)?];
    if session_id == [0, 0] {
        return Err(AdminLineError);
    }
    let timeout = |text| u32::try_from(identity(text)?).map_err(|_| AdminLineError);
    Ok(AdminInfo {
        session_id,
        connection: identity(connection)?,
        command_timeout_ms: timeout(command)?,
        frame_timeout_ms: timeout(frame)?,
        idle_timeout_ms: timeout(idle)?,
    })
}

pub fn encode_admin_info(value: &AdminInfo) -> Result<Vec<u8>, AdminLineError> {
    if value.session_id == [0, 0]
        || value.connection == 0
        || value.command_timeout_ms == 0
        || value.frame_timeout_ms == 0
        || value.idle_timeout_ms == 0
    {
        return Err(AdminLineError);
    }
    Ok(format!(
        "v1 info {} {} {} {} {} {}\n",
        value.session_id[0],
        value.session_id[1],
        value.connection,
        value.command_timeout_ms,
        value.frame_timeout_ms,
        value.idle_timeout_ms
    )
    .into_bytes())
}

pub fn decode_admin_status(bytes: &[u8]) -> Result<AdminStatus, AdminLineError> {
    if bytes.starts_with(b"v1 outcome ") {
        return decode_admin_outcome(bytes).map(AdminStatus::Outcome);
    }
    if bytes.starts_with(b"v1 ready ") {
        let [_, _, connection] = tokens(bytes)?;
        return Ok(AdminStatus::Ready {
            connection: identity(connection)?,
        });
    }
    let ["v1", "pending", connection, request, generation] = tokens(bytes)? else {
        return Err(AdminLineError);
    };
    Ok(AdminStatus::Pending {
        connection: identity(connection)?,
        request: identity(request)?,
        generation: identity(generation)?,
    })
}

pub fn encode_admin_status(value: &AdminStatus) -> Result<Vec<u8>, AdminLineError> {
    match value {
        AdminStatus::Ready { connection } if *connection != 0 => {
            Ok(format!("v1 ready {connection}\n").into_bytes())
        }
        AdminStatus::Pending {
            connection,
            request,
            generation,
        } if *connection != 0 && *request != 0 && *generation != 0 => {
            Ok(format!("v1 pending {connection} {request} {generation}\n").into_bytes())
        }
        AdminStatus::Outcome(value) => encode_admin_outcome(value),
        _ => Err(AdminLineError),
    }
}

pub fn decode_admin_catalog(bytes: &[u8]) -> Result<ControlCatalog, AdminLineError> {
    if bytes.len() > ADMIN_CATALOG_MAX_BYTES {
        return Err(AdminLineError);
    }
    let mut lines = bytes.split_inclusive(|b| *b == b'\n');
    let ["v1", "catalog", generation, count] = tokens(lines.next().ok_or(AdminLineError)?)? else {
        return Err(AdminLineError);
    };
    let count = decimal(count)?;
    if count > CONTROL_MAX_COMMANDS as u64 {
        return Err(AdminLineError);
    }
    let mut catalog = ControlCatalog {
        generation: identity(generation)?,
        commands: Vec::with_capacity(count as usize),
    };
    for _ in 0..count {
        let mut parts = line(lines.next().ok_or(AdminLineError)?)?.splitn(4, ' ');
        if parts.next() != Some("v1") || parts.next() != Some("command") {
            return Err(AdminLineError);
        }
        let owner = match parts.next() {
            Some("policy") => ControlOwner::Policy,
            Some("session") => ControlOwner::Session,
            _ => return Err(AdminLineError),
        };
        let name = counted(parts.next().ok_or(AdminLineError)?, 128)?;
        validate_control_name(name).map_err(|_| AdminLineError)?;
        catalog.commands.push(ControlCommand {
            owner,
            name: name.to_owned(),
        });
    }
    if lines.next().is_some() {
        return Err(AdminLineError);
    }
    validate_control_catalog(&catalog).map_err(|_| AdminLineError)?;
    Ok(catalog)
}

pub fn encode_admin_catalog(value: &ControlCatalog) -> Result<Vec<u8>, AdminLineError> {
    // Validate before allocating or formatting any client-supplied text.
    validate_control_catalog(value).map_err(|_| AdminLineError)?;
    let mut out = format!("v1 catalog {} {}\n", value.generation, value.commands.len());
    for command in &value.commands {
        writeln!(
            out,
            "v1 command {} {}:{}",
            command.owner.name(),
            command.name.len(),
            command.name
        )
        .expect("formatting into a String cannot fail");
    }
    Ok(out.into_bytes())
}
