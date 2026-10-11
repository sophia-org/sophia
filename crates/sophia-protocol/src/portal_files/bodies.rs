//! The bodies of `protocol/sophia-portal-files-v1.kdl`, with their declared
//! byte constraints and the value rules its prose states. Decoding refuses a
//! malformed body; an encoder refuses to produce one. Admission, the admitted
//! namespace pair, lifecycle validity and budgets belong to the export.

use super::{PortalFileClass, PortalFileKind, invalid, nonzero, range, reserved};
use crate::byte_cursor::{Cursor, push_u8, push_u16, push_u32, push_u64};
use crate::{
    BinaryCodecError, NamespaceId, PortalBrokerResponseDecision, PortalBrokerResponsePacket,
    PortalDecision, PortalGrant, PortalGrantState, PortalRequest, PortalTransfer, PortalTransferId,
    PortalTransferKind, SOPHIA_PORTAL_MAX_MIME_TYPE_LEN,
};

/// One payload slot carries at most one clipboard handoff.
pub const PORTAL_FILE_MAX_PAYLOAD_BYTES: u64 = 65_536;
pub const PORTAL_FILE_MAX_TRANSFERS: u32 = 64;
pub const PORTAL_FILE_MAX_RETAINED_TRANSFERS: u32 = 4_096;
pub const PORTAL_FILE_INTERFACE_REVISION: u16 = 1;
/// The fixed staging size: the longest candidate record.
pub const PORTAL_FILE_STAGING_BYTES: u32 = 344;
const UPLOAD_SLOTS: u16 = 1;
const MIME_BYTES: usize = SOPHIA_PORTAL_MAX_MIME_TYPE_LEN;

/// The immutable limits object, readable before negotiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalFileLimits {
    pub max_transfers: u32,
    pub max_payload_bytes: u32,
    pub journal_records: u32,
    pub journal_bytes: u32,
    pub assembly_timeout_ms: u32,
    pub ack_progress_timeout_ms: u32,
    pub max_retained_transfers: u32,
    pub broker_generation: u64,
}

impl PortalFileLimits {
    pub const BYTES: usize = 48;

    /// The contract's ceilings, which are also its defaults.
    pub const fn defaults(broker_generation: u64) -> Self {
        Self {
            max_transfers: PORTAL_FILE_MAX_TRANSFERS,
            max_payload_bytes: PORTAL_FILE_MAX_PAYLOAD_BYTES as u32,
            journal_records: 256,
            journal_bytes: 32_768,
            assembly_timeout_ms: 12_000,
            ack_progress_timeout_ms: 2_000,
            max_retained_transfers: PORTAL_FILE_MAX_RETAINED_TRANSFERS,
            broker_generation,
        }
    }

