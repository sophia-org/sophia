use super::fields::{Wire, reserved};
use super::*;
use crate::ipc::cursor::Cursor;
use crate::{IpcCodecError, IpcMessageKind, TransactionId, decode_frame, encode_frame};

/// Decode one bounded frame; permission and lifecycle validation remain with
/// the owner. Nonzero reserved bytes and trailing payloads are rejected.
pub(crate) fn decode_shell_content_payload(
    kind: IpcMessageKind,
    transaction: TransactionId,
    payload: &[u8],
) -> Result<ShellContentRecord, IpcCodecError> {
    let mut cursor = Cursor::new(payload);
    let record = match kind {
        IpcMessageKind::ShellContentAdmissionRefused => {
            ShellContentRecord::AdmissionRefused(ContentAdmissionRefused::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentLimits => {
            ShellContentRecord::Limits(ContentLimits::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentOutputFacts => {
            ShellContentRecord::OutputFacts(ContentOutputFacts::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentAllocationRequest => {
            ShellContentRecord::AllocationRequest(ContentAllocationRequest::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentAllocationResult => {
            ShellContentRecord::AllocationResult(ContentAllocationResult::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentResourceBegin => {
            ShellContentRecord::ResourceBegin(ContentResourceBegin::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentResourceStatus => {
            ShellContentRecord::ResourceStatus(ContentResourceStatus::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentResourceChunk => {
            ShellContentRecord::ResourceChunk(ContentResourceChunk::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentResourceEnd => {
            ShellContentRecord::ResourceEnd(ContentResourceEnd::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentResourceCancel => {
            ShellContentRecord::ResourceCancel(ContentResourceCancel::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentResourceRetire => {
            ShellContentRecord::ResourceRetire(ContentResourceRetire::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentResourceReleased => {
            ShellContentRecord::ResourceReleased(ContentResourceReleased::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentCandidateBegin => {
            ShellContentRecord::CandidateBegin(ContentCandidateBegin::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentCandidateChunk => {
            ShellContentRecord::CandidateChunk(ContentCandidateChunk::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentCandidateEnd => {
            ShellContentRecord::CandidateEnd(ContentCandidateEnd::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentCandidateOutcome => {
            ShellContentRecord::CandidateOutcome(ContentCandidateOutcome::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentFrameDemand => {
            ShellContentRecord::FrameDemand(ContentFrameDemand::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentFramePermit => {
            ShellContentRecord::FramePermit(ContentFramePermit::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentFrameDemandCancel => {
            ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentAction => {
            ShellContentRecord::Action(ContentAction::take(&mut cursor)?)
        }
        IpcMessageKind::ShellContentActionAck => {
            ShellContentRecord::ActionAck(ContentActionAck::take(&mut cursor)?)
        }
        _ => return Err(IpcCodecError::InvalidRecord("not a shell content record")),
    };
    cursor.finish()?;
    validate_transaction(transaction, &record)?;
    super::validation::validate(&record)?;
    Ok(record)
}

pub fn decode_shell_content_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellContentRecord), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    let record = decode_shell_content_payload(header.message_kind, header.transaction, payload)?;
    Ok((header.transaction, record))
}

pub(crate) fn encode_shell_content_payload(
    transaction: TransactionId,
    record: &ShellContentRecord,
) -> Result<(IpcMessageKind, Vec<u8>), IpcCodecError> {
    validate_transaction(transaction, record)?;
    super::validation::validate(record)?;
    let mut bytes = Vec::new();
    let kind = match record {
        ShellContentRecord::AdmissionRefused(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentAdmissionRefused
        }
        ShellContentRecord::Limits(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentLimits
        }
        ShellContentRecord::OutputFacts(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentOutputFacts
        }
        ShellContentRecord::AllocationRequest(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentAllocationRequest
        }
        ShellContentRecord::AllocationResult(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentAllocationResult
        }
        ShellContentRecord::ResourceBegin(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentResourceBegin
        }
        ShellContentRecord::ResourceStatus(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentResourceStatus
        }
        ShellContentRecord::ResourceChunk(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentResourceChunk
        }
        ShellContentRecord::ResourceEnd(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentResourceEnd
        }
        ShellContentRecord::ResourceCancel(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentResourceCancel
        }
        ShellContentRecord::ResourceRetire(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentResourceRetire
        }
        ShellContentRecord::ResourceReleased(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentResourceReleased
        }
        ShellContentRecord::CandidateBegin(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentCandidateBegin
        }
        ShellContentRecord::CandidateChunk(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentCandidateChunk
        }
        ShellContentRecord::CandidateEnd(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentCandidateEnd
        }
        ShellContentRecord::CandidateOutcome(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentCandidateOutcome
        }
        ShellContentRecord::FrameDemand(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentFrameDemand
        }
        ShellContentRecord::FramePermit(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentFramePermit
        }
        ShellContentRecord::FrameDemandCancel(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentFrameDemandCancel
        }
        ShellContentRecord::Action(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentAction
        }
        ShellContentRecord::ActionAck(value) => {
            value.put(&mut bytes);
            IpcMessageKind::ShellContentActionAck
        }
    };
    Ok((kind, bytes))
}

pub fn encode_shell_content_frame(
    transaction: TransactionId,
    record: &ShellContentRecord,
) -> Result<Vec<u8>, IpcCodecError> {
    let (kind, bytes) = encode_shell_content_payload(transaction, record)?;
    encode_frame(kind, transaction, &bytes)
}

fn validate_transaction(
    transaction: TransactionId,
    record: &ShellContentRecord,
) -> Result<(), IpcCodecError> {
    let zero = matches!(
        record,
        ShellContentRecord::AdmissionRefused(_) | ShellContentRecord::Limits(_)
    );
    if (transaction.raw() == 0) != zero {
        return Err(IpcCodecError::InvalidTransaction(transaction.raw()));
    }
    Ok(())
}

fn count(cursor: &mut Cursor<'_>, maximum: usize) -> Result<usize, IpcCodecError> {
    let value = cursor.u32()? as usize;
    if value > maximum {
        return Err(IpcCodecError::CountTooLarge {
            count: value,
            max: maximum,
        });
    }
    Ok(value)
}
fn take_table<T: Wire>(cursor: &mut Cursor<'_>, count: usize) -> Result<Vec<T>, IpcCodecError> {
    (0..count).map(|_| T::take(cursor)).collect()
}
impl Wire for ContentOutputFacts {
    fn put(&self, b: &mut Vec<u8>) {
        self.grant.put(b);
        self.facts_generation.put(b);
        (self.outputs.len() as u32).put(b);
        0u32.put(b);
        for row in &self.outputs {
            row.put(b);
        }
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(c)?;
        let facts_generation = c.u64()?;
        let count = count(c, 16)?;
        reserved::<u32>(c)?;
        Ok(Self {
            grant,
            facts_generation,
            outputs: take_table(c, count)?,
        })
    }
}
impl Wire for ContentResourceChunk {
    fn put(&self, b: &mut Vec<u8>) {
        self.grant.put(b);
        self.resource.put(b);
        self.ordinal.put(b);
        (self.bytes.len() as u32).put(b);
        self.offset.put(b);
        b.extend_from_slice(&self.bytes);
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(c)?;
        let resource = ContentResourceId::take(c)?;
        let ordinal = c.u32()?;
        let length = count(c, 65488)?;
        let offset = c.u64()?;
        let bytes = c.slice(length)?.to_vec();
        Ok(Self {
            grant,
            resource,
            ordinal,
            offset,
            bytes,
        })
    }
}
impl Wire for ContentCandidateChunk {
    fn put(&self, b: &mut Vec<u8>) {
        self.grant.put(b);
        self.candidate_generation.put(b);
        self.chunk_ordinal.put(b);
        (self.surfaces.len() as u32).put(b);
        (self.placements.len() as u32).put(b);
        (self.targets.len() as u32).put(b);
        for row in &self.surfaces {
            row.put(b);
        }
        for row in &self.placements {
            row.put(b);
        }
        for row in &self.targets {
            row.put(b);
        }
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let grant = ContentGrant::take(c)?;
        let candidate_generation = c.u64()?;
        let chunk_ordinal = c.u32()?;
        let ns = count(c, 8)?;
        let np = count(c, 32)?;
        let nt = count(c, 64)?;
        Ok(Self {
            grant,
            candidate_generation,
            chunk_ordinal,
            surfaces: take_table(c, ns)?,
            placements: take_table(c, np)?,
            targets: take_table(c, nt)?,
        })
    }
}
