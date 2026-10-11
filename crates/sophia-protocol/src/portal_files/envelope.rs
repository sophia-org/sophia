use super::{invalid, nonzero, reserved};
use crate::BinaryCodecError;
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};

pub const PORTAL_FILE_API_VERSION: u16 = 1;
pub const PORTAL_FILE_HEADER_BYTES: usize = 32;
/// TransferCancel, the shortest candidate.
pub const PORTAL_FILE_MIN_CANDIDATE_BYTES: usize = 40;
/// TransferRequest, the longest candidate and the staging size.
pub const PORTAL_FILE_MAX_CANDIDATE_BYTES: usize = 344;
pub const PORTAL_FILE_MAX_OBJECT_BYTES: usize = 80;
pub const PORTAL_FILE_MAX_EVENT_BYTES: usize = 88;
pub const PORTAL_FILE_SUBMIT_BYTES: usize = 24;
pub const PORTAL_FILE_ACK_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalFileClass {
    Object,
    Candidate,
    Event,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum PortalFileKind {
    Limits = 1,
    Negotiated = 16,
    Refused = 17,
    Submitted = 18,
    Decision = 32,
    TransferOutcome = 33,
    Negotiate = 256,
    TransferRequest = 257,
    PayloadBegin = 258,
    PayloadEnd = 259,
    TransferCancel = 260,
}

impl PortalFileKind {
    pub const fn class(self) -> PortalFileClass {
        match self {
            Self::Limits => PortalFileClass::Object,
            Self::Negotiate
            | Self::TransferRequest
            | Self::PayloadBegin
            | Self::PayloadEnd
            | Self::TransferCancel => PortalFileClass::Candidate,
            Self::Negotiated
            | Self::Refused
            | Self::Submitted
            | Self::Decision
            | Self::TransferOutcome => PortalFileClass::Event,
        }
    }

    /// Every portal body has one fixed size, so a record's length is fully
    /// determined by its kind.
    pub const fn body_bytes(self) -> usize {
        match self {
            Self::Limits => 48,
            Self::Negotiated | Self::Submitted | Self::TransferOutcome | Self::PayloadEnd => 16,
            Self::Refused | Self::Negotiate | Self::TransferCancel => 8,
            Self::Decision => 56,
            Self::TransferRequest => 312,
            Self::PayloadBegin => 24,
        }
    }

    pub const fn record_bytes(self) -> usize {
        PORTAL_FILE_HEADER_BYTES + self.body_bytes()
    }

    pub fn decode(value: u16) -> Result<Self, BinaryCodecError> {
        Ok(match value {
            1 => Self::Limits,
            16 => Self::Negotiated,
            17 => Self::Refused,
            18 => Self::Submitted,
            32 => Self::Decision,
            33 => Self::TransferOutcome,
            256 => Self::Negotiate,
            257 => Self::TransferRequest,
            258 => Self::PayloadBegin,
            259 => Self::PayloadEnd,
            260 => Self::TransferCancel,
            _ => return Err(BinaryCodecError::UnknownMessageKind(value)),
        })
    }
}

/// Why a portal file write or record was refused. The export maps `Invalid`
/// to EINVAL and `StaleEpoch` to ESTALE, both before any custody.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalFileError {
    Invalid(BinaryCodecError),
    StaleEpoch,
}

impl From<BinaryCodecError> for PortalFileError {
    fn from(error: BinaryCodecError) -> Self {
        Self::Invalid(error)
    }
}