    pub fn validate(&self) -> Result<(), BinaryCodecError> {
        range(
            u64::from(self.max_transfers),
            1,
            u64::from(PORTAL_FILE_MAX_TRANSFERS),
            "max_transfers",
        )?;
        range(
            u64::from(self.max_payload_bytes),
            1,
            PORTAL_FILE_MAX_PAYLOAD_BYTES,
            "max_payload_bytes",
        )?;
        range(u64::from(self.journal_records), 4, 256, "journal_records")?;
        range(u64::from(self.journal_bytes), 280, 32_768, "journal_bytes")?;
        range(
            u64::from(self.assembly_timeout_ms),
            1,
            12_000,
            "assembly_timeout_ms",
        )?;
        range(
            u64::from(self.ack_progress_timeout_ms),
            1,
            2_000,
            "ack_progress_timeout_ms",
        )?;
        range(
            u64::from(self.max_retained_transfers),
            1,
            u64::from(PORTAL_FILE_MAX_RETAINED_TRANSFERS),
            "max_retained_transfers",
        )?;
        // Every active transfer keeps a retained identity.
        if self.max_retained_transfers < self.max_transfers {
            return Err(invalid("max_retained_transfers"));
        }
        nonzero(self.broker_generation, "broker_generation")?;
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(Self::BYTES);
        push_u16(&mut bytes, PORTAL_FILE_INTERFACE_REVISION);
        push_u16(&mut bytes, UPLOAD_SLOTS);
        push_u32(&mut bytes, self.max_transfers);
        push_u32(&mut bytes, self.max_payload_bytes);
        push_u16(&mut bytes, MIME_BYTES as u16);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, self.journal_records);
        push_u32(&mut bytes, self.journal_bytes);
        push_u32(&mut bytes, self.assembly_timeout_ms);
        push_u32(&mut bytes, self.ack_progress_timeout_ms);
        push_u32(&mut bytes, PORTAL_FILE_STAGING_BYTES);
        push_u32(&mut bytes, self.max_retained_transfers);
        push_u64(&mut bytes, self.broker_generation);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.u16()? != PORTAL_FILE_INTERFACE_REVISION {
            return Err(invalid("interface_revision"));
        }
        if cursor.u16()? != UPLOAD_SLOTS {
            return Err(invalid("upload_slots"));
        }
        let max_transfers = cursor.u32()?;
        let max_payload_bytes = cursor.u32()?;
        if usize::from(cursor.u16()?) != MIME_BYTES {
            return Err(invalid("max_mime_bytes"));
        }
        reserved(&mut cursor, 2)?;
        let journal_records = cursor.u32()?;
        let journal_bytes = cursor.u32()?;
        let assembly_timeout_ms = cursor.u32()?;
        let ack_progress_timeout_ms = cursor.u32()?;
        if cursor.u32()? != PORTAL_FILE_STAGING_BYTES {
            return Err(invalid("staging_bytes"));
        }
        let limits = Self {
            max_transfers,
            max_payload_bytes,
            journal_records,
            journal_bytes,
            assembly_timeout_ms,
            ack_progress_timeout_ms,
            max_retained_transfers: cursor.u32()?,
            broker_generation: cursor.u64()?,
        };
        cursor.finish()?;
        limits.validate()?;
        Ok(limits)
    }
}

/// The requester's revision range. Any range decodes: an invalid or
/// unsupported one is the owner's Refused, not a malformed record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalNegotiate {
    pub minimum_revision: u16,
    pub maximum_revision: u16,
}

impl PortalNegotiate {
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8);
        push_u16(&mut bytes, self.minimum_revision);
        push_u16(&mut bytes, self.maximum_revision);
        push_u32(&mut bytes, 0);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let negotiate = Self {
            minimum_revision: cursor.u16()?,
            maximum_revision: cursor.u16()?,
        };
        reserved(&mut cursor, 4)?;
        cursor.finish()?;
        Ok(negotiate)
    }
}

/// Negotiated's body: the selected revision and the connection epoch it
/// confirms.
pub(super) fn encode_negotiated(connection_epoch: u64) -> Result<Vec<u8>, BinaryCodecError> {
    nonzero(connection_epoch, "connection_epoch")?;
    let mut bytes = Vec::with_capacity(16);
    push_u16(&mut bytes, PORTAL_FILE_INTERFACE_REVISION);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u64(&mut bytes, connection_epoch);
    Ok(bytes)
}

pub(super) fn decode_negotiated(bytes: &[u8]) -> Result<u64, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.u16()? != PORTAL_FILE_INTERFACE_REVISION {
        return Err(invalid("selected_revision"));
    }
    reserved(&mut cursor, 6)?;
    let epoch = nonzero(cursor.u64()?, "connection_epoch")?;
    cursor.finish()?;
    Ok(epoch)
}

/// Refused carries one reason, an unsupported revision range.
const REFUSED_UNSUPPORTED_REVISION: u16 = 1;

pub(super) fn encode_refused() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    push_u16(&mut bytes, REFUSED_UNSUPPORTED_REVISION);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes
}

pub(super) fn decode_refused(bytes: &[u8]) -> Result<(), BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.u16()? != REFUSED_UNSUPPORTED_REVISION {
        return Err(invalid("reason"));
    }
    reserved(&mut cursor, 6)?;
    cursor.finish()?;
    Ok(())
}

/// The receipt for one accepted submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalSubmitted {
    pub submission_id: u64,
    pub candidate_kind: PortalFileKind,
}

