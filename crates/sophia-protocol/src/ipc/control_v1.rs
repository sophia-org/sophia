//! The public control wire has connection-local correlation, never Engine authority.
use super::cursor::{Cursor, push_u16, push_u32, push_u64};
use super::{IpcCodecError, SOPHIA_IPC_MAX_PAYLOAD_LEN};

pub use crate::control_records::*;
/// The highest revision this implementation speaks.
pub const CONTROL_REVISION: u16 = 2;

/// Session operations each revision names. Revision 1 reserves `reload-profile`
/// without dispatching it; revision 2 dispatches it and adds `logout`.
pub const fn control_session_operations(revision: u16) -> &'static [&'static str] {
    match revision {
        1 => &["reload-profile", "restart-wm"],
        _ => CONTROL_SESSION_OPERATIONS,
    }
}

/// The catalog capacity a welcome advertises for `revision`.
pub const fn control_max_commands(revision: u16) -> usize {
    256 + control_session_operations(revision).len()
}

pub const CONTROL_FRAME_TIMEOUT_MS: u32 = 2000;
pub const CONTROL_COMMAND_TIMEOUT_MS: u32 = 10000;
pub const CONTROL_IDLE_TIMEOUT_MS: u32 = 60000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlWelcome {
    /// The selected revision, 1 or 2.
    pub revision: u16,
    pub session_id: [u64; 2],
    pub connection_id: u64,
    pub command_timeout_ms: u32,
    pub frame_timeout_ms: u32,
    pub idle_timeout_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlMessage {
    Hello {
        minimum_revision: u16,
        maximum_revision: u16,
        required_features: u64,
    },
    Welcome(ControlWelcome),
    Commands,
    Catalog(ControlCatalog),
    Invoke {
        generation: u64,
        command: ControlCommand,
    },
    Outcome {
        generation: u64,
        outcome: ControlOutcome,
        detail: String,
    },
    ProtocolError {
        code: u16,
    },
}

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidRecord("control v1")
}
fn require(ok: bool) -> Result<(), IpcCodecError> {
    if ok { Ok(()) } else { Err(invalid()) }
}
fn owner(raw: u16) -> Result<ControlOwner, IpcCodecError> {
    match raw {
        1 => Ok(ControlOwner::Policy),
        2 => Ok(ControlOwner::Session),
        _ => Err(invalid()),
    }
}
fn text(
    cursor: &mut Cursor<'_>,
    length: usize,
    width: usize,
    name: bool,
) -> Result<String, IpcCodecError> {
    require(length <= width)?;
    let bytes = cursor.slice(width)?;
    require(bytes[length..].iter().all(|b| *b == 0))?;
    let value = std::str::from_utf8(&bytes[..length]).map_err(|_| invalid())?;
    require(!value.chars().any(char::is_control))?;
    if name {
        validate_control_name(value)?;
    }
    Ok(value.to_owned())
}

fn put_text(out: &mut Vec<u8>, value: &str, width: usize) -> Result<(), IpcCodecError> {
    require(value.len() <= width)?;
    out.extend_from_slice(value.as_bytes());
    out.resize(out.len() + width - value.len(), 0);
    Ok(())
}

/// Validate the header before allocating a payload. The ID is local to control.
pub fn decode_control_header(bytes: &[u8]) -> Result<(u16, u64, usize), IpcCodecError> {
    require(bytes.len() == 24)?;
    let mut c = Cursor::new(bytes);
    require(c.u32()? == 0x48504f53 && c.u16()? == 1)?;
    let kind = c.u16()?;
    let id = c.u64()?;
    let len = c.u32()? as usize;
    require((128..=134).contains(&kind) && len <= SOPHIA_IPC_MAX_PAYLOAD_LEN && c.u32()? == 0)?;
    require(match kind {
        128 | 129 => id == 0,
        134 => true,
        _ => id != 0,
    })?;
    Ok((kind, id, len))
}

