use super::{OutputFileKind, invalid, nonzero, reserved};
use crate::byte_cursor::{Cursor, push_u16, push_u32, push_u64};
use crate::{
    BinaryCodecError, OutputV1ClientHello, OutputV1Outcome, OutputV1OutcomeKind, TransactionId,
};

/// Revision and capability refusal belongs to negotiation. In particular,
/// unknown requested capability bits survive this decode for intersection by
/// the owner, just as they do on the existing output role.
pub fn decode_output_file_negotiate(bytes: &[u8]) -> Result<OutputV1ClientHello, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let minimum_revision = cursor.u16()?;
    let maximum_revision = cursor.u16()?;
    reserved(&mut cursor, 4)?;
    let capabilities = cursor.u64()?;
    cursor.finish()?;
    Ok(OutputV1ClientHello {
        minimum_revision,
        maximum_revision,
        capabilities,
    })
}

pub fn encode_output_file_negotiate(hello: OutputV1ClientHello) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..2].copy_from_slice(&hello.minimum_revision.to_le_bytes());
    bytes[2..4].copy_from_slice(&hello.maximum_revision.to_le_bytes());
    bytes[8..].copy_from_slice(&hello.capabilities.to_le_bytes());
    bytes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFileSubmitted {
    pub submission_id: u64,
    pub candidate_kind: OutputFileKind,
}

pub fn decode_output_file_submitted(bytes: &[u8]) -> Result<OutputFileSubmitted, BinaryCodecError> {
    let mut cursor = Cursor::new(bytes);
    let submitted = OutputFileSubmitted {
        submission_id: nonzero(cursor.u64()?, "submission_id")?,
        candidate_kind: OutputFileKind::decode(cursor.u16()?)?,
    };
    reserved(&mut cursor, 6)?;
    cursor.finish()?;
    validate_submitted(submitted)?;
    Ok(submitted)
}

fn validate_submitted(submitted: OutputFileSubmitted) -> Result<(), BinaryCodecError> {
    nonzero(submitted.submission_id, "submission_id")?;
    if !matches!(
        submitted.candidate_kind,
        OutputFileKind::Negotiate | OutputFileKind::Proposal
    ) {
        return Err(invalid("candidate_kind"));
    }
    Ok(())
}

pub fn encode_output_file_submitted(
    submitted: OutputFileSubmitted,
) -> Result<[u8; 16], BinaryCodecError> {
    validate_submitted(submitted)?;
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&submitted.submission_id.to_le_bytes());
    bytes[8..10].copy_from_slice(&(submitted.candidate_kind as u16).to_le_bytes());
    Ok(bytes)
}

/// The epoch is taken from the already decoded event envelope. The topology
/// transaction is distinct from its file submission ID and journal sequence.
pub fn decode_output_file_outcome(
    bytes: &[u8],
    connection_epoch: u64,
) -> Result<(TransactionId, OutputV1Outcome), BinaryCodecError> {
    nonzero(connection_epoch, "connection_epoch")?;
    let mut cursor = Cursor::new(bytes);
    let transaction = TransactionId::from_raw(nonzero(cursor.u64()?, "transaction")?);
    let topology_epoch = nonzero(cursor.u64()?, "topology_epoch")?;
    let kind = match cursor.u16()? {
        1 => OutputV1OutcomeKind::Validated,
        2 => OutputV1OutcomeKind::Committed,
        3 => OutputV1OutcomeKind::Stale,
        4 => OutputV1OutcomeKind::Rejected,
        5 => OutputV1OutcomeKind::RolledBack,
        6 => OutputV1OutcomeKind::Failed,
        value => {
            return Err(BinaryCodecError::InvalidEnum {
                field: "outcome",
                value: value.into(),
            });
        }
    };
    let reason = cursor.u16()?;
    reserved(&mut cursor, 4)?;
    cursor.finish()?;
    Ok((
        transaction,
        OutputV1Outcome {
            connection_epoch,
            topology_epoch,
            kind,
            reason,
        },
    ))
}

pub fn encode_output_file_outcome(
    transaction: TransactionId,
    outcome: OutputV1Outcome,
) -> Result<Vec<u8>, BinaryCodecError> {
    nonzero(transaction.raw(), "transaction")?;
    nonzero(outcome.connection_epoch, "connection_epoch")?;
    nonzero(outcome.topology_epoch, "topology_epoch")?;
    let mut bytes = Vec::with_capacity(24);
    push_u64(&mut bytes, transaction.raw());
    push_u64(&mut bytes, outcome.topology_epoch);
    push_u16(
        &mut bytes,
        match outcome.kind {
            OutputV1OutcomeKind::Validated => 1,
            OutputV1OutcomeKind::Committed => 2,
            OutputV1OutcomeKind::Stale => 3,
            OutputV1OutcomeKind::Rejected => 4,
            OutputV1OutcomeKind::RolledBack => 5,
            OutputV1OutcomeKind::Failed => 6,
        },
    );
    push_u16(&mut bytes, outcome.reason);
    push_u32(&mut bytes, 0);
    Ok(bytes)
}
