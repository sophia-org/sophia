//! Value encoding for the r5 content vocabulary (`crate::shell::content`):
//! the per-variant body encode and decode of a [`ShellContentRecord`].
//!
//! The frame codec's message-kind enum, frame headers and transaction-id
//! rules stay with the frame codec, which maps its own message kinds onto
//! [`ShellContentValueKind`] and wraps [`ValueError`] into its own error
//! type at the boundary. The field-level `Wire` shapes live in `fields`,
//! split out purely for file size.
mod fields;

use super::Wire;
use crate::byte_cursor::Cursor;
use crate::shell::encoding::ValueError;
use crate::*;

/// The neutral counterpart of the frame codec's `ShellContent*` message
/// kinds: names which [`ShellContentRecord`] variant a byte body decodes
/// into, without naming any frame message kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellContentValueKind {
    AdmissionRefused,
    Limits,
    OutputFacts,
    AllocationRequest,
    AllocationResult,
    ResourceBegin,
    ResourceStatus,
    ResourceChunk,
    ResourceEnd,
    ResourceCancel,
    ResourceRetire,
    ResourceReleased,
    CandidateBegin,
    CandidateChunk,
    CandidateEnd,
    CandidateOutcome,
    FrameDemand,
    FramePermit,
    FrameDemandCancel,
    Action,
    ActionAck,
}

/// The value kind a given record encodes as. The frame codec uses this to
/// pick the message kind a frame carries it under.
pub fn shell_content_value_kind(record: &ShellContentRecord) -> ShellContentValueKind {
    use ShellContentValueKind as V;
    match record {
        ShellContentRecord::AdmissionRefused(_) => V::AdmissionRefused,
        ShellContentRecord::Limits(_) => V::Limits,
        ShellContentRecord::OutputFacts(_) => V::OutputFacts,
        ShellContentRecord::AllocationRequest(_) => V::AllocationRequest,
        ShellContentRecord::AllocationResult(_) => V::AllocationResult,
        ShellContentRecord::ResourceBegin(_) => V::ResourceBegin,
        ShellContentRecord::ResourceStatus(_) => V::ResourceStatus,
        ShellContentRecord::ResourceChunk(_) => V::ResourceChunk,
        ShellContentRecord::ResourceEnd(_) => V::ResourceEnd,
        ShellContentRecord::ResourceCancel(_) => V::ResourceCancel,
        ShellContentRecord::ResourceRetire(_) => V::ResourceRetire,
        ShellContentRecord::ResourceReleased(_) => V::ResourceReleased,
        ShellContentRecord::CandidateBegin(_) => V::CandidateBegin,
        ShellContentRecord::CandidateChunk(_) => V::CandidateChunk,
        ShellContentRecord::CandidateEnd(_) => V::CandidateEnd,
        ShellContentRecord::CandidateOutcome(_) => V::CandidateOutcome,
        ShellContentRecord::FrameDemand(_) => V::FrameDemand,
        ShellContentRecord::FramePermit(_) => V::FramePermit,
        ShellContentRecord::FrameDemandCancel(_) => V::FrameDemandCancel,
        ShellContentRecord::Action(_) => V::Action,
        ShellContentRecord::ActionAck(_) => V::ActionAck,
    }
}

/// Encodes one record's value body. Validates first, exactly as the IPC
/// payload codec did before this split.
pub fn encode_shell_content_value(record: &ShellContentRecord) -> Result<Vec<u8>, ValueError> {
    crate::shell::content::validation::validate(record)?;
    let mut bytes = Vec::new();
    put_content_value(record, &mut bytes);
    Ok(bytes)
}

/// Validated value-body length, without allocating or encoding a byte buffer.
pub fn shell_content_value_bytes(record: &ShellContentRecord) -> Result<usize, ValueError> {
    crate::shell::content::validation::validate(record)?;
    let mut bytes = super::ByteCount::new();
    put_content_value(record, &mut bytes);
    bytes.finish()
}

