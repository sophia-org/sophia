use super::records::*;
use crate::ipc::cursor::Cursor;
use crate::ipc::shell_content::fields::Wire;
use crate::{
    ContentCandidateChunk, IpcCodecError, IpcMessageKind, TransactionId, decode_frame, encode_frame,
};

/// All native-launcher records require a nonzero transaction. Admission and
/// matching an outstanding request remain the receiving owner's responsibility.
pub fn encode_shell_native_launcher_frame(
    transaction: TransactionId,
    record: &ShellNativeLauncherRecord,
) -> Result<Vec<u8>, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidRecord("native launcher transaction"));
    }
    super::validation::validate(record)?;
    let mut payload = Vec::new();
    macro_rules! put {
        ($v:ident, $kind:ident) => {{
            $v.put(&mut payload);
            IpcMessageKind::$kind
        }};
    }
    let kind = match record {
        ShellNativeLauncherRecord::Opening(v) => put!(v, ShellNativeLauncherOpening),
        ShellNativeLauncherRecord::AllocationRequest(v) => {
            put!(v, ShellNativeLauncherAllocationRequest)
        }
        ShellNativeLauncherRecord::CandidateBegin(v) => put!(v, ShellNativeLauncherCandidateBegin),
        ShellNativeLauncherRecord::CandidateChunk(v) => put!(v, ShellNativeLauncherCandidateChunk),
        ShellNativeLauncherRecord::Focus(v) => put!(v, ShellNativeLauncherFocus),
        ShellNativeLauncherRecord::FocusRevoked(v) => put!(v, ShellNativeLauncherFocusRevoked),
        ShellNativeLauncherRecord::Input(v) => put!(v, ShellNativeLauncherInput),
        ShellNativeLauncherRecord::InputAck(v) => put!(v, ShellNativeLauncherInputAck),
        ShellNativeLauncherRecord::Activate(v) => put!(v, ShellNativeLauncherActivate),
        ShellNativeLauncherRecord::ActivationOutcome(v) => {
            put!(v, ShellNativeLauncherActivationOutcome)
        }
        ShellNativeLauncherRecord::Closed(v) => put!(v, ShellNativeLauncherClosed),
    };
    encode_frame(kind, transaction, &payload)
}

pub fn decode_shell_native_launcher_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellNativeLauncherRecord), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    if !header.transaction.is_valid() {
        return Err(IpcCodecError::InvalidRecord("native launcher transaction"));
    }
    let mut cursor = Cursor::new(payload);
    use IpcMessageKind::*;
    use ShellNativeLauncherRecord as R;
    let record = match header.message_kind {
        ShellNativeLauncherOpening => R::Opening(NativeLauncherOpening::take(&mut cursor)?),
        ShellNativeLauncherAllocationRequest => {
            R::AllocationRequest(NativeLauncherAllocationRequest::take(&mut cursor)?)
        }
        ShellNativeLauncherCandidateBegin => {
            R::CandidateBegin(NativeLauncherCandidateBegin::take(&mut cursor)?)
        }
        ShellNativeLauncherCandidateChunk => {
            R::CandidateChunk(ContentCandidateChunk::take(&mut cursor)?)
        }
        ShellNativeLauncherFocus => R::Focus(NativeLauncherBinding::take(&mut cursor)?),
        ShellNativeLauncherFocusRevoked => {
            R::FocusRevoked(NativeLauncherFocusRevoked::take(&mut cursor)?)
        }
        ShellNativeLauncherInput => R::Input(NativeLauncherInput::take(&mut cursor)?),
        ShellNativeLauncherInputAck => R::InputAck(NativeLauncherInputAck::take(&mut cursor)?),
        ShellNativeLauncherActivate => R::Activate(NativeLauncherActivation::take(&mut cursor)?),
        ShellNativeLauncherActivationOutcome => {
            R::ActivationOutcome(NativeLauncherActivationOutcome::take(&mut cursor)?)
        }
        ShellNativeLauncherClosed => R::Closed(NativeLauncherClosed::take(&mut cursor)?),
        _ => return Err(IpcCodecError::InvalidRecord("not a native launcher record")),
    };
    cursor.finish()?;
    super::validation::validate(&record)?;
    Ok((header.transaction, record))
}
