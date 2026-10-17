use super::{invalid, nonzero, reserved};
use crate::BinaryCodecError;
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};

pub const LOCK_FILE_API_VERSION: u16 = 1;
pub const LOCK_FILE_HEADER_BYTES: usize = 32;
pub const LOCK_FILE_MAX_BYTES: usize = 65_536;
/// The image-formats object is the one record allowed past the general
/// record bound: a 64-byte framing prefix plus at most 4096 format rows.
pub const LOCK_FILE_MAX_IMAGE_FORMATS_BYTES: usize = 163_904;
pub const LOCK_FILE_MAX_CANDIDATE_BYTES: usize = 128;
/// The smallest candidate: a header and the shortest candidate body.
const LOCK_FILE_MIN_CANDIDATE_BYTES: usize = 48;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockFileClass {
    Object,
    Candidate,
    Event,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum LockFileKind {
    Limits = 1,
    Lock = 2,
    Negotiated = 16,
    Refused = 17,
    Submitted = 18,
    ObjectPublished = 19,
    ResourceStatus = 33,
    ResourceReleased = 34,
    CandidateOutcome = 35,
    FramePermit = 36,
    Entry = 40,
    Chord = 41,
    Negotiate = 256,
    ResourceBegin = 258,
    ResourceEnd = 259,
    ResourceCancel = 260,
    ResourceRetire = 261,
    Candidate = 262,
    FrameDemand = 263,
    ImageBegin = 28_672,
    ImageAdmitted = 28_673,
    SourceReleased = 28_674,
    ChannelReady = 28_675,
    ChannelClosed = 28_676,
    ImageLimits = 28_677,
    ImageFormats = 28_678,
}

impl LockFileKind {
    pub const fn class(self) -> LockFileClass {
        match self {
            Self::Limits | Self::Lock | Self::ImageLimits | Self::ImageFormats => {
                LockFileClass::Object
            }
            Self::Negotiate
            | Self::ResourceBegin
            | Self::ResourceEnd
            | Self::ResourceCancel
            | Self::ResourceRetire
            | Self::Candidate
            | Self::FrameDemand
            | Self::ImageBegin => LockFileClass::Candidate,
            _ => LockFileClass::Event,
        }
    }

    pub fn decode(value: u16) -> Result<Self, BinaryCodecError> {
        Ok(match value {
            1 => Self::Limits,
            2 => Self::Lock,
            16 => Self::Negotiated,
            17 => Self::Refused,
            18 => Self::Submitted,
            19 => Self::ObjectPublished,
            33 => Self::ResourceStatus,
            34 => Self::ResourceReleased,
            35 => Self::CandidateOutcome,
            36 => Self::FramePermit,
            40 => Self::Entry,
            41 => Self::Chord,
            256 => Self::Negotiate,
            258 => Self::ResourceBegin,
            259 => Self::ResourceEnd,
            260 => Self::ResourceCancel,
            261 => Self::ResourceRetire,
            262 => Self::Candidate,
            263 => Self::FrameDemand,
            28_672 => Self::ImageBegin,
            28_673 => Self::ImageAdmitted,
            28_674 => Self::SourceReleased,
            28_675 => Self::ChannelReady,
            28_676 => Self::ChannelClosed,
            28_677 => Self::ImageLimits,
            28_678 => Self::ImageFormats,
            _ => return Err(BinaryCodecError::UnknownMessageKind(value)),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockFileHeader {
    pub kind: LockFileKind,
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockFileRecord<'a> {
    pub header: LockFileHeader,
    pub body: &'a [u8],
}

fn validate(header: LockFileHeader) -> Result<(), BinaryCodecError> {
    nonzero(header.connection_epoch, "connection_epoch")?;
    let valid = match header.kind.class() {
        LockFileClass::Object => header.submission_id == 0 && header.sequence == 0,
        LockFileClass::Candidate => header.submission_id != 0 && header.sequence == 0,
        LockFileClass::Event => header.submission_id == 0 && header.sequence != 0,
    };
    valid
        .then_some(())
        .ok_or_else(|| invalid("record_identity"))
}

fn check_revision(kind: LockFileKind, revision: u16) -> Result<(), BinaryCodecError> {
    match revision {
        1 | 2 => {}
        other => return Err(BinaryCodecError::UnsupportedVersion(other)),
    }
    let image = matches!(
        kind,
        LockFileKind::ImageBegin
            | LockFileKind::ImageAdmitted
            | LockFileKind::SourceReleased
            | LockFileKind::ChannelReady
            | LockFileKind::ChannelClosed
            | LockFileKind::ImageLimits
            | LockFileKind::ImageFormats
    );
    if image && revision < 2 {
        return Err(invalid("image_kind_revision"));
    }
    Ok(())
}

/// Decodes exactly one complete record of `class` under the original r1
/// vocabulary. Image kinds are refused here; they exist only through
/// [`decode_lock_file_record_at_revision`]. Its typed body must still be
/// decoded before anything acts on it.
pub fn decode_lock_file_record(
    bytes: &[u8],
    class: LockFileClass,
) -> Result<LockFileRecord<'_>, BinaryCodecError> {
    decode_lock_file_record_at_revision(bytes, class, 1)
}

/// Decodes one record under an explicit negotiated role revision. Revision
/// 1 is the existing vocabulary; revision 2 adds the image kinds. Any other
/// revision is refused, and no revision is inferred from the bytes.
pub fn decode_lock_file_record_at_revision(
    bytes: &[u8],
    class: LockFileClass,
    revision: u16,
) -> Result<LockFileRecord<'_>, BinaryCodecError> {
    // Only the image-formats object may pass the general record bound.
    let max_bytes = if bytes.len() >= 8
        && u16::from_le_bytes([bytes[6], bytes[7]]) == LockFileKind::ImageFormats as u16
    {
        LOCK_FILE_MAX_IMAGE_FORMATS_BYTES
    } else {
        LOCK_FILE_MAX_BYTES
    };
    if !(LOCK_FILE_HEADER_BYTES..=max_bytes).contains(&bytes.len()) {
        return Err(invalid("record_length"));
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.u32()? as usize != bytes.len() {
        return Err(invalid("record_length"));
    }
    let version = cursor.u16()?;
    if version != LOCK_FILE_API_VERSION {
        return Err(BinaryCodecError::UnsupportedVersion(version));
    }
    let header = LockFileHeader {
        kind: LockFileKind::decode(cursor.u16()?)?,
        connection_epoch: cursor.u64()?,
        submission_id: cursor.u64()?,
        sequence: cursor.u64()?,
    };
    validate(header)?;
    check_revision(header.kind, revision)?;
    if header.kind.class() != class {
        return Err(invalid("record_class"));
    }
    if class == LockFileClass::Candidate && bytes.len() > LOCK_FILE_MAX_CANDIDATE_BYTES {
        return Err(invalid("candidate_length"));
    }
    Ok(LockFileRecord {
        header,
        body: &bytes[LOCK_FILE_HEADER_BYTES..],
    })
}

/// Encodes one record under the original r1 vocabulary; image kinds are
/// refused, exactly as on decode.
pub fn encode_lock_file_record(
    header: LockFileHeader,
    body: &[u8],
) -> Result<Vec<u8>, BinaryCodecError> {
    encode_lock_file_record_at_revision(header, body, 1)
}

/// Encodes one record under an explicit negotiated role revision; see
/// [`decode_lock_file_record_at_revision`].
pub fn encode_lock_file_record_at_revision(
    header: LockFileHeader,
    body: &[u8],
    revision: u16,
) -> Result<Vec<u8>, BinaryCodecError> {
    validate(header)?;
    check_revision(header.kind, revision)?;
    let limit = if header.kind.class() == LockFileClass::Candidate {
        LOCK_FILE_MAX_CANDIDATE_BYTES
    } else if header.kind == LockFileKind::ImageFormats {
        LOCK_FILE_MAX_IMAGE_FORMATS_BYTES
    } else {
        LOCK_FILE_MAX_BYTES
    };
    let size = LOCK_FILE_HEADER_BYTES
        .checked_add(body.len())
        .filter(|size| *size <= limit)
        .ok_or_else(|| invalid("record_length"))?;
    let mut bytes = Vec::with_capacity(size);
    push_u32(&mut bytes, size as u32);
    push_u16(&mut bytes, LOCK_FILE_API_VERSION);
    push_u16(&mut bytes, header.kind as u16);
    push_u64(&mut bytes, header.connection_epoch);
    push_u64(&mut bytes, header.submission_id);
    push_u64(&mut bytes, header.sequence);
    bytes.extend_from_slice(body);
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockFileSubmit {
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub candidate_bytes: u32,
}

impl LockFileSubmit {
    fn validate(self) -> Result<(), BinaryCodecError> {
        nonzero(self.connection_epoch, "connection_epoch")?;
        nonzero(self.submission_id, "submission_id")?;
        if !(LOCK_FILE_MIN_CANDIDATE_BYTES..=LOCK_FILE_MAX_CANDIDATE_BYTES)
            .contains(&(self.candidate_bytes as usize))
        {
            return Err(invalid("candidate_length"));
        }
        Ok(())
    }
}

pub fn decode_lock_file_submit(bytes: &[u8]) -> Result<LockFileSubmit, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let submit = LockFileSubmit {
        connection_epoch: cursor.u64()?,
        submission_id: cursor.u64()?,
        candidate_bytes: cursor.u32()?,
    };
    reserved(&mut cursor, 4)?;
    cursor.finish()?;
    submit.validate()?;
    Ok(submit)
}

pub fn encode_lock_file_submit(submit: LockFileSubmit) -> Result<[u8; 24], BinaryCodecError> {
    submit.validate()?;
    let mut bytes = [0; 24];
    bytes[..8].copy_from_slice(&submit.connection_epoch.to_le_bytes());
    bytes[8..16].copy_from_slice(&submit.submission_id.to_le_bytes());
    bytes[16..20].copy_from_slice(&submit.candidate_bytes.to_le_bytes());
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockFileAck {
    pub connection_epoch: u64,
    pub sequence: u64,
}

pub fn decode_lock_file_ack(bytes: &[u8]) -> Result<LockFileAck, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let ack = LockFileAck {
        connection_epoch: nonzero(cursor.u64()?, "connection_epoch")?,
        sequence: nonzero(cursor.u64()?, "sequence")?,
    };
    cursor.finish()?;
    Ok(ack)
}

pub fn encode_lock_file_ack(ack: LockFileAck) -> Result<[u8; 16], BinaryCodecError> {
    nonzero(ack.connection_epoch, "connection_epoch")?;
    nonzero(ack.sequence, "sequence")?;
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&ack.connection_epoch.to_le_bytes());
    bytes[8..].copy_from_slice(&ack.sequence.to_le_bytes());
    Ok(bytes)
}