fn put_content_value(record: &ShellContentRecord, bytes: &mut impl super::ByteSink) {
    match record {
        ShellContentRecord::AdmissionRefused(value) => value.put(bytes),
        ShellContentRecord::Limits(value) => value.put(bytes),
        ShellContentRecord::OutputFacts(value) => value.put(bytes),
        ShellContentRecord::AllocationRequest(value) => value.put(bytes),
        ShellContentRecord::AllocationResult(value) => value.put(bytes),
        ShellContentRecord::ResourceBegin(value) => value.put(bytes),
        ShellContentRecord::ResourceStatus(value) => value.put(bytes),
        ShellContentRecord::ResourceChunk(value) => value.put(bytes),
        ShellContentRecord::ResourceEnd(value) => value.put(bytes),
        ShellContentRecord::ResourceCancel(value) => value.put(bytes),
        ShellContentRecord::ResourceRetire(value) => value.put(bytes),
        ShellContentRecord::ResourceReleased(value) => value.put(bytes),
        ShellContentRecord::CandidateBegin(value) => value.put(bytes),
        ShellContentRecord::CandidateChunk(value) => value.put(bytes),
        ShellContentRecord::CandidateEnd(value) => value.put(bytes),
        ShellContentRecord::CandidateOutcome(value) => value.put(bytes),
        ShellContentRecord::FrameDemand(value) => value.put(bytes),
        ShellContentRecord::FramePermit(value) => value.put(bytes),
        ShellContentRecord::FrameDemandCancel(value) => value.put(bytes),
        ShellContentRecord::Action(value) => value.put(bytes),
        ShellContentRecord::ActionAck(value) => value.put(bytes),
    }
}

/// Decodes and validates one record's value body for the given kind.
pub fn decode_shell_content_value(
    kind: ShellContentValueKind,
    payload: &[u8],
) -> Result<ShellContentRecord, ValueError> {
    let record = parse_shell_content_value(kind, payload)?;
    crate::shell::content::validation::validate(&record)?;
    Ok(record)
}

/// The structural half of [`decode_shell_content_value`]: fields, reserved
/// bytes, counts and no trailing bytes, without semantic validation, so a
/// carrier can check its own framing rules in between.
pub fn parse_shell_content_value(
    kind: ShellContentValueKind,
    payload: &[u8],
) -> Result<ShellContentRecord, ValueError> {
    use ShellContentValueKind as V;
    let mut cursor = Cursor::new(payload);
    let record = match kind {
        V::AdmissionRefused => {
            ShellContentRecord::AdmissionRefused(ContentAdmissionRefused::take(&mut cursor)?)
        }
        V::Limits => ShellContentRecord::Limits(ContentLimits::take(&mut cursor)?),
        V::OutputFacts => ShellContentRecord::OutputFacts(ContentOutputFacts::take(&mut cursor)?),
        V::AllocationRequest => {
            ShellContentRecord::AllocationRequest(ContentAllocationRequest::take(&mut cursor)?)
        }
        V::AllocationResult => {
            ShellContentRecord::AllocationResult(ContentAllocationResult::take(&mut cursor)?)
        }
        V::ResourceBegin => {
            ShellContentRecord::ResourceBegin(ContentResourceBegin::take(&mut cursor)?)
        }
        V::ResourceStatus => {
            ShellContentRecord::ResourceStatus(ContentResourceStatus::take(&mut cursor)?)
        }
        V::ResourceChunk => {
            ShellContentRecord::ResourceChunk(ContentResourceChunk::take(&mut cursor)?)
        }
        V::ResourceEnd => ShellContentRecord::ResourceEnd(ContentResourceEnd::take(&mut cursor)?),
        V::ResourceCancel => {
            ShellContentRecord::ResourceCancel(ContentResourceCancel::take(&mut cursor)?)
        }
        V::ResourceRetire => {
            ShellContentRecord::ResourceRetire(ContentResourceRetire::take(&mut cursor)?)
        }
        V::ResourceReleased => {
            ShellContentRecord::ResourceReleased(ContentResourceReleased::take(&mut cursor)?)
        }
        V::CandidateBegin => {
            ShellContentRecord::CandidateBegin(ContentCandidateBegin::take(&mut cursor)?)
        }
        V::CandidateChunk => {
            ShellContentRecord::CandidateChunk(ContentCandidateChunk::take(&mut cursor)?)
        }
        V::CandidateEnd => {
            ShellContentRecord::CandidateEnd(ContentCandidateEnd::take(&mut cursor)?)
        }
        V::CandidateOutcome => {
            ShellContentRecord::CandidateOutcome(ContentCandidateOutcome::take(&mut cursor)?)
        }
        V::FrameDemand => ShellContentRecord::FrameDemand(ContentFrameDemand::take(&mut cursor)?),
        V::FramePermit => ShellContentRecord::FramePermit(ContentFramePermit::take(&mut cursor)?),
        V::FrameDemandCancel => {
            ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel::take(&mut cursor)?)
        }
        V::Action => ShellContentRecord::Action(ContentAction::take(&mut cursor)?),
        V::ActionAck => ShellContentRecord::ActionAck(ContentActionAck::take(&mut cursor)?),
    };
    cursor.finish()?;
    Ok(record)
}

