use crate::TransactionId;
use crate::shell::encoding::native_launcher::{
    ShellNativeLauncherValueKind, decode_shell_native_launcher_value,
    encode_shell_native_launcher_value, shell_native_launcher_value_kind,
};
use crate::{IpcCodecError, IpcMessageKind, ShellNativeLauncherRecord, decode_frame, encode_frame};

/// All native-launcher records require a nonzero transaction. Admission and
/// matching an outstanding request remain the receiving owner's responsibility.
pub fn encode_shell_native_launcher_frame(
    transaction: TransactionId,
    record: &ShellNativeLauncherRecord,
) -> Result<Vec<u8>, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidRecord("native launcher transaction"));
    }
    let bytes = encode_shell_native_launcher_value(record)?;
    let kind = value_kind_to_ipc(shell_native_launcher_value_kind(record));
    encode_frame(kind, transaction, &bytes)
}

pub fn decode_shell_native_launcher_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellNativeLauncherRecord), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    if !header.transaction.is_valid() {
        return Err(IpcCodecError::InvalidRecord("native launcher transaction"));
    }
    let value_kind = value_kind_from_ipc(header.message_kind)
        .ok_or(IpcCodecError::InvalidRecord("not a native launcher record"))?;
    let record = decode_shell_native_launcher_value(value_kind, payload)?;
    Ok((header.transaction, record))
}

/// The `IpcMessageKind` <-> `ShellNativeLauncherValueKind` mapping. This is
/// the only place that knows both namings.
fn value_kind_from_ipc(kind: IpcMessageKind) -> Option<ShellNativeLauncherValueKind> {
    use IpcMessageKind as K;
    use ShellNativeLauncherValueKind as V;
    Some(match kind {
        K::ShellNativeLauncherOpening => V::Opening,
        K::ShellNativeLauncherAllocationRequest => V::AllocationRequest,
        K::ShellNativeLauncherCandidateBegin => V::CandidateBegin,
        K::ShellNativeLauncherCandidateChunk => V::CandidateChunk,
        K::ShellNativeLauncherFocus => V::Focus,
        K::ShellNativeLauncherFocusRevoked => V::FocusRevoked,
        K::ShellNativeLauncherInput => V::Input,
        K::ShellNativeLauncherInputAck => V::InputAck,
        K::ShellNativeLauncherActivate => V::Activate,
        K::ShellNativeLauncherActivationOutcome => V::ActivationOutcome,
        K::ShellNativeLauncherClosed => V::Closed,
        _ => return None,
    })
}

fn value_kind_to_ipc(kind: ShellNativeLauncherValueKind) -> IpcMessageKind {
    use IpcMessageKind as K;
    use ShellNativeLauncherValueKind as V;
    match kind {
        V::Opening => K::ShellNativeLauncherOpening,
        V::AllocationRequest => K::ShellNativeLauncherAllocationRequest,
        V::CandidateBegin => K::ShellNativeLauncherCandidateBegin,
        V::CandidateChunk => K::ShellNativeLauncherCandidateChunk,
        V::Focus => K::ShellNativeLauncherFocus,
        V::FocusRevoked => K::ShellNativeLauncherFocusRevoked,
        V::Input => K::ShellNativeLauncherInput,
        V::InputAck => K::ShellNativeLauncherInputAck,
        V::Activate => K::ShellNativeLauncherActivate,
        V::ActivationOutcome => K::ShellNativeLauncherActivationOutcome,
        V::Closed => K::ShellNativeLauncherClosed,
    }
}
