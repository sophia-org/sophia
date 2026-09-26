//! Scalar file messages reuse strict neutral semantics. Their delivery is not
//! admission, commit, presentation, or resource retirement.
use super::codec::{kind, u16_at, u64_at};
use super::payload::*;
use super::*;
use crate::*;

fn outcome(bytes: &[u8], at: usize) -> Result<PolicyProjectionOutcome, WmFilePayloadError> {
    policy_projection_outcome_from_code(u16_at(bytes, at)?).ok_or(WmFilePayloadError::Value)
}

pub fn encode_wm_file_dirty(
    header: WmFileHeader,
    value: &PolicyDirtyRequest,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Dirty)?;
    same_epoch(header, value.connection_epoch)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_POLICY_DIRTY)?;
    validate_policy_dirty_request(value)?;
    let mut body = Vec::new();
    body.extend(value.policy_generation.to_le_bytes());
    body.extend(
        u16::try_from(value.affected_outputs.len())
            .map_err(|_| WmFileCodecError::Length)?
            .to_le_bytes(),
    );
    body.extend([0; 6]);
    for output in &value.affected_outputs {
        body.extend(output.raw().to_le_bytes());
    }
    Ok(encode_wm_file_record(header, &body)?)
}

pub fn decode_wm_file_dirty(
    bytes: &[u8],
    capabilities: u64,
) -> Result<PolicyDirtyRequest, WmFilePayloadError> {
    let r = record(bytes, WmFileKind::Dirty, 16)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_POLICY_DIRTY)?;
    reserved(&r.body[10..16])?;
    let value = PolicyDirtyRequest {
        connection_epoch: r.header.connection_epoch,
        policy_generation: u64_at(r.body, 0)?,
        affected_outputs: outputs(&r.body[16..], u16_at(r.body, 8)?)?,
    };
    validate_policy_dirty_request(&value)?;
    Ok(value)
}

pub fn encode_wm_file_session_operation(
    header: WmFileHeader,
    value: &WmFileSessionOperation,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::SessionOperation)?;
    same_epoch(header, value.request.connection_epoch)?;
    identity(&[value.transaction.raw()])?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)?;
    validate_policy_session_operation_request(&value.request)?;
    let mut body = Vec::new();
    for id in [
        value.transaction.raw(),
        value.request.request_id,
        value.request.operation,
    ] {
        body.extend(id.to_le_bytes());
    }
    push_surface(&mut body, value.request.target);
    Ok(encode_wm_file_record(header, &body)?)
}

pub fn decode_wm_file_session_operation(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFileSessionOperation, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::SessionOperation, 32)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)?;
    let transaction = u64_at(r.body, 0)?;
    identity(&[transaction])?;
    let request = PolicySessionOperationRequest {
        connection_epoch: r.header.connection_epoch,
        request_id: u64_at(r.body, 8)?,
        operation: u64_at(r.body, 16)?,
        target: optional_surface(r.body, 24)?,
    };
    validate_policy_session_operation_request(&request)?;
    Ok(WmFileSessionOperation {
        transaction: TransactionId::from_raw(transaction),
        request,
    })
}

pub fn encode_wm_file_configuration_outcome(
    header: WmFileHeader,
    value: &WmFileConfigurationOutcome,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::ConfigurationOutcome)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_CONFIGURATION)?;
    identity(&[value.transaction.raw(), value.generation])?;
    let mut body = Vec::new();
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend(value.generation.to_le_bytes());
    body.extend(policy_projection_outcome_code(value.outcome).to_le_bytes());
    body.extend([0; 6]);
    Ok(encode_wm_file_record(header, &body)?)
}

pub fn decode_wm_file_configuration_outcome(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFileConfigurationOutcome, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::ConfigurationOutcome, 24)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_CONFIGURATION)?;
    reserved(&r.body[18..24])?;
    let transaction = u64_at(r.body, 0)?;
    let generation = u64_at(r.body, 8)?;
    identity(&[transaction, generation])?;
    Ok(WmFileConfigurationOutcome {
        transaction: TransactionId::from_raw(transaction),
        generation,
        outcome: outcome(r.body, 16)?,
    })
}

pub fn encode_wm_file_projection_outcome(
    header: WmFileHeader,
    value: &WmFileProjectionOutcome,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::ProjectionOutcome)?;
    identity(&[
        value.transaction.raw(),
        value.request_id,
        value.scene_generation,
    ])?;
    if value.expect_session_operation {
        require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)?;
    }
    let mut body = Vec::new();
    for id in [
        value.transaction.raw(),
        value.request_id,
        value.scene_generation,
    ] {
        body.extend(id.to_le_bytes());
    }
    body.extend(policy_projection_outcome_code(value.outcome).to_le_bytes());
    body.extend(u16::from(value.expect_session_operation).to_le_bytes());
    body.extend([0; 4]);
    Ok(encode_wm_file_record(header, &body)?)
}