impl PortalSubmitted {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.submission_id, "submission_id")?;
        if self.candidate_kind.class() != PortalFileClass::Candidate {
            return Err(invalid("candidate_kind"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(16);
        push_u64(&mut bytes, self.submission_id);
        push_u16(&mut bytes, self.candidate_kind as u16);
        bytes.extend_from_slice(&[0; 6]);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let submission_id = cursor.u64()?;
        let candidate_kind =
            PortalFileKind::decode(cursor.u16()?).map_err(|_| invalid("candidate_kind"))?;
        reserved(&mut cursor, 6)?;
        cursor.finish()?;
        let submitted = Self {
            submission_id,
            candidate_kind,
        };
        submitted.validate()?;
        Ok(submitted)
    }
}

fn kind_value(kind: PortalTransferKind) -> u16 {
    match kind {
        PortalTransferKind::Clipboard => 1,
        PortalTransferKind::DragAndDrop => 2,
        PortalTransferKind::FileHandoff => 3,
        PortalTransferKind::ScreenCapture => 4,
        PortalTransferKind::ScreenRecording => 5,
        PortalTransferKind::UriOpen => 6,
        PortalTransferKind::Notification => 7,
    }
}

fn decode_kind(value: u16) -> Result<PortalTransferKind, BinaryCodecError> {
    Ok(match value {
        1 => PortalTransferKind::Clipboard,
        2 => PortalTransferKind::DragAndDrop,
        3 => PortalTransferKind::FileHandoff,
        4 => PortalTransferKind::ScreenCapture,
        5 => PortalTransferKind::ScreenRecording,
        6 => PortalTransferKind::UriOpen,
        7 => PortalTransferKind::Notification,
        _ => return Err(invalid("kind")),
    })
}

/// TransferRequest's body as the lifecycle's request. It is always Pending:
/// the wire has no decision field. Namespace distinctness and membership in
/// the admitted pair are the owner's admission checks, not syntax. Zero
/// transfer, generation and deadline are representable; the lifecycle denies
/// them.
pub fn encode_portal_transfer_request(
    request: &PortalRequest,
) -> Result<Vec<u8>, BinaryCodecError> {
    let transfer = &request.transfer;
    if transfer.decision != PortalDecision::Pending {
        return Err(invalid("decision"));
    }
    nonzero(transfer.source_namespace.raw(), "source_namespace")?;
    nonzero(transfer.target_namespace.raw(), "target_namespace")?;
    let mime = transfer.mime_type.as_deref().map(str::as_bytes);
    if mime.is_some_and(|mime| mime.len() > MIME_BYTES) {
        return Err(invalid("mime_len"));
    }
    let mut bytes = Vec::with_capacity(312);
    push_u64(&mut bytes, transfer.transfer.raw());
    push_u64(&mut bytes, transfer.source_namespace.raw());
    push_u64(&mut bytes, transfer.target_namespace.raw());
    push_u16(&mut bytes, kind_value(transfer.kind));
    push_u8(&mut bytes, u8::from(mime.is_some()));
    push_u8(&mut bytes, 0);
    push_u16(&mut bytes, mime.map_or(0, |mime| mime.len() as u16));
    push_u16(&mut bytes, 0);
    push_u64(&mut bytes, transfer.byte_size);
    push_u64(&mut bytes, transfer.generation);
    push_u64(&mut bytes, request.deadline_msec);
    let mut text = [0; MIME_BYTES];
    if let Some(mime) = mime {
        text[..mime.len()].copy_from_slice(mime);
    }
    bytes.extend_from_slice(&text);
    push_u8(&mut bytes, 0);
    Ok(bytes)
}

pub fn decode_portal_transfer_request(bytes: &[u8]) -> Result<PortalRequest, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let transfer = PortalTransferId::from_raw(cursor.u64()?);
    let source_namespace = NamespaceId::from_raw(nonzero(cursor.u64()?, "source_namespace")?);
    let target_namespace = NamespaceId::from_raw(nonzero(cursor.u64()?, "target_namespace")?);
    let kind = decode_kind(cursor.u16()?)?;
    let present = match cursor.u8()? {
        0 => false,
        1 => true,
        _ => return Err(invalid("mime_present")),
    };
    reserved(&mut cursor, 1)?;
    let mime_len = usize::from(cursor.u16()?);
    reserved(&mut cursor, 2)?;
    let byte_size = cursor.u64()?;
    let generation = cursor.u64()?;
    let deadline_msec = cursor.u64()?;
    let text = cursor.slice(MIME_BYTES)?;
    reserved(&mut cursor, 1)?;
    cursor.finish()?;
    if mime_len > MIME_BYTES || (!present && mime_len != 0) {
        return Err(invalid("mime_len"));
    }
    // Padding after the text is zero, so one request has one encoding.
    if text[mime_len..].iter().any(|byte| *byte != 0) {
        return Err(invalid("mime"));
    }
    let mime_type = present
        .then(|| std::str::from_utf8(&text[..mime_len]).map(str::to_owned))
        .transpose()
        .map_err(|_| BinaryCodecError::InvalidUtf8 { field: "mime" })?;
    Ok(PortalRequest {
        transfer: PortalTransfer {
            transfer,
            source_namespace,
            target_namespace,
            kind,
            mime_type,
            byte_size,
            decision: PortalDecision::Pending,
            generation,
        },
        deadline_msec,
    })
}