/// Encodes one whole candidate: its header, the three row counts as `u16`
/// and one reserved `u16`, then the surface, placement and target rows. Each
/// part the owner will receive is validated first.
pub fn encode_content_candidate(candidate: &ContentCandidate) -> Result<Vec<u8>, ValueError> {
    crate::shell::content::validation::validate_content_candidate(candidate)?;
    let mut bytes = Vec::new();
    put_candidate(candidate, &mut bytes);
    Ok(bytes)
}

/// Validated whole candidate value length, without cloning its rows.
pub fn content_candidate_bytes(candidate: &ContentCandidate) -> Result<usize, ValueError> {
    crate::shell::content::validation::validate_content_candidate(candidate)?;
    let mut bytes = super::ByteCount::new();
    put_candidate(candidate, &mut bytes);
    bytes.finish()
}

fn put_candidate(candidate: &ContentCandidate, bytes: &mut impl super::ByteSink) {
    candidate.grant.put(bytes);
    candidate.candidate_generation.put(bytes);
    candidate.output.put(bytes);
    candidate.facts_generation.put(bytes);
    candidate.pacing_permit.put(bytes);
    candidate.interaction_generation.put(bytes);
    (candidate.surfaces.len() as u16).put(bytes);
    (candidate.placements.len() as u16).put(bytes);
    (candidate.targets.len() as u16).put(bytes);
    0u16.put(bytes);
    for row in &candidate.surfaces {
        row.put(bytes);
    }
    for row in &candidate.placements {
        row.put(bytes);
    }
    for row in &candidate.targets {
        row.put(bytes);
    }
}

pub fn decode_content_candidate(bytes: &[u8]) -> Result<ContentCandidate, ValueError> {
    let mut cursor = Cursor::new(bytes);
    let grant = ContentGrant::take(&mut cursor)?;
    let candidate_generation = u64::take(&mut cursor)?;
    let output = ContentOutputId::take(&mut cursor)?;
    let facts_generation = u64::take(&mut cursor)?;
    let pacing_permit = u64::take(&mut cursor)?;
    let interaction_generation = u64::take(&mut cursor)?;
    let surfaces = table_count(&mut cursor, 8)?;
    let placements = table_count(&mut cursor, 32)?;
    let targets = table_count(&mut cursor, 64)?;
    super::reserved::<u16>(&mut cursor)?;
    let candidate = ContentCandidate {
        grant,
        candidate_generation,
        output,
        facts_generation,
        pacing_permit,
        interaction_generation,
        surfaces: rows(&mut cursor, surfaces)?,
        placements: rows(&mut cursor, placements)?,
        targets: rows(&mut cursor, targets)?,
    };
    cursor.finish()?;
    for part in candidate.parts() {
        crate::shell::content::validation::validate(&part)?;
    }
    Ok(candidate)
}

fn table_count(cursor: &mut Cursor<'_>, maximum: usize) -> Result<usize, ValueError> {
    let value = usize::from(u16::take(cursor)?);
    if value > maximum {
        return Err(ValueError::CountTooLarge {
            count: value,
            max: maximum,
        });
    }
    Ok(value)
}

fn rows<T: Wire>(cursor: &mut Cursor<'_>, count: usize) -> Result<Vec<T>, ValueError> {
    (0..count).map(|_| T::take(cursor)).collect()
}
