use super::{OutputFileKind, invalid, nonzero, reserved};
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};
use crate::{
    BinaryCodecError, MAX_OUTPUT_AUTHORITY_GROUPS, MAX_OUTPUT_AUTHORITY_HEADS,
    MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP, MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD,
    OutputV1ServerWelcome, SOPHIA_OUTPUT_CAPABILITY_CONFIGURE, SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
    SOPHIA_OUTPUT_INTERFACE_REVISION,
};

fn validate_welcome(welcome: OutputV1ServerWelcome) -> Result<(), BinaryCodecError> {
    nonzero(welcome.connection_epoch, "connection_epoch")?;
    let supported = SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE;
    if welcome.selected_revision != SOPHIA_OUTPUT_INTERFACE_REVISION
        || welcome.capabilities & SOPHIA_OUTPUT_CAPABILITY_OBSERVE == 0
        || welcome.capabilities & !supported != 0
        || usize::from(welcome.max_heads) != MAX_OUTPUT_AUTHORITY_HEADS
        || usize::from(welcome.max_groups) != MAX_OUTPUT_AUTHORITY_GROUPS
        || usize::from(welcome.max_modes_per_head) != MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD
        || usize::from(welcome.max_heads_per_group) != MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP
    {
        return Err(invalid("negotiated"));
    }
    Ok(())
}

/// The admitted epoch comes from the event header. Matching the selected
/// revision/capabilities against the actual request belongs to the client.
pub fn decode_output_file_negotiated(
    bytes: &[u8],
    connection_epoch: u64,
) -> Result<OutputV1ServerWelcome, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let selected_revision = cursor.u16()?;
    reserved(&mut cursor, 6)?;
    let welcome = OutputV1ServerWelcome {
        selected_revision,
        connection_epoch,
        capabilities: cursor.u64()?,
        max_heads: cursor.u16()?,
        max_groups: cursor.u16()?,
        max_modes_per_head: cursor.u16()?,
        max_heads_per_group: cursor.u16()?,
    };
    cursor.finish()?;
    validate_welcome(welcome)?;
    Ok(welcome)
}

pub fn encode_output_file_negotiated(
    welcome: OutputV1ServerWelcome,
) -> Result<Vec<u8>, BinaryCodecError> {
    validate_welcome(welcome)?;
    let mut bytes = Vec::with_capacity(24);
    push_u16(&mut bytes, welcome.selected_revision);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u64(&mut bytes, welcome.capabilities);
    push_u16(&mut bytes, welcome.max_heads);
    push_u16(&mut bytes, welcome.max_groups);
    push_u16(&mut bytes, welcome.max_modes_per_head);
    push_u16(&mut bytes, welcome.max_heads_per_group);
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum OutputFileRefusal {
    UnsupportedRevision = 1,
    ObservationRequired = 2,
}

pub fn decode_output_file_refused(bytes: &[u8]) -> Result<OutputFileRefusal, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let refusal = match cursor.u16()? {
        1 => OutputFileRefusal::UnsupportedRevision,
        2 => OutputFileRefusal::ObservationRequired,
        _ => return Err(invalid("negotiation_refusal")),
    };
    reserved(&mut cursor, 6)?;
    cursor.finish()?;
    Ok(refusal)
}

pub fn encode_output_file_refused(refusal: OutputFileRefusal) -> [u8; 8] {
    let mut bytes = [0; 8];
    bytes[..2].copy_from_slice(&(refusal as u16).to_le_bytes());
    bytes
}

/// Announces one immutable Topology object. Generation is its topology epoch;
/// the Qid path identifies the exact bytes retained by the export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFilePublication {
    pub topology_epoch: u64,
    pub qid_path: u64,
}

pub fn decode_output_file_publication(
    bytes: &[u8],
) -> Result<OutputFilePublication, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.u16()? != OutputFileKind::Topology as u16 {
        return Err(invalid("object_kind"));
    }
    reserved(&mut cursor, 6)?;
    let publication = OutputFilePublication {
        topology_epoch: nonzero(cursor.u64()?, "topology_epoch")?,
        qid_path: nonzero(cursor.u64()?, "qid_path")?,
    };
    cursor.finish()?;
    Ok(publication)
}

pub fn encode_output_file_publication(
    publication: OutputFilePublication,
) -> Result<[u8; 24], BinaryCodecError> {
    nonzero(publication.topology_epoch, "topology_epoch")?;
    nonzero(publication.qid_path, "qid_path")?;
    let mut bytes = [0; 24];
    bytes[..2].copy_from_slice(&(OutputFileKind::Topology as u16).to_le_bytes());
    bytes[8..16].copy_from_slice(&publication.topology_epoch.to_le_bytes());
    bytes[16..].copy_from_slice(&publication.qid_path.to_le_bytes());
    Ok(bytes)
}
