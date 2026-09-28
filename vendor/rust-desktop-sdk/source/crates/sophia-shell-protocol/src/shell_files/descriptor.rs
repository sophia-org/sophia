//! Native descriptor file envelopes. Domain transactions, file submission IDs
//! and journal sequences are distinct; the codec never substitutes one for
//! another. Admission, snapshot membership and presentation stay with owners.
use super::codec::u64_at;
use super::payload::{header_kind, record, same_epoch};
use super::*;
use crate::shell::encoding::descriptor::*;
use crate::*;

// A single declaration binds each native value to its file kind and limit.
// Limits below include the file header and the domain transaction word.
macro_rules! descriptor_records {
    ($($variant:ident($ty:ty), $encode:ident, $decode:ident, $cap:expr, |$v:ident| $epoch:expr;)+) => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub enum ShellDescriptorRecord { $($variant($ty),)+ }

        pub fn shell_file_descriptor_kind(value: &ShellDescriptorRecord) -> ShellFileKind {
            match value { $(ShellDescriptorRecord::$variant(_) => ShellFileKind::$variant,)+ }
        }

        pub fn shell_file_descriptor_max_bytes(kind: ShellFileKind) -> Option<usize> {
            match kind { $(ShellFileKind::$variant => Some($cap),)+ _ => None }
        }

        fn epoch(value: &ShellDescriptorRecord) -> u64 {
            match value { $(ShellDescriptorRecord::$variant($v) => $epoch,)+ }
        }

        fn encode_value(value: &ShellDescriptorRecord) -> Result<Vec<u8>, ShellFilePayloadError> {
            Ok(match value { $(ShellDescriptorRecord::$variant(v) => $encode(v)?,)+ })
        }

        fn decode_value(kind: ShellFileKind, bytes: &[u8]) -> Result<ShellDescriptorRecord, ShellFilePayloadError> {
            Ok(match kind {
                $(ShellFileKind::$variant => ShellDescriptorRecord::$variant($decode(bytes)?),)+
                _ => return Err(ShellFileCodecError::Kind.into()),
            })
        }
    }
}

descriptor_records! {
    Descriptors(ShellV1DescriptorSnapshot), encode_shell_descriptors_value, decode_shell_descriptors_value, SHELL_FILE_DESCRIPTORS_MAX_BYTES, |v| v.connection_epoch;
    Tabs(ShellTabSnapshot), encode_shell_tabs_value, decode_shell_tabs_value, SHELL_FILE_TABS_MAX_BYTES, |v| v.connection_epoch;
    Shortcuts(ShellShortcutCatalog), encode_shell_shortcuts_value, decode_shell_shortcuts_value, SHELL_FILE_SHORTCUTS_MAX_BYTES, |v| v.connection_epoch;
    DescriptorOutcome(ShellV1CandidateOutcome), encode_shell_descriptor_outcome_value, decode_shell_descriptor_outcome_value, 68, |v| v.connection_epoch;
    DescriptorActivation(ShellV1Activation), encode_shell_descriptor_activation_value, decode_shell_descriptor_activation_value, 116, |v| v.connection_epoch;
    ReferenceRequest(ShellReferenceRequest), encode_shell_reference_request_value, decode_shell_reference_request_value, 92, |v| v.connection_epoch;
    ReferenceOutcome(ShellReferenceOutcome), encode_shell_reference_outcome_value, decode_shell_reference_outcome_value, 88, |v| v.connection_epoch;
    LauncherRequest(ShellLauncherRequest), encode_shell_launcher_request_value, decode_shell_launcher_request_value, 352, |v| v.connection_epoch;
    LauncherOutcome(ShellLauncherOutcome), encode_shell_launcher_outcome_value, decode_shell_launcher_outcome_value, 76, |v| v.connection_epoch;
    LauncherActivation(ShellLauncherActivation), encode_shell_launcher_activation_value, decode_shell_launcher_activation_value, 92, |v| v.connection_epoch;
    LaunchOutcome(ShellLaunchOutcome), encode_shell_launch_outcome_value, decode_shell_launch_outcome_value, 92, |v| v.activation.connection_epoch;
    DescriptorCandidate(ShellV1Candidate), encode_shell_descriptor_candidate_value, decode_shell_descriptor_candidate_value, 276, |v| v.connection_epoch;
    DescriptorActivationAck(ShellV1ActivationAck), encode_shell_descriptor_ack_value, decode_shell_descriptor_ack_value, 60, |v| v.connection_epoch;
    TabsCandidate(ShellTabCandidate), encode_shell_tab_candidate_value, decode_shell_tab_candidate_value, 8260, |v| v.connection_epoch;
    ReferenceCandidate(ShellReferenceCandidate), encode_shell_reference_candidate_value, decode_shell_reference_candidate_value, 52488, |v| v.connection_epoch;
    LauncherCandidate(ShellLauncherCandidate), encode_shell_launcher_candidate_value, decode_shell_launcher_candidate_value, 168, |v| v.connection_epoch;
    LauncherActivationAck(ShellLauncherActivationAck), encode_shell_launcher_ack_value, decode_shell_launcher_ack_value, 92, |v| v.activation.connection_epoch;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileDescriptorRecord {
    pub transaction: TransactionId,
    pub record: ShellDescriptorRecord,
}

/// The journal assigns its sequence only after the value has been validated.
pub fn encode_shell_file_descriptor_body(
    value: &ShellFileDescriptorRecord,
) -> Result<(ShellFileKind, Vec<u8>), ShellFilePayloadError> {
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let kind = shell_file_descriptor_kind(&value.record);
    let payload = encode_value(&value.record)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend(payload);
    check_size(kind, SHELL_FILE_HEADER_BYTES + body.len())?;
    Ok((kind, body))
}

pub fn encode_shell_file_descriptor(
    header: ShellFileHeader,
    value: &ShellFileDescriptorRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, shell_file_descriptor_kind(&value.record))?;
    same_epoch(header, epoch(&value.record))?;
    let (_, body) = encode_shell_file_descriptor_body(value)?;
    Ok(encode_shell_file_record(header, &body)?)
}

fn check_size(kind: ShellFileKind, size: usize) -> Result<(), ShellFilePayloadError> {
    let max = shell_file_descriptor_max_bytes(kind).ok_or(ShellFileCodecError::Kind)?;
    if size > max {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(())
}

pub fn decode_shell_file_descriptor(
    bytes: &[u8],
    kind: ShellFileKind,
) -> Result<ShellFileDescriptorRecord, ShellFilePayloadError> {
    check_size(kind, bytes.len())?;
    let r = record(bytes, kind, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let value = decode_value(kind, &r.body[8..])?;
    same_epoch(r.header, epoch(&value))?;
    Ok(ShellFileDescriptorRecord {
        transaction,
        record: value,
    })
}
