//! Admission file bytes carry the Session's supplied ceiling and the existing
//! profile identity. They neither authenticate a peer nor negotiate authority.
use super::codec::{u16_at, u32_at, u64_at};
use super::payload::*;
use super::*;
use crate::*;

fn limits(value: WmFileLimits) -> Result<(), WmFilePayloadError> {
    if value.profile_required {
        require_capabilities(
            value.capability_ceiling,
            SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
        )?;
    }
    Ok(())
}

pub fn encode_wm_file_limits(
    header: WmFileHeader,
    value: WmFileLimits,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Limits)?;
    limits(value)?;
    let mut body = Vec::new();
    body.extend(value.capability_ceiling.to_le_bytes());
    body.extend(
        u32::try_from(WM_FILE_MAX_BYTES)
            .map_err(|_| WmFileCodecError::Length)?
            .to_le_bytes(),
    );
    body.extend(
        u32::try_from(WM_FILE_MAX_BYTES)
            .map_err(|_| WmFileCodecError::Length)?
            .to_le_bytes(),
    );
    body.extend(WM_FILE_MAX_JOURNAL_RECORDS.to_le_bytes());
    body.extend(
        u16::try_from(WM_FILE_MAX_SECTIONS)
            .map_err(|_| WmFileCodecError::Length)?
            .to_le_bytes(),
    );
    body.extend(WM_FILE_ASSEMBLY_TIMEOUT_MILLIS.to_le_bytes());
    body.extend(WM_FILE_SEND_TIMEOUT_MILLIS.to_le_bytes());
    body.extend(u16::from(value.profile_required).to_le_bytes());
    body.extend([0; 2]);
    Ok(encode_wm_file_record(header, &body)?)
}

pub fn decode_wm_file_limits(bytes: &[u8]) -> Result<WmFileLimits, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::Limits, 32)?;
    reserved(&r.body[30..32])?;
    // API version one publishes fixed retention/assembly bounds. Adapters
    // use these same constants, so this object cannot silently promise less.
    if usize::try_from(u32_at(r.body, 8)?).ok() != Some(WM_FILE_MAX_BYTES)
        || usize::try_from(u32_at(r.body, 12)?).ok() != Some(WM_FILE_MAX_BYTES)
        || u16_at(r.body, 16)? != WM_FILE_MAX_JOURNAL_RECORDS
        || usize::from(u16_at(r.body, 18)?) != WM_FILE_MAX_SECTIONS
        || u32_at(r.body, 20)? != WM_FILE_ASSEMBLY_TIMEOUT_MILLIS
        || u32_at(r.body, 24)? != WM_FILE_SEND_TIMEOUT_MILLIS
        || u16_at(r.body, 28)? > 1
    {
        return Err(WmFilePayloadError::Value);
    }
    let value = WmFileLimits {
        capability_ceiling: u64_at(r.body, 0)?,
        profile_required: u16_at(r.body, 28)? == 1,
    };
    limits(value)?;
    Ok(value)
}

fn capabilities_record(
    header: WmFileHeader,
    kind: WmFileKind,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, kind)?;
    Ok(encode_wm_file_record(header, &capabilities.to_le_bytes())?)
}

pub fn encode_wm_file_negotiate(
    header: WmFileHeader,
    offered: WmFileNegotiationOffer,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Negotiate)?;
    validate_offer(offered)?;
    let mut body = Vec::with_capacity(16);
    body.extend(offered.required_capabilities.to_le_bytes());
    body.extend(offered.optional_capabilities.to_le_bytes());
    Ok(encode_wm_file_record(header, &body)?)
}
pub fn decode_wm_file_negotiate(
    bytes: &[u8],
) -> Result<WmFileNegotiationOffer, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::Negotiate, 16)?;
    let offer = WmFileNegotiationOffer {
        required_capabilities: u64_at(r.body, 0)?,
        optional_capabilities: u64_at(r.body, 8)?,
    };
    validate_offer(offer)?;
    Ok(offer)
}