pub fn decode_control_frame(bytes: &[u8]) -> Result<(u64, ControlMessage), IpcCodecError> {
    let (kind, id, len) = decode_control_header(bytes.get(..24).ok_or(IpcCodecError::Truncated)?)?;
    require(bytes.len() == 24 + len)?;
    let mut c = Cursor::new(&bytes[24..]);
    let message = match kind {
        128 => {
            let minimum_revision = c.u16()?;
            let maximum_revision = c.u16()?;
            require(minimum_revision > 0 && minimum_revision <= maximum_revision)?;
            ControlMessage::Hello {
                minimum_revision,
                maximum_revision,
                required_features: c.u64()?,
            }
        }
        129 => {
            let revision = c.u16()?;
            require((1..=CONTROL_REVISION).contains(&revision) && c.u16()? == 0)?;
            let session_id = [c.u64()?, c.u64()?];
            let connection_id = c.u64()?;
            require(session_id != [0, 0] && connection_id != 0 && c.u64()? == 0)?;
            require(
                c.u32()? == 65536
                    && usize::from(c.u16()?) == control_max_commands(revision)
                    && c.u16()? == 128,
            )?;
            let welcome = ControlWelcome {
                revision,
                session_id,
                connection_id,
                command_timeout_ms: c.u32()?,
                frame_timeout_ms: c.u32()?,
                idle_timeout_ms: c.u32()?,
            };
            require(
                (1..=10000).contains(&welcome.command_timeout_ms)
                    && (1..=2000).contains(&welcome.frame_timeout_ms)
                    && (1..=60000).contains(&welcome.idle_timeout_ms),
            )?;
            ControlMessage::Welcome(welcome)
        }
        130 => ControlMessage::Commands,
        131 => {
            let generation = c.u64()?;
            let count = c.u16()? as usize;
            require(
                generation > 0
                    && count <= CONTROL_MAX_COMMANDS
                    && c.u16()? == 0
                    && len == 12 + count * 136,
            )?;
            let mut commands = Vec::with_capacity(count);
            for _ in 0..count {
                let owner = owner(c.u16()?)?;
                require(c.u16()? == owner as u16)?;
                let length = c.u16()? as usize;
                require(c.u16()? == 0)?;
                let name = text(&mut c, length, 128, true)?;
                require(owner != ControlOwner::Session || control_session_operation(&name))?;
                let command = ControlCommand { owner, name };
                require(commands.last().is_none_or(|last| last < &command))?;
                commands.push(command);
            }
            require(
                commands
                    .iter()
                    .filter(|c| c.owner == ControlOwner::Policy)
                    .count()
                    <= 256,
            )?;
            ControlMessage::Catalog(ControlCatalog {
                generation,
                commands,
            })
        }
        132 => {
            let generation = c.u64()?;
            require(generation > 0)?;
            let owner = owner(c.u16()?)?;
            let length = c.u16()? as usize;
            ControlMessage::Invoke {
                generation,
                command: ControlCommand {
                    owner,
                    name: text(&mut c, length, 128, true)?,
                },
            }
        }
        133 => {
            let generation = c.u64()?;
            require(generation > 0)?;
            let outcome = ControlOutcome::from_wire(c.u16()?)?;
            let length = c.u16()? as usize;
            ControlMessage::Outcome {
                generation,
                outcome,
                detail: text(&mut c, length, 256, false)?,
            }
        }
        134 => {
            let code = c.u16()?;
            require((1..=4).contains(&code) && c.u16()? == 0)?;
            ControlMessage::ProtocolError { code }
        }
        _ => return Err(invalid()),
    };
    c.finish()?;
    Ok((id, message))
}

pub fn encode_control_frame(id: u64, message: &ControlMessage) -> Result<Vec<u8>, IpcCodecError> {
    let mut out = Vec::new();
    let kind: u16 = match message {
        ControlMessage::Hello {
            minimum_revision,
            maximum_revision,
            required_features,
        } => {
            push_u16(&mut out, *minimum_revision);
            push_u16(&mut out, *maximum_revision);
            push_u64(&mut out, *required_features);
            128
        }
        ControlMessage::Welcome(w) => {
            require((1..=CONTROL_REVISION).contains(&w.revision))?;
            push_u16(&mut out, w.revision);
            push_u16(&mut out, 0);
            for n in [w.session_id[0], w.session_id[1], w.connection_id, 0] {
                push_u64(&mut out, n);
            }
            push_u32(&mut out, 65536);
            push_u16(&mut out, control_max_commands(w.revision) as u16);
            push_u16(&mut out, 128);
            for n in [w.command_timeout_ms, w.frame_timeout_ms, w.idle_timeout_ms] {
                push_u32(&mut out, n);
            }
            129
        }
        ControlMessage::Commands => 130,
        ControlMessage::Catalog(catalog) => {
            require(catalog.commands.len() <= CONTROL_MAX_COMMANDS)?;
            push_u64(&mut out, catalog.generation);
            push_u16(&mut out, catalog.commands.len() as u16);
            push_u16(&mut out, 0);
            for command in &catalog.commands {
                push_u16(&mut out, command.owner as u16);
                push_u16(&mut out, command.owner as u16);
                push_u16(&mut out, command.name.len() as u16);
                push_u16(&mut out, 0);
                put_text(&mut out, &command.name, 128)?;
            }
            131
        }
        ControlMessage::Invoke {
            generation,
            command,
        } => {
            push_u64(&mut out, *generation);
            push_u16(&mut out, command.owner as u16);
            push_u16(&mut out, command.name.len() as u16);
            put_text(&mut out, &command.name, 128)?;
            132
        }
        ControlMessage::Outcome {
            generation,
            outcome,
            detail,
        } => {
            push_u64(&mut out, *generation);
            push_u16(&mut out, *outcome as u16);
            push_u16(&mut out, detail.len() as u16);
            put_text(&mut out, detail, 256)?;
            133
        }
        ControlMessage::ProtocolError { code } => {
            push_u16(&mut out, *code);
            push_u16(&mut out, 0);
            134
        }
    };
    let mut frame = Vec::with_capacity(24 + out.len());
    push_u32(&mut frame, 0x48504f53);
    push_u16(&mut frame, 1);
    push_u16(&mut frame, kind);
    push_u64(&mut frame, id);
    push_u32(&mut frame, out.len() as u32);
    push_u32(&mut frame, 0);
    frame.extend_from_slice(&out);
    // Encoding observes exactly the same strict semantic contract as decoding.
    decode_control_frame(&frame)?;
    Ok(frame)
}