pub fn decode_wm_file_projection_outcome(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFileProjectionOutcome, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::ProjectionOutcome, 32)?;
    reserved(&r.body[28..32])?;
    let transaction = u64_at(r.body, 0)?;
    let request_id = u64_at(r.body, 8)?;
    let scene_generation = u64_at(r.body, 16)?;
    identity(&[transaction, request_id, scene_generation])?;
    let flags = u16_at(r.body, 26)?;
    if flags > 1 {
        return Err(WmFilePayloadError::Value);
    }
    if flags == 1 {
        require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)?;
    }
    Ok(WmFileProjectionOutcome {
        transaction: TransactionId::from_raw(transaction),
        request_id,
        scene_generation,
        outcome: outcome(r.body, 24)?,
        expect_session_operation: flags == 1,
    })
}

pub fn encode_wm_file_session_operation_outcome(
    header: WmFileHeader,
    value: &WmFileSessionOperationOutcome,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::SessionOperationOutcome)?;
    same_epoch(header, value.outcome.connection_epoch)?;
    identity(&[value.transaction.raw()])?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)?;
    validate_policy_session_operation_outcome(&value.outcome)?;
    let mut body = Vec::new();
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend(value.outcome.request_id.to_le_bytes());
    body.extend(policy_projection_outcome_code(value.outcome.outcome).to_le_bytes());
    body.extend([0; 6]);
    Ok(encode_wm_file_record(header, &body)?)
}

pub fn decode_wm_file_session_operation_outcome(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFileSessionOperationOutcome, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::SessionOperationOutcome, 24)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS)?;
    reserved(&r.body[18..24])?;
    let transaction = u64_at(r.body, 0)?;
    identity(&[transaction])?;
    let value = PolicySessionOperationOutcome {
        connection_epoch: r.header.connection_epoch,
        request_id: u64_at(r.body, 8)?,
        outcome: outcome(r.body, 16)?,
    };
    validate_policy_session_operation_outcome(&value)?;
    Ok(WmFileSessionOperationOutcome {
        transaction: TransactionId::from_raw(transaction),
        outcome: value,
    })
}

pub fn encode_wm_file_presentation_receipt(
    header: WmFileHeader,
    value: &WmFilePresentationReceipt,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::PresentationReceipt)?;
    same_epoch(header, value.receipt.connection_epoch)?;
    identity(&[value.transaction.raw()])?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES)?;
    validate_policy_presentation_receipt(&value.receipt)?;
    let receipt = value.receipt;
    let mut body = Vec::new();
    for id in [
        value.transaction.raw(),
        receipt.publication_generation,
        receipt.output.raw(),
        receipt.output_generation,
        receipt.presentation_epoch,
    ] {
        body.extend(id.to_le_bytes());
    }
    body.extend(policy_presentation_outcome_code(receipt.outcome).to_le_bytes());
    body.extend([0; 6]);
    Ok(encode_wm_file_record(header, &body)?)
}

pub fn decode_wm_file_presentation_receipt(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFilePresentationReceipt, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::PresentationReceipt, 48)?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES)?;
    reserved(&r.body[42..48])?;
    let transaction = u64_at(r.body, 0)?;
    identity(&[transaction])?;
    let receipt = PolicyPresentationReceipt {
        connection_epoch: r.header.connection_epoch,
        publication_generation: u64_at(r.body, 8)?,
        output: OutputId::from_raw(u64_at(r.body, 16)?),
        output_generation: u64_at(r.body, 24)?,
        presentation_epoch: u64_at(r.body, 32)?,
        outcome: policy_presentation_outcome_from_code(u16_at(r.body, 40)?)
            .ok_or(WmFilePayloadError::Value)?,
    };
    validate_policy_presentation_receipt(&receipt)?;
    Ok(WmFilePresentationReceipt {
        transaction: TransactionId::from_raw(transaction),
        receipt,
    })
}

fn validate_submitted(value: WmFileSubmitted) -> Result<(), WmFilePayloadError> {
    identity(&[value.submission_id])?;
    if wm_file_class(value.candidate_kind) != WmFileClass::Candidate {
        return Err(WmFileCodecError::Class.into());
    }
    Ok(())
}

pub fn encode_wm_file_submitted(
    header: WmFileHeader,
    value: WmFileSubmitted,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Submitted)?;
    Ok(encode_wm_file_record(
        header,
        &encode_wm_file_submitted_body(value)?,
    )?)
}

/// The journal supplies the real epoch and event sequence when it reserves
/// the complete event; callers need not invent a header to encode custody.
pub fn encode_wm_file_submitted_body(
    value: WmFileSubmitted,
) -> Result<Vec<u8>, WmFilePayloadError> {
    validate_submitted(value)?;
    let mut body = Vec::new();
    body.extend(value.submission_id.to_le_bytes());
    body.extend((value.candidate_kind as u16).to_le_bytes());
    body.extend([0; 6]);
    Ok(body)
}

pub fn decode_wm_file_submitted(bytes: &[u8]) -> Result<WmFileSubmitted, WmFilePayloadError> {
    let r = fixed_record(bytes, WmFileKind::Submitted, 16)?;
    reserved(&r.body[10..16])?;
    let value = WmFileSubmitted {
        submission_id: u64_at(r.body, 0)?,
        candidate_kind: kind(u16_at(r.body, 8)?)?,
    };
    validate_submitted(value)?;
    Ok(value)
}