fn validate_offer(offer: WmFileNegotiationOffer) -> Result<(), WmFilePayloadError> {
    if offer.required_capabilities & offer.optional_capabilities != 0 {
        return Err(WmFilePayloadError::Value);
    }
    Ok(())
}
pub fn encode_wm_file_negotiated(
    header: WmFileHeader,
    selected: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    capabilities_record(header, WmFileKind::Negotiated, selected)
}
pub fn decode_wm_file_negotiated(bytes: &[u8]) -> Result<u64, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::Negotiated, 8)?;
    Ok(u64_at(r.body, 0)?)
}

fn command_kind(kind: WmFileKind) -> Result<(), WmFilePayloadError> {
    match kind {
        WmFileKind::ProfilePrepare | WmFileKind::ProfileActivate | WmFileKind::ProfileRollback => {
            Ok(())
        }
        _ => Err(WmFileCodecError::Kind.into()),
    }
}
fn completion_kind(kind: WmFileKind) -> Result<(), WmFilePayloadError> {
    match kind {
        WmFileKind::ProfilePrepared | WmFileKind::ProfileActive | WmFileKind::ProfileRolledBack => {
            Ok(())
        }
        _ => Err(WmFileCodecError::Kind.into()),
    }
}

fn profile_prefix(
    transaction: TransactionId,
    identity_value: PolicyProfileIdentity,
) -> Result<Vec<u8>, WmFilePayloadError> {
    identity(&[transaction.raw()])?;
    PolicyProfileIdentity::new(
        identity_value.connection_epoch,
        identity_value.profile_generation,
        identity_value.profile_digest,
    )?;
    let mut body = Vec::new();
    body.extend(transaction.raw().to_le_bytes());
    body.extend(identity_value.profile_generation.to_le_bytes());
    body.extend(identity_value.profile_digest);
    Ok(body)
}
fn profile_record(r: WmFileRecord<'_>) -> Result<PolicyProfileCommand, WmFilePayloadError> {
    let transaction = u64_at(r.body, 0)?;
    identity(&[transaction])?;
    let digest = r
        .body
        .get(16..48)
        .ok_or(WmFileCodecError::Length)?
        .try_into()
        .map_err(|_| WmFileCodecError::Length)?;
    Ok(PolicyProfileCommand {
        transaction: TransactionId::from_raw(transaction),
        identity: PolicyProfileIdentity::new(
            r.header.connection_epoch,
            u64_at(r.body, 8)?,
            digest,
        )?,
    })
}

pub fn encode_wm_file_profile_command(
    header: WmFileHeader,
    command: PolicyProfileCommand,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    command_kind(header.kind)?;
    header_kind(header, header.kind)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION)?;
    same_epoch(header, command.identity.connection_epoch)?;
    let body = profile_prefix(command.transaction, command.identity)?;
    Ok(encode_wm_file_record(header, &body)?)
}
pub fn decode_wm_file_profile_command(
    bytes: &[u8],
    expected: WmFileKind,
    capabilities: u64,
) -> Result<PolicyProfileCommand, WmFilePayloadError> {
    command_kind(expected)?;
    let r = fixed_record(bytes, expected, 48)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION)?;
    profile_record(r)
}

pub fn encode_wm_file_profile_completion(
    header: WmFileHeader,
    completion: PolicyProfileCompletion,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    completion_kind(header.kind)?;
    header_kind(header, header.kind)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION)?;
    same_epoch(header, completion.identity.connection_epoch)?;
    let mut body = profile_prefix(completion.transaction, completion.identity)?;
    body.extend((completion.outcome as u16).to_le_bytes());
    body.extend([0; 6]);
    Ok(encode_wm_file_record(header, &body)?)
}
pub fn decode_wm_file_profile_completion(
    bytes: &[u8],
    expected: WmFileKind,
    capabilities: u64,
) -> Result<PolicyProfileCompletion, WmFilePayloadError> {
    completion_kind(expected)?;
    let r = fixed_record(bytes, expected, 56)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION)?;
    reserved(&r.body[50..56])?;
    let command = profile_record(r)?;
    Ok(PolicyProfileCompletion {
        transaction: command.transaction,
        identity: command.identity,
        outcome: PolicyProfileOutcome::try_from(u16_at(r.body, 48)?)?,
    })
}
