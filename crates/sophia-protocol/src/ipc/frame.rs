use crate::TransactionId;

use super::cursor::{Cursor, push_u16, push_u32, push_u64};
use super::types::{
    IpcCodecError, IpcFrameHeader, IpcMessageKind, SOPHIA_IPC_HEADER_LEN, SOPHIA_IPC_MAGIC,
    SOPHIA_IPC_MAX_PAYLOAD_LEN, SOPHIA_IPC_VERSION,
};

pub fn encode_frame(
    message_kind: IpcMessageKind,
    transaction: TransactionId,
    payload: &[u8],
) -> Result<Vec<u8>, IpcCodecError> {
    if payload.len() > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(IpcCodecError::PayloadTooLarge(payload.len()));
    }

    let mut frame = Vec::with_capacity(SOPHIA_IPC_HEADER_LEN + payload.len());
    push_u32(&mut frame, SOPHIA_IPC_MAGIC);
    push_u16(&mut frame, SOPHIA_IPC_VERSION);
    push_u16(&mut frame, message_kind as u16);
    push_u64(&mut frame, transaction.raw());
    push_u32(&mut frame, payload.len() as u32);
    push_u32(&mut frame, 0);
    frame.extend_from_slice(payload);
    Ok(frame)
}

pub fn decode_frame(frame: &[u8]) -> Result<(IpcFrameHeader, &[u8]), IpcCodecError> {
    if frame.len() < SOPHIA_IPC_HEADER_LEN {
        return Err(IpcCodecError::Truncated);
    }

    let mut cursor = Cursor::new(frame);
    let magic = cursor.u32()?;
    if magic != SOPHIA_IPC_MAGIC {
        return Err(IpcCodecError::BadMagic);
    }

    let version = cursor.u16()?;
    if version != SOPHIA_IPC_VERSION {
        return Err(IpcCodecError::UnsupportedVersion(version));
    }

    let message_kind = match cursor.u16()? {
        3 => IpcMessageKind::BrokerHealth,
        4 => IpcMessageKind::XAuthorityRequest,
        5 => IpcMessageKind::XAuthorityResponse,
        6 => IpcMessageKind::PortalBrokerRequest,
        7 => IpcMessageKind::PortalBrokerResponse,
        8 => IpcMessageKind::PortalClipboardPayload,
        64 => IpcMessageKind::OutputV1ClientHello,
        65 => IpcMessageKind::OutputV1ServerWelcome,
        66 => IpcMessageKind::OutputV1Snapshot,
        67 => IpcMessageKind::OutputV1Proposal,
        68 => IpcMessageKind::OutputV1Outcome,
        80 => IpcMessageKind::BrokerV1ClientHello,
        81 => IpcMessageKind::BrokerV1ServerWelcome,
        82 => IpcMessageKind::BrokerV1Request,
        83 => IpcMessageKind::BrokerV1Response,

        other => return Err(IpcCodecError::UnknownMessageKind(other)),
    };
    let transaction = TransactionId::from_raw(cursor.u64()?);
    let payload_len = cursor.u32()?;
    let reserved = cursor.u32()?;
    if reserved != 0 {
        return Err(IpcCodecError::ReservedNonZero(reserved));
    }

    let payload_len_usize = payload_len as usize;
    if payload_len_usize > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(IpcCodecError::PayloadTooLarge(payload_len_usize));
    }
    let expected_len = SOPHIA_IPC_HEADER_LEN
        .checked_add(payload_len_usize)
        .ok_or(IpcCodecError::PayloadTooLarge(payload_len_usize))?;
    if frame.len() < expected_len {
        return Err(IpcCodecError::Truncated);
    }
    if frame.len() > expected_len {
        return Err(IpcCodecError::TrailingBytes(frame.len() - expected_len));
    }

    Ok((
        IpcFrameHeader {
            message_kind,
            transaction,
            payload_len,
        },
        &frame[SOPHIA_IPC_HEADER_LEN..expected_len],
    ))
}
