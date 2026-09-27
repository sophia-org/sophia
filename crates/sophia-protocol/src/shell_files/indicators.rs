//! View indicator (r6) file records: the whole `Indicators` object, the
//! indicator activation outcome event and the indicator activation
//! candidate, as whole typed values with no transfer shape.
use super::codec::u64_at;
use super::payload::*;
use super::*;
use crate::shell::encoding::indicators::{
    decode_shell_indicator_activation_outcome_value, decode_shell_indicator_activation_value,
    decode_shell_indicator_snapshot, encode_shell_indicator_activation_outcome_value,
    encode_shell_indicator_activation_value, encode_shell_indicator_snapshot,
};
use crate::*;

/// The whole indicator snapshot object under its domain transaction: active
/// output, output statuses and indicators, all in one body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileIndicators {
    pub transaction: TransactionId,
    pub snapshot: ShellIndicatorSnapshot,
}

pub fn encode_shell_file_indicators(
    header: ShellFileHeader,
    value: &ShellFileIndicators,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Indicators)?;
    let body = encode_shell_file_indicators_body(value)?;
    let bytes = encode_shell_file_record(header, &body)?;
    if bytes.len() > SHELL_FILE_INDICATORS_MAX_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(bytes)
}

pub fn encode_shell_file_indicators_body(
    value: &ShellFileIndicators,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let payload = encode_shell_indicator_snapshot(&value.snapshot)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    if SHELL_FILE_HEADER_BYTES + body.len() > SHELL_FILE_INDICATORS_MAX_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(body)
}

pub fn decode_shell_file_indicators(
    bytes: &[u8],
) -> Result<ShellFileIndicators, ShellFilePayloadError> {
    if bytes.len() > SHELL_FILE_INDICATORS_MAX_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    let r = record(bytes, ShellFileKind::Indicators, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileIndicators {
        transaction,
        snapshot: decode_shell_indicator_snapshot(&r.body[8..])?,
    })
}

/// One indicator activation-outcome event under its transaction: the exact
/// activation echo, status and reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileIndicatorActivationOutcome {
    pub transaction: TransactionId,
    pub outcome: ShellIndicatorActivationOutcome,
}

pub fn encode_shell_file_indicator_activation_outcome(
    header: ShellFileHeader,
    value: &ShellFileIndicatorActivationOutcome,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::IndicatorActivationOutcome)?;
    let body = encode_shell_file_indicator_activation_outcome_body(value)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn encode_shell_file_indicator_activation_outcome_body(
    value: &ShellFileIndicatorActivationOutcome,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let payload = encode_shell_indicator_activation_outcome_value(&value.outcome)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok(body)
}

pub fn decode_shell_file_indicator_activation_outcome(
    bytes: &[u8],
) -> Result<ShellFileIndicatorActivationOutcome, ShellFilePayloadError> {
    let r = record(bytes, ShellFileKind::IndicatorActivationOutcome, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileIndicatorActivationOutcome {
        transaction,
        outcome: decode_shell_indicator_activation_outcome_value(&r.body[8..])?,
    })
}

/// One indicator activation candidate under its transaction: the snapshot
/// generation, output, indicator, action and event the client is choosing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileIndicatorActivate {
    pub transaction: TransactionId,
    pub activation: ShellIndicatorActivation,
}

pub fn encode_shell_file_indicator_activate(
    header: ShellFileHeader,
    value: &ShellFileIndicatorActivate,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::IndicatorActivate)?;
    let body = encode_shell_file_indicator_activate_body(value)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn encode_shell_file_indicator_activate_body(
    value: &ShellFileIndicatorActivate,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let payload = encode_shell_indicator_activation_value(&value.activation)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok(body)
}

pub fn decode_shell_file_indicator_activate(
    bytes: &[u8],
) -> Result<ShellFileIndicatorActivate, ShellFilePayloadError> {
    let r = record(bytes, ShellFileKind::IndicatorActivate, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileIndicatorActivate {
        transaction,
        activation: decode_shell_indicator_activation_value(&r.body[8..])?,
    })
}