impl From<crate::byte_cursor::CursorError> for PortalFileError {
    fn from(error: crate::byte_cursor::CursorError) -> Self {
        Self::Invalid(error.into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalFileHeader {
    pub kind: PortalFileKind,
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub sequence: u64,
}

fn validate(header: PortalFileHeader) -> Result<(), BinaryCodecError> {
    nonzero(header.connection_epoch, "connection_epoch")?;
    let valid = match header.kind.class() {
        PortalFileClass::Object => header.submission_id == 0 && header.sequence == 0,
        PortalFileClass::Candidate => header.submission_id != 0 && header.sequence == 0,
        PortalFileClass::Event => header.submission_id == 0 && header.sequence != 0,
    };
    valid
        .then_some(())
        .ok_or_else(|| invalid("record_identity"))
}

/// Decodes one complete record of `class` written for `epoch`, returning its
/// header and exact-length body.
///
/// The envelope is judged first. A well-formed record from another epoch is
/// then refused as stale without its body being interpreted.
pub(super) fn decode_record(
    bytes: &[u8],
    class: PortalFileClass,
    epoch: u64,
) -> Result<(PortalFileHeader, &[u8]), PortalFileError> {
    if bytes.len() < PORTAL_FILE_HEADER_BYTES {
        return Err(BinaryCodecError::Truncated.into());
    }
    let mut cursor = Cursor::new(bytes);
    let total = cursor.u32()? as usize;
    let version = cursor.u16()?;
    if version != PORTAL_FILE_API_VERSION {
        return Err(BinaryCodecError::UnsupportedVersion(version).into());
    }
    let header = PortalFileHeader {
        kind: PortalFileKind::decode(cursor.u16()?)?,
        connection_epoch: cursor.u64()?,
        submission_id: cursor.u64()?,
        sequence: cursor.u64()?,
    };
    if header.kind.class() != class {
        return Err(invalid("record_class").into());
    }
    if total != bytes.len() || total != header.kind.record_bytes() {
        return Err(invalid("record_length").into());
    }
    validate(header)?;
    if header.connection_epoch != epoch {
        return Err(PortalFileError::StaleEpoch);
    }
    Ok((header, &bytes[PORTAL_FILE_HEADER_BYTES..]))
}

pub(super) fn encode_record(
    header: PortalFileHeader,
    body: &[u8],
) -> Result<Vec<u8>, BinaryCodecError> {
    validate(header)?;
    let size = header.kind.record_bytes();
    if body.len() != header.kind.body_bytes() {
        return Err(invalid("record_length"));
    }
    let mut bytes = Vec::with_capacity(size);
    push_u32(&mut bytes, size as u32);
    push_u16(&mut bytes, PORTAL_FILE_API_VERSION);
    push_u16(&mut bytes, header.kind as u16);
    push_u64(&mut bytes, header.connection_epoch);
    push_u64(&mut bytes, header.submission_id);
    push_u64(&mut bytes, header.sequence);
    bytes.extend_from_slice(body);
    Ok(bytes)
}

/// The `api` file's single line for one admitted connection epoch.
pub fn portal_file_api_line(epoch: u64) -> String {
    format!("sophia-portal-files version=1 role=portal-requester epoch={epoch} fd_transfer=none\n")
}

/// The exact `submit` control: which staged candidate, of how many bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalFileSubmit {
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub candidate_bytes: u32,
}

impl PortalFileSubmit {
    fn validate(self) -> Result<(), BinaryCodecError> {
        nonzero(self.connection_epoch, "connection_epoch")?;
        nonzero(self.submission_id, "submission_id")?;
        if !(PORTAL_FILE_MIN_CANDIDATE_BYTES..=PORTAL_FILE_MAX_CANDIDATE_BYTES)
            .contains(&(self.candidate_bytes as usize))
        {
            return Err(invalid("candidate_bytes"));
        }
        Ok(())
    }
}

pub fn decode_portal_file_submit(
    bytes: &[u8],
    epoch: u64,
) -> Result<PortalFileSubmit, PortalFileError> {
    let mut cursor = Cursor::new(bytes);
    let submit = PortalFileSubmit {
        connection_epoch: cursor.u64()?,
        submission_id: cursor.u64()?,
        candidate_bytes: cursor.u32()?,
    };
    reserved(&mut cursor, 4)?;
    cursor.finish()?;
    submit.validate()?;
    if submit.connection_epoch != epoch {
        return Err(PortalFileError::StaleEpoch);
    }
    Ok(submit)
}

pub fn encode_portal_file_submit(
    submit: PortalFileSubmit,
) -> Result<[u8; PORTAL_FILE_SUBMIT_BYTES], BinaryCodecError> {
    submit.validate()?;
    let mut bytes = [0; PORTAL_FILE_SUBMIT_BYTES];
    bytes[..8].copy_from_slice(&submit.connection_epoch.to_le_bytes());
    bytes[8..16].copy_from_slice(&submit.submission_id.to_le_bytes());
    bytes[16..20].copy_from_slice(&submit.candidate_bytes.to_le_bytes());
    Ok(bytes)
}

/// The cumulative `ack` control: every event up to `sequence` was read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalFileAck {
    pub connection_epoch: u64,
    pub sequence: u64,
}

pub fn decode_portal_file_ack(bytes: &[u8], epoch: u64) -> Result<PortalFileAck, PortalFileError> {
    let mut cursor = Cursor::new(bytes);
    let ack = PortalFileAck {
        connection_epoch: nonzero(cursor.u64()?, "connection_epoch")?,
        sequence: nonzero(cursor.u64()?, "sequence")?,
    };
    cursor.finish()?;
    if ack.connection_epoch != epoch {
        return Err(PortalFileError::StaleEpoch);
    }
    Ok(ack)
}

pub fn encode_portal_file_ack(
    ack: PortalFileAck,
) -> Result<[u8; PORTAL_FILE_ACK_BYTES], BinaryCodecError> {
    nonzero(ack.connection_epoch, "connection_epoch")?;
    nonzero(ack.sequence, "sequence")?;
    let mut bytes = [0; PORTAL_FILE_ACK_BYTES];
    bytes[..8].copy_from_slice(&ack.connection_epoch.to_le_bytes());
    bytes[8..].copy_from_slice(&ack.sequence.to_le_bytes());
    Ok(bytes)
}