const DENIED: u8 = 1;
const ALLOWED: u8 = 2;
const GRANT_ACTIVE: u8 = 1;

/// Decision's body. Denied discloses nothing but the request's transfer,
/// which may be zero; Allowed carries one active grant for a nonzero
/// transfer.
pub fn encode_portal_decision(
    packet: &PortalBrokerResponsePacket,
) -> Result<Vec<u8>, BinaryCodecError> {
    let mut bytes = Vec::with_capacity(56);
    push_u64(&mut bytes, packet.transfer.raw());
    match &packet.decision {
        PortalBrokerResponseDecision::Denied => {
            push_u8(&mut bytes, DENIED);
            bytes.extend_from_slice(&[0; 47]);
        }
        PortalBrokerResponseDecision::Allowed(grant) => {
            validate_grant(packet.transfer, grant)?;
            push_u8(&mut bytes, ALLOWED);
            push_u8(&mut bytes, GRANT_ACTIVE);
            push_u16(&mut bytes, kind_value(grant.kind));
            push_u32(&mut bytes, 0);
            push_u64(&mut bytes, grant.source_namespace.raw());
            push_u64(&mut bytes, grant.target_namespace.raw());
            push_u64(&mut bytes, grant.source_generation);
            push_u64(&mut bytes, grant.broker_generation);
            push_u64(&mut bytes, grant.deadline_msec);
        }
    }
    Ok(bytes)
}

fn validate_grant(transfer: PortalTransferId, grant: &PortalGrant) -> Result<(), BinaryCodecError> {
    nonzero(transfer.raw(), "transfer")?;
    if grant.transfer != transfer {
        return Err(invalid("transfer"));
    }
    if grant.state != PortalGrantState::Active {
        return Err(invalid("grant_state"));
    }
    nonzero(grant.source_namespace.raw(), "source_namespace")?;
    nonzero(grant.target_namespace.raw(), "target_namespace")?;
    Ok(())
}

pub fn decode_portal_decision(
    bytes: &[u8],
) -> Result<PortalBrokerResponsePacket, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let transfer = PortalTransferId::from_raw(cursor.u64()?);
    let decision = cursor.u8()?;
    let rest = cursor.slice(47)?;
    cursor.finish()?;
    let decision = match decision {
        DENIED => {
            if rest.iter().any(|byte| *byte != 0) {
                return Err(invalid("denied_fields"));
            }
            PortalBrokerResponseDecision::Denied
        }
        ALLOWED => {
            let mut cursor = Cursor::new(rest);
            if cursor.u8()? != GRANT_ACTIVE {
                return Err(invalid("grant_state"));
            }
            let kind = decode_kind(cursor.u16()?)?;
            reserved(&mut cursor, 4)?;
            let grant = PortalGrant {
                transfer,
                source_namespace: NamespaceId::from_raw(cursor.u64()?),
                target_namespace: NamespaceId::from_raw(cursor.u64()?),
                kind,
                source_generation: cursor.u64()?,
                broker_generation: cursor.u64()?,
                deadline_msec: cursor.u64()?,
                state: PortalGrantState::Active,
            };
            cursor.finish()?;
            validate_grant(transfer, &grant)?;
            PortalBrokerResponseDecision::Allowed(grant)
        }
        _ => return Err(invalid("decision")),
    };
    Ok(PortalBrokerResponsePacket { transfer, decision })
}

