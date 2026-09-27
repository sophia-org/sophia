//! The `sophia_shell_v1` handshake frames: the client hello and the server
//! welcome, and nothing else of that revision's descriptor vocabulary.
use crate::{ShellV1ClientHello, ShellV1ServerWelcome, TransactionId};

use super::cursor::{Cursor, push_u16, push_u64};
use super::frame::{decode_frame, encode_frame};
use super::types::{IpcCodecError, IpcMessageKind};

/// The welcome's descriptor and label bounds, as the session's shell
/// vocabulary declares them (`SOPHIA_SHELL_MAX_DESCRIPTORS`,
/// `MAX_CHROME_LABEL_LEN`); Sophia's parity test pins both.
pub const SOPHIA_SHELL_V1_MAX_DESCRIPTORS: usize = 16;
pub const SOPHIA_SHELL_V1_MAX_LABEL_BYTES: usize = 128;

pub fn encode_shell_v1_client_hello_frame(
    hello: ShellV1ClientHello,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut payload = Vec::with_capacity(16);
    push_u16(&mut payload, hello.minimum_revision);
    push_u16(&mut payload, hello.maximum_revision);
    push_u64(&mut payload, hello.required_capabilities);
    encode_frame(
        IpcMessageKind::ShellV1ClientHello,
        TransactionId::INVALID,
        &payload,
    )
}

pub fn decode_shell_v1_client_hello_frame(
    frame: &[u8],
) -> Result<ShellV1ClientHello, IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1ClientHello)?;
    require_handshake_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let hello = ShellV1ClientHello {
        minimum_revision: cursor.u16()?,
        maximum_revision: cursor.u16()?,
        required_capabilities: cursor.u64()?,
    };
    cursor.finish()?;
    Ok(hello)
}

pub fn encode_shell_v1_server_welcome_frame(
    welcome: ShellV1ServerWelcome,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut payload = Vec::with_capacity(32);
    push_u16(&mut payload, welcome.selected_revision);
    push_u16(&mut payload, 0);
    push_u64(&mut payload, welcome.connection_epoch);
    push_u64(&mut payload, welcome.capabilities);
    push_u16(&mut payload, welcome.max_descriptors);
    push_u16(&mut payload, welcome.max_label_bytes);
    push_u16(&mut payload, welcome.max_pending_activations);
    push_u16(&mut payload, 0);
    encode_frame(
        IpcMessageKind::ShellV1ServerWelcome,
        TransactionId::INVALID,
        &payload,
    )
}

pub fn decode_shell_v1_server_welcome_frame(
    frame: &[u8],
) -> Result<ShellV1ServerWelcome, IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1ServerWelcome)?;
    require_handshake_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let selected_revision = cursor.u16()?;
    require_zero(cursor.u16()?, "shell_welcome_reserved")?;
    let welcome = ShellV1ServerWelcome {
        selected_revision,
        connection_epoch: cursor.u64()?,
        capabilities: cursor.u64()?,
        max_descriptors: cursor.u16()?,
        max_label_bytes: cursor.u16()?,
        max_pending_activations: cursor.u16()?,
    };
    require_zero(cursor.u16()?, "shell_welcome_trailing_reserved")?;
    cursor.finish()?;
    validate_welcome(welcome)?;
    Ok(welcome)
}

fn validate_welcome(welcome: ShellV1ServerWelcome) -> Result<(), IpcCodecError> {
    if welcome.selected_revision == 0
        || welcome.connection_epoch == 0
        || welcome.max_descriptors == 0
        || usize::from(welcome.max_descriptors) > SOPHIA_SHELL_V1_MAX_DESCRIPTORS
        || welcome.max_label_bytes == 0
        || usize::from(welcome.max_label_bytes) > SOPHIA_SHELL_V1_MAX_LABEL_BYTES
        || welcome.max_pending_activations == 0
    {
        return Err(IpcCodecError::InvalidRecord("shell_welcome"));
    }
    Ok(())
}

fn require_kind(actual: IpcMessageKind, expected: IpcMessageKind) -> Result<(), IpcCodecError> {
    if actual == expected {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidEnum {
            field: "shell_message_kind",
            value: actual as u32,
        })
    }
}

fn require_handshake_transaction(transaction: TransactionId) -> Result<(), IpcCodecError> {
    if transaction.is_valid() {
        Err(IpcCodecError::InvalidTransaction(transaction.raw()))
    } else {
        Ok(())
    }
}

fn require_zero(value: u16, _field: &'static str) -> Result<(), IpcCodecError> {
    if value == 0 {
        Ok(())
    } else {
        Err(IpcCodecError::ReservedNonZero(u32::from(value)))
    }
}
