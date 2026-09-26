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
    match record {
        ShellContentRecord::AdmissionRefused(value) => value.put(&mut bytes),
        ShellContentRecord::Limits(value) => value.put(&mut bytes),
        ShellContentRecord::OutputFacts(value) => value.put(&mut bytes),
        ShellContentRecord::AllocationRequest(value) => value.put(&mut bytes),
        ShellContentRecord::AllocationResult(value) => value.put(&mut bytes),
        ShellContentRecord::ResourceBegin(value) => value.put(&mut bytes),
        ShellContentRecord::ResourceStatus(value) => value.put(&mut bytes),
        ShellContentRecord::ResourceChunk(value) => value.put(&mut bytes),
        ShellContentRecord::ResourceEnd(value) => value.put(&mut bytes),
        ShellContentRecord::ResourceCancel(value) => value.put(&mut bytes),
        ShellContentRecord::ResourceRetire(value) => value.put(&mut bytes),
        ShellContentRecord::ResourceReleased(value) => value.put(&mut bytes),
        ShellContentRecord::CandidateBegin(value) => value.put(&mut bytes),
        ShellContentRecord::CandidateChunk(value) => value.put(&mut bytes),
        ShellContentRecord::CandidateEnd(value) => value.put(&mut bytes),
        ShellContentRecord::CandidateOutcome(value) => value.put(&mut bytes),
        ShellContentRecord::FrameDemand(value) => value.put(&mut bytes),
        ShellContentRecord::FramePermit(value) => value.put(&mut bytes),
        ShellContentRecord::FrameDemandCancel(value) => value.put(&mut bytes),
        ShellContentRecord::Action(value) => value.put(&mut bytes),
        ShellContentRecord::ActionAck(value) => value.put(&mut bytes),
    }
    Ok(bytes)
}

/// Decodes one record's value body for the given kind. Rejects trailing
/// bytes and then validates, exactly as the IPC payload codec did before
/// this split.
pub fn decode_shell_content_value(
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
    crate::shell::content::validation::validate(&record)?;
    Ok(record)
}
