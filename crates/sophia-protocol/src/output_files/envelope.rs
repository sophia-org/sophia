use super::{invalid, nonzero, reserved};
use crate::BinaryCodecError;
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};

pub const OUTPUT_FILE_API_VERSION: u16 = 1;
pub const OUTPUT_FILE_HEADER_BYTES: usize = 32;
pub const OUTPUT_FILE_MAX_BYTES: usize = 65_536;
pub const OUTPUT_FILE_MAX_CANDIDATE_BYTES: usize = 1_784;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFileClass {
    Object,
    Candidate,
    Event,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum OutputFileKind {
    Limits = 1,
    Topology = 2,
    Negotiated = 16,
    Refused = 17,
    Submitted = 18,
    ObjectPublished = 19,
    Outcome = 32,
    Negotiate = 256,
    Proposal = 257,
}

impl OutputFileKind {
    pub const fn class(self) -> OutputFileClass {
        match self {
            Self::Limits | Self::Topology => OutputFileClass::Object,
            Self::Negotiate | Self::Proposal => OutputFileClass::Candidate,
            _ => OutputFileClass::Event,
        }
    }

    pub(super) fn decode(value: u16) -> Result<Self, BinaryCodecError> {
        Ok(match value {
            1 => Self::Limits,
            2 => Self::Topology,
            16 => Self::Negotiated,
            17 => Self::Refused,
            18 => Self::Submitted,
            19 => Self::ObjectPublished,
            32 => Self::Outcome,
            256 => Self::Negotiate,
            257 => Self::Proposal,
            _ => return Err(BinaryCodecError::UnknownMessageKind(value)),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFileHeader {
    pub kind: OutputFileKind,
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFileRecord<'a> {
    pub header: OutputFileHeader,
    pub body: &'a [u8],
}

fn validate(header: OutputFileHeader) -> Result<(), BinaryCodecError> {
    nonzero(header.connection_epoch, "connection_epoch")?;
    let valid = match header.kind.class() {
        OutputFileClass::Object => header.submission_id == 0 && header.sequence == 0,
        OutputFileClass::Candidate => header.submission_id != 0 && header.sequence == 0,
        OutputFileClass::Event => header.submission_id == 0 && header.sequence != 0,
    };
    valid
        .then_some(())
        .ok_or_else(|| invalid("record_identity"))
}

/// Decode exactly one complete record. The caller must also decode its typed
/// body before admission; the envelope alone does not validate body semantics.
pub fn decode_output_file_record(
    bytes: &[u8],
    class: OutputFileClass,
) -> Result<OutputFileRecord<'_>, BinaryCodecError> {
    if !(OUTPUT_FILE_HEADER_BYTES..=OUTPUT_FILE_MAX_BYTES).contains(&bytes.len()) {
        return Err(invalid("record_length"));
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.u32()? as usize != bytes.len() {
        return Err(invalid("record_length"));
    }
    let version = cursor.u16()?;
    if version != OUTPUT_FILE_API_VERSION {
        return Err(BinaryCodecError::UnsupportedVersion(version));
    }
    let header = OutputFileHeader {
        kind: OutputFileKind::decode(cursor.u16()?)?,
        connection_epoch: cursor.u64()?,
        submission_id: cursor.u64()?,
        sequence: cursor.u64()?,
    };
    validate(header)?;
    if header.kind.class() != class {
        return Err(invalid("record_class"));
    }
    if class == OutputFileClass::Candidate && bytes.len() > OUTPUT_FILE_MAX_CANDIDATE_BYTES {
        return Err(invalid("candidate_length"));
    }
    Ok(OutputFileRecord {
        header,
        body: &bytes[OUTPUT_FILE_HEADER_BYTES..],
    })
}

pub fn encode_output_file_record(
    header: OutputFileHeader,
    body: &[u8],
) -> Result<Vec<u8>, BinaryCodecError> {
    validate(header)?;
    let limit = if header.kind.class() == OutputFileClass::Candidate {
        OUTPUT_FILE_MAX_CANDIDATE_BYTES
    } else {
        OUTPUT_FILE_MAX_BYTES
    };
    let size = OUTPUT_FILE_HEADER_BYTES
        .checked_add(body.len())
        .filter(|size| *size <= limit)
        .ok_or_else(|| invalid("record_length"))?;
    let mut bytes = Vec::with_capacity(size);
    push_u32(&mut bytes, size as u32);
    push_u16(&mut bytes, OUTPUT_FILE_API_VERSION);
    push_u16(&mut bytes, header.kind as u16);
    push_u64(&mut bytes, header.connection_epoch);
    push_u64(&mut bytes, header.submission_id);
    push_u64(&mut bytes, header.sequence);
    bytes.extend_from_slice(body);
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFileSubmit {
    pub connection_epoch: u64,
    pub submission_id: u64,
    pub candidate_bytes: u32,
}

impl OutputFileSubmit {
    fn validate(self) -> Result<(), BinaryCodecError> {
        nonzero(self.connection_epoch, "connection_epoch")?;
        nonzero(self.submission_id, "submission_id")?;
        if !(48..=OUTPUT_FILE_MAX_CANDIDATE_BYTES).contains(&(self.candidate_bytes as usize)) {
            return Err(invalid("candidate_length"));
        }
        Ok(())
    }
}

pub fn decode_output_file_submit(bytes: &[u8]) -> Result<OutputFileSubmit, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let submit = OutputFileSubmit {
        connection_epoch: cursor.u64()?,
        submission_id: cursor.u64()?,
        candidate_bytes: cursor.u32()?,
    };
    reserved(&mut cursor, 4)?;
    cursor.finish()?;
    submit.validate()?;
    Ok(submit)
}

pub fn encode_output_file_submit(submit: OutputFileSubmit) -> Result<[u8; 24], BinaryCodecError> {
    submit.validate()?;
    let mut bytes = [0; 24];
    bytes[..8].copy_from_slice(&submit.connection_epoch.to_le_bytes());
    bytes[8..16].copy_from_slice(&submit.submission_id.to_le_bytes());
    bytes[16..20].copy_from_slice(&submit.candidate_bytes.to_le_bytes());
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFileAck {
    pub connection_epoch: u64,
    pub sequence: u64,
}

pub fn decode_output_file_ack(bytes: &[u8]) -> Result<OutputFileAck, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let ack = OutputFileAck {
        connection_epoch: nonzero(cursor.u64()?, "connection_epoch")?,
        sequence: nonzero(cursor.u64()?, "sequence")?,
    };
    cursor.finish()?;
    Ok(ack)
}

pub fn encode_output_file_ack(ack: OutputFileAck) -> Result<[u8; 16], BinaryCodecError> {
    nonzero(ack.connection_epoch, "connection_epoch")?;
    nonzero(ack.sequence, "sequence")?;
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&ack.connection_epoch.to_le_bytes());
    bytes[8..].copy_from_slice(&ack.sequence.to_le_bytes());
    Ok(bytes)
}