/// Binds upload slot zero to one active grant and declares its length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalPayloadBegin {
    pub transfer: PortalTransferId,
    pub total_bytes: u64,
}

impl PortalPayloadBegin {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.transfer.raw(), "transfer")?;
        range(
            self.total_bytes,
            0,
            PORTAL_FILE_MAX_PAYLOAD_BYTES,
            "total_bytes",
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(24);
        push_u64(&mut bytes, self.transfer.raw());
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u64(&mut bytes, self.total_bytes);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let transfer = PortalTransferId::from_raw(cursor.u64()?);
        if cursor.u16()? != 0 {
            return Err(invalid("slot"));
        }
        reserved(&mut cursor, 6)?;
        let begin = Self {
            transfer,
            total_bytes: cursor.u64()?,
        };
        cursor.finish()?;
        begin.validate()?;
        Ok(begin)
    }
}

/// Ends the upload; its length must equal the begin's and the bytes written,
/// which the export checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalPayloadEnd {
    pub transfer: PortalTransferId,
    pub total_bytes: u64,
}

impl PortalPayloadEnd {
    fn validate(&self) -> Result<(), BinaryCodecError> {
        nonzero(self.transfer.raw(), "transfer")?;
        range(
            self.total_bytes,
            0,
            PORTAL_FILE_MAX_PAYLOAD_BYTES,
            "total_bytes",
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(16);
        push_u64(&mut bytes, self.transfer.raw());
        push_u64(&mut bytes, self.total_bytes);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let end = Self {
            transfer: PortalTransferId::from_raw(cursor.u64()?),
            total_bytes: cursor.u64()?,
        };
        cursor.finish()?;
        end.validate()?;
        Ok(end)
    }
}

pub fn encode_portal_transfer_cancel(
    transfer: PortalTransferId,
) -> Result<Vec<u8>, BinaryCodecError> {
    nonzero(transfer.raw(), "transfer")?;
    Ok(transfer.raw().to_le_bytes().to_vec())
}

pub fn decode_portal_transfer_cancel(bytes: &[u8]) -> Result<PortalTransferId, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let transfer = nonzero(cursor.u64()?, "transfer")?;
    cursor.finish()?;
    Ok(PortalTransferId::from_raw(transfer))
}

/// One terminal outcome: Completed means the executor succeeded; failure,
/// cancellation and revocation are Revoked; deadline expiry is Expired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalTransferOutcome {
    pub transfer: PortalTransferId,
    pub state: PortalGrantState,
}

impl PortalTransferOutcome {
    fn state_value(&self) -> Result<u8, BinaryCodecError> {
        Ok(match self.state {
            PortalGrantState::Completed => 2,
            PortalGrantState::Revoked => 3,
            PortalGrantState::Expired => 4,
            PortalGrantState::Active => return Err(invalid("state")),
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, BinaryCodecError> {
        nonzero(self.transfer.raw(), "transfer")?;
        let mut bytes = Vec::with_capacity(16);
        push_u64(&mut bytes, self.transfer.raw());
        push_u8(&mut bytes, self.state_value()?);
        bytes.extend_from_slice(&[0; 7]);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BinaryCodecError> {
        let mut cursor = Cursor::new(bytes);
        let transfer = PortalTransferId::from_raw(nonzero(cursor.u64()?, "transfer")?);
        let state = match cursor.u8()? {
            2 => PortalGrantState::Completed,
            3 => PortalGrantState::Revoked,
            4 => PortalGrantState::Expired,
            _ => return Err(invalid("state")),
        };
        reserved(&mut cursor, 7)?;
        cursor.finish()?;
        Ok(Self { transfer, state })
    }
}
