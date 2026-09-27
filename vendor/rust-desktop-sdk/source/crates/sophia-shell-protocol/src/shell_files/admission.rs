use super::codec::{u16_at, u64_at};
use super::payload::*;
use super::*;
use crate::*;

pub fn encode_shell_file_negotiate(
    header: ShellFileHeader,
    hello: ShellV1ClientHello,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Negotiate)?;
    if hello.minimum_revision < 1 || hello.minimum_revision > hello.maximum_revision {
        return Err(ShellFilePayloadError::Value);
    }
    let mut body = Vec::with_capacity(16);
    body.extend(hello.minimum_revision.to_le_bytes());
    body.extend(hello.maximum_revision.to_le_bytes());
    body.extend(0u32.to_le_bytes());
    body.extend(hello.required_capabilities.to_le_bytes());
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn decode_shell_file_negotiate(
    bytes: &[u8],
) -> Result<ShellV1ClientHello, ShellFilePayloadError> {
    let r = fixed_record(bytes, ShellFileKind::Negotiate, 16)?;
    reserved(&r.body[4..8])?;
    let hello = ShellV1ClientHello {
        minimum_revision: u16_at(r.body, 0)?,
        maximum_revision: u16_at(r.body, 2)?,
        required_capabilities: u64_at(r.body, 8)?,
    };
    if hello.minimum_revision < 1 || hello.minimum_revision > hello.maximum_revision {
        return Err(ShellFilePayloadError::Value);
    }
    Ok(hello)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileNegotiated {
    pub welcome: crate::ShellV1ServerWelcome,
    pub limits_published: bool,
}

pub fn encode_shell_file_negotiated(
    header: ShellFileHeader,
    negotiated: ShellFileNegotiated,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Negotiated)?;
    same_epoch(header, negotiated.welcome.connection_epoch)?;
    let body = encode_shell_file_negotiated_body(negotiated)?;
    Ok(encode_shell_file_record(header, &body)?)
}

/// The journal supplies the real epoch and sequence; the body's epoch must
/// equal the journal's, which the decoder checks against the header.
pub fn encode_shell_file_negotiated_body(
    negotiated: ShellFileNegotiated,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    let welcome = negotiated.welcome;
    if welcome.selected_revision == 0 || welcome.connection_epoch == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    let mut body = Vec::with_capacity(32);
    body.extend(welcome.selected_revision.to_le_bytes());
    body.extend(0u16.to_le_bytes());
    body.extend(welcome.connection_epoch.to_le_bytes());
    body.extend(welcome.capabilities.to_le_bytes());
    body.extend(welcome.max_descriptors.to_le_bytes());
    body.extend(welcome.max_label_bytes.to_le_bytes());
    body.extend(welcome.max_pending_activations.to_le_bytes());
    body.extend(u16::from(negotiated.limits_published).to_le_bytes());
    body.extend(0u32.to_le_bytes());
    Ok(body)
}

pub fn decode_shell_file_negotiated(
    bytes: &[u8],
) -> Result<ShellFileNegotiated, ShellFilePayloadError> {
    let r = fixed_record(bytes, ShellFileKind::Negotiated, 32)?;
    reserved(&r.body[2..4])?;
    reserved(&r.body[28..32])?;
    let welcome = crate::ShellV1ServerWelcome {
        selected_revision: u16_at(r.body, 0)?,
        connection_epoch: u64_at(r.body, 4)?,
        capabilities: u64_at(r.body, 12)?,
        max_descriptors: u16_at(r.body, 20)?,
        max_label_bytes: u16_at(r.body, 22)?,
        max_pending_activations: u16_at(r.body, 24)?,
    };
    if welcome.selected_revision == 0 || welcome.connection_epoch == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    same_epoch(r.header, welcome.connection_epoch)?;
    let lp = u16_at(r.body, 26)?;
    if lp > 1 {
        return Err(ShellFilePayloadError::Value);
    }
    Ok(ShellFileNegotiated {
        welcome,
        limits_published: lp == 1,
    })
}

pub fn encode_shell_file_refused(
    header: ShellFileHeader,
    refused: crate::ContentAdmissionRefused,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Refused)?;
    let body = encode_shell_file_refused_body(&refused)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn encode_shell_file_refused_body(
    refused: &crate::ContentAdmissionRefused,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !(1..=4).contains(&refused.reason) {
        return Err(ShellFilePayloadError::Value);
    }
    let mut body = Vec::with_capacity(16);
    body.extend(refused.reason.to_le_bytes());
    body.extend(0u16.to_le_bytes());
    body.extend(0u32.to_le_bytes());
    body.extend(refused.denied_capabilities.to_le_bytes());
    Ok(body)
}

pub fn decode_shell_file_refused(
    bytes: &[u8],
) -> Result<crate::ContentAdmissionRefused, ShellFilePayloadError> {
    let r = fixed_record(bytes, ShellFileKind::Refused, 16)?;
    reserved(&r.body[2..4])?;
    reserved(&r.body[4..8])?;
    let reason = u16_at(r.body, 0)?;
    if !(1..=4).contains(&reason) {
        return Err(ShellFilePayloadError::Value);
    }
    Ok(crate::ContentAdmissionRefused {
        reason,
        denied_capabilities: u64_at(r.body, 8)?,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileSubmitted {
    pub submission_id: u64,
    pub candidate_kind: ShellFileKind,
}

pub fn encode_shell_file_submitted_body(
    value: ShellFileSubmitted,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if value.submission_id == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    if shell_file_class(value.candidate_kind) != ShellFileClass::Candidate {
        return Err(ShellFileCodecError::Kind.into());
    }
    let mut body = Vec::with_capacity(16);
    body.extend(value.submission_id.to_le_bytes());
    body.extend((value.candidate_kind as u16).to_le_bytes());
    body.extend([0; 6]);
    Ok(body)
}

pub fn encode_shell_file_submitted(
    header: ShellFileHeader,
    value: ShellFileSubmitted,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Submitted)?;
    let body = encode_shell_file_submitted_body(value)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn decode_shell_file_submitted(
    bytes: &[u8],
) -> Result<ShellFileSubmitted, ShellFilePayloadError> {
    let r = fixed_record(bytes, ShellFileKind::Submitted, 16)?;
    reserved(&r.body[10..16])?;
    let value = ShellFileSubmitted {
        submission_id: u64_at(r.body, 0)?,
        candidate_kind: super::codec::kind(u16_at(r.body, 8)?)?,
    };
    if value.submission_id == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    if shell_file_class(value.candidate_kind) != ShellFileClass::Candidate {
        return Err(ShellFileCodecError::Kind.into());
    }
    Ok(value)
}
