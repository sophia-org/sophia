use crate::TransactionId;
use crate::shell::encoding::content::{
    ShellContentValueKind, encode_shell_content_value, parse_shell_content_value,
    shell_content_value_kind,
};
use crate::{IpcCodecError, IpcMessageKind, ShellContentRecord, decode_frame, encode_frame};

/// Decode one bounded frame; permission and lifecycle validation remain with
/// the owner. Nonzero reserved bytes and trailing payloads are rejected.
pub(crate) fn decode_shell_content_payload(
    kind: IpcMessageKind,
    transaction: TransactionId,
    payload: &[u8],
) -> Result<ShellContentRecord, IpcCodecError> {
    let value_kind = value_kind_from_ipc(kind)
        .ok_or(IpcCodecError::InvalidRecord("not a shell content record"))?;
    // Structure, then the frame's transaction rule, then semantics: the
    // order callers have always observed.
    let record = parse_shell_content_value(value_kind, payload)?;
    validate_transaction(transaction, &record)?;
    crate::shell::content::validate_shell_content_record(&record)?;
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
    let bytes = encode_shell_content_value(record)?;
    let kind = value_kind_to_ipc(shell_content_value_kind(record));
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

/// The `IpcMessageKind` <-> `ShellContentValueKind` mapping. This is the
/// only place that knows both namings.
fn value_kind_from_ipc(kind: IpcMessageKind) -> Option<ShellContentValueKind> {
    use IpcMessageKind as K;
    use ShellContentValueKind as V;
    Some(match kind {
        K::ShellContentAdmissionRefused => V::AdmissionRefused,
        K::ShellContentLimits => V::Limits,
        K::ShellContentOutputFacts => V::OutputFacts,
        K::ShellContentAllocationRequest => V::AllocationRequest,
        K::ShellContentAllocationResult => V::AllocationResult,
        K::ShellContentResourceBegin => V::ResourceBegin,
        K::ShellContentResourceStatus => V::ResourceStatus,
        K::ShellContentResourceChunk => V::ResourceChunk,
        K::ShellContentResourceEnd => V::ResourceEnd,
        K::ShellContentResourceCancel => V::ResourceCancel,
        K::ShellContentResourceRetire => V::ResourceRetire,
        K::ShellContentResourceReleased => V::ResourceReleased,
        K::ShellContentCandidateBegin => V::CandidateBegin,
        K::ShellContentCandidateChunk => V::CandidateChunk,
        K::ShellContentCandidateEnd => V::CandidateEnd,
        K::ShellContentCandidateOutcome => V::CandidateOutcome,
        K::ShellContentFrameDemand => V::FrameDemand,
        K::ShellContentFramePermit => V::FramePermit,
        K::ShellContentFrameDemandCancel => V::FrameDemandCancel,
        K::ShellContentAction => V::Action,
        K::ShellContentActionAck => V::ActionAck,
        _ => return None,
    })
}

fn value_kind_to_ipc(kind: ShellContentValueKind) -> IpcMessageKind {
    use IpcMessageKind as K;
    use ShellContentValueKind as V;
    match kind {
        V::AdmissionRefused => K::ShellContentAdmissionRefused,
        V::Limits => K::ShellContentLimits,
        V::OutputFacts => K::ShellContentOutputFacts,
        V::AllocationRequest => K::ShellContentAllocationRequest,
        V::AllocationResult => K::ShellContentAllocationResult,
        V::ResourceBegin => K::ShellContentResourceBegin,
        V::ResourceStatus => K::ShellContentResourceStatus,
        V::ResourceChunk => K::ShellContentResourceChunk,
        V::ResourceEnd => K::ShellContentResourceEnd,
        V::ResourceCancel => K::ShellContentResourceCancel,
        V::ResourceRetire => K::ShellContentResourceRetire,
        V::ResourceReleased => K::ShellContentResourceReleased,
        V::CandidateBegin => K::ShellContentCandidateBegin,
        V::CandidateChunk => K::ShellContentCandidateChunk,
        V::CandidateEnd => K::ShellContentCandidateEnd,
        V::CandidateOutcome => K::ShellContentCandidateOutcome,
        V::FrameDemand => K::ShellContentFrameDemand,
        V::FramePermit => K::ShellContentFramePermit,
        V::FrameDemandCancel => K::ShellContentFrameDemandCancel,
        V::Action => K::ShellContentAction,
        V::ActionAck => K::ShellContentActionAck,
    }
}
