use crate::{
    AttentionState, DisplayLabel, MAX_CHROME_LABEL_LEN, OutputId, SOPHIA_SHELL_MAX_DESCRIPTORS,
    ShellV1Activation, ShellV1ActivationAck, ShellV1ActivationDisposition, ShellV1Candidate,
    ShellV1CandidateEntry, ShellV1CandidateOutcome, ShellV1CandidateOutcomeKind,
    ShellV1ClientHello, ShellV1Descriptor, ShellV1DescriptorSnapshot, ShellV1ReservationEdge,
    ShellV1ServerWelcome, ShellV1WorkAreaReservation, ToplevelActionCapabilityRef, TransactionId,
    TrustLevel,
};

use super::cursor::{Cursor, push_u8, push_u16, push_u64};
use super::frame::{decode_frame, encode_frame};
use super::primitives::{decode_optional_text, encode_optional_text};
use super::types::{IpcCodecError, IpcMessageKind};

pub fn encode_shell_v1_client_hello_frame(
    hello: ShellV1ClientHello,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut payload = Vec::with_capacity(16);
    push_u16(&mut payload, hello.minimum_revision);
    push_u16(&mut payload, hello.maximum_revision);
    push_u64(&mut payload, hello.required_capabilities);
    encode_frame(
        IpcMessageKind::ShellV1ClientHello,
        TransactionId::INVALID,
        &payload,
    )
}

pub fn decode_shell_v1_client_hello_frame(
    frame: &[u8],
) -> Result<ShellV1ClientHello, IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1ClientHello)?;
    require_handshake_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let hello = ShellV1ClientHello {
        minimum_revision: cursor.u16()?,
        maximum_revision: cursor.u16()?,
        required_capabilities: cursor.u64()?,
    };
    cursor.finish()?;
    Ok(hello)
}

pub fn encode_shell_v1_server_welcome_frame(
    welcome: ShellV1ServerWelcome,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut payload = Vec::with_capacity(32);
    push_u16(&mut payload, welcome.selected_revision);
    push_u16(&mut payload, 0);
    push_u64(&mut payload, welcome.connection_epoch);
    push_u64(&mut payload, welcome.capabilities);
    push_u16(&mut payload, welcome.max_descriptors);
    push_u16(&mut payload, welcome.max_label_bytes);
    push_u16(&mut payload, welcome.max_pending_activations);
    push_u16(&mut payload, 0);
    encode_frame(
        IpcMessageKind::ShellV1ServerWelcome,
        TransactionId::INVALID,
        &payload,
    )
}

pub fn decode_shell_v1_server_welcome_frame(
    frame: &[u8],
) -> Result<ShellV1ServerWelcome, IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1ServerWelcome)?;
    require_handshake_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let selected_revision = cursor.u16()?;
    require_zero(cursor.u16()?, "shell_welcome_reserved")?;
    let welcome = ShellV1ServerWelcome {
        selected_revision,
        connection_epoch: cursor.u64()?,
        capabilities: cursor.u64()?,
        max_descriptors: cursor.u16()?,
        max_label_bytes: cursor.u16()?,
        max_pending_activations: cursor.u16()?,
    };
    require_zero(cursor.u16()?, "shell_welcome_trailing_reserved")?;
    cursor.finish()?;
    validate_welcome(welcome)?;
    Ok(welcome)
}

pub fn encode_shell_v1_descriptor_snapshot_frame(
    transaction: TransactionId,
    snapshot: &ShellV1DescriptorSnapshot,
) -> Result<Vec<u8>, IpcCodecError> {
    require_transaction(transaction)?;
    validate_snapshot(snapshot)?;
    let mut payload = Vec::new();
    push_u64(&mut payload, snapshot.connection_epoch);
    push_u64(&mut payload, snapshot.snapshot_generation);
    push_u64(&mut payload, snapshot.output.raw());
    push_u64(&mut payload, snapshot.output_generation);
    push_u64(&mut payload, snapshot.broker_epoch);
    push_u64(&mut payload, snapshot.broker_revocation_epoch);
    push_u16(&mut payload, snapshot.descriptors.len() as u16);
    push_u16(&mut payload, 0);
    for descriptor in &snapshot.descriptors {
        encode_descriptor(descriptor, &mut payload)?;
    }
    encode_frame(
        IpcMessageKind::ShellV1DescriptorSnapshot,
        transaction,
        &payload,
    )
}

pub fn decode_shell_v1_descriptor_snapshot_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellV1DescriptorSnapshot), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(
        header.message_kind,
        IpcMessageKind::ShellV1DescriptorSnapshot,
    )?;
    require_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let connection_epoch = cursor.u64()?;
    let snapshot_generation = cursor.u64()?;
    let output = OutputId::from_raw(cursor.u64()?);
    let output_generation = cursor.u64()?;
    let broker_epoch = cursor.u64()?;
    let broker_revocation_epoch = cursor.u64()?;
    let count = cursor.u16()? as usize;
    require_count(count, SOPHIA_SHELL_MAX_DESCRIPTORS)?;
    require_zero(cursor.u16()?, "shell_snapshot_reserved")?;
    let mut descriptors = Vec::with_capacity(count);
    for _ in 0..count {
        descriptors.push(decode_descriptor(&mut cursor)?);
    }
    cursor.finish()?;
    let snapshot = ShellV1DescriptorSnapshot {
        connection_epoch,
        snapshot_generation,
        output,
        output_generation,
        broker_epoch,
        broker_revocation_epoch,
        descriptors,
    };
    validate_snapshot(&snapshot)?;
    Ok((header.transaction, snapshot))
}

pub fn encode_shell_v1_candidate_frame(
    transaction: TransactionId,
    candidate: &ShellV1Candidate,
) -> Result<Vec<u8>, IpcCodecError> {
    require_transaction(transaction)?;
    validate_candidate(candidate)?;
    let mut payload = Vec::new();
    push_u64(&mut payload, candidate.connection_epoch);
    push_u64(&mut payload, candidate.snapshot_generation);
    push_u64(&mut payload, candidate.candidate_generation);
    push_u64(&mut payload, candidate.output.raw());
    push_u8(&mut payload, u8::from(candidate.visible));
    push_u8(
        &mut payload,
        candidate
            .reservation
            .map_or(0, |reservation| encode_reservation_edge(reservation.edge)),
    );
    push_u16(&mut payload, candidate.selected_slot.unwrap_or(0));
    push_u16(&mut payload, candidate.entries.len() as u16);
    push_u16(
        &mut payload,
        candidate
            .reservation
            .map_or(0, |reservation| reservation.thickness_px),
    );
    for entry in &candidate.entries {
        push_u16(&mut payload, entry.slot);
        push_u16(&mut payload, 0);
        push_u64(&mut payload, entry.generation);
    }
    encode_frame(IpcMessageKind::ShellV1Candidate, transaction, &payload)
}

pub fn decode_shell_v1_candidate_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellV1Candidate), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1Candidate)?;
    require_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let connection_epoch = cursor.u64()?;
    let snapshot_generation = cursor.u64()?;
    let candidate_generation = cursor.u64()?;
    let output = OutputId::from_raw(cursor.u64()?);
    let visible = decode_bool(cursor.u8()?, "shell_candidate_visible")?;
    let reservation_edge = cursor.u8()?;
    let selected_slot = match cursor.u16()? {
        0 => None,
        slot => Some(slot),
    };
    let count = cursor.u16()? as usize;
    require_count(count, SOPHIA_SHELL_MAX_DESCRIPTORS)?;
    let reservation_thickness = cursor.u16()?;
    let reservation = decode_reservation(reservation_edge, reservation_thickness)?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let slot = cursor.u16()?;
        require_zero(cursor.u16()?, "shell_candidate_entry_reserved")?;
        entries.push(ShellV1CandidateEntry {
            slot,
            generation: cursor.u64()?,
        });
    }
    cursor.finish()?;
    let candidate = ShellV1Candidate {
        connection_epoch,
        snapshot_generation,
        candidate_generation,
        output,
        visible,
        selected_slot,
        reservation,
        entries,
    };
    validate_candidate(&candidate)?;
    Ok((header.transaction, candidate))
}

pub fn encode_shell_v1_candidate_outcome_frame(
    transaction: TransactionId,
    outcome: ShellV1CandidateOutcome,
) -> Result<Vec<u8>, IpcCodecError> {
    require_transaction(transaction)?;
    if outcome.connection_epoch == 0 || outcome.candidate_generation == 0 {
        return Err(IpcCodecError::InvalidRecord("shell_candidate_outcome"));
    }
    if outcome.kind == ShellV1CandidateOutcomeKind::Presented && outcome.presentation_epoch == 0 {
        return Err(IpcCodecError::InvalidRecord(
            "shell_presented_outcome_epoch",
        ));
    }
    if outcome.kind != ShellV1CandidateOutcomeKind::Presented && outcome.presentation_epoch != 0 {
        return Err(IpcCodecError::InvalidRecord(
            "shell_nonpresented_outcome_epoch",
        ));
    }
    let mut payload = Vec::with_capacity(32);
    push_u64(&mut payload, outcome.connection_epoch);
    push_u64(&mut payload, outcome.candidate_generation);
    push_u64(&mut payload, outcome.presentation_epoch);
    push_u16(&mut payload, encode_outcome(outcome.kind));
    push_u16(&mut payload, 0);
    encode_frame(
        IpcMessageKind::ShellV1CandidateOutcome,
        transaction,
        &payload,
    )
}

pub fn decode_shell_v1_candidate_outcome_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellV1CandidateOutcome), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1CandidateOutcome)?;
    require_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let outcome = ShellV1CandidateOutcome {
        connection_epoch: cursor.u64()?,
        candidate_generation: cursor.u64()?,
        presentation_epoch: cursor.u64()?,
        kind: decode_outcome(cursor.u16()?)?,
    };
    require_zero(cursor.u16()?, "shell_outcome_reserved")?;
    cursor.finish()?;
    encode_shell_v1_candidate_outcome_frame(header.transaction, outcome)?;
    Ok((header.transaction, outcome))
}

pub fn encode_shell_v1_activation_frame(
    transaction: TransactionId,
    activation: ShellV1Activation,
) -> Result<Vec<u8>, IpcCodecError> {
    require_transaction(transaction)?;
    if activation.connection_epoch == 0
        || activation.candidate_generation == 0
        || activation.presentation_epoch == 0
        || activation.activation == 0
    {
        return Err(IpcCodecError::InvalidRecord("shell_activation"));
    }
    validate_action(activation.action, activation.action.target_slot)?;
    if activation.action.recipient_epoch != activation.connection_epoch {
        return Err(IpcCodecError::InvalidRecord("shell_activation_recipient"));
    }
    let mut payload = Vec::with_capacity(80);
    push_u64(&mut payload, activation.connection_epoch);
    push_u64(&mut payload, activation.candidate_generation);
    push_u64(&mut payload, activation.presentation_epoch);
    push_u64(&mut payload, activation.activation);
    encode_action(activation.action, &mut payload);
    encode_frame(IpcMessageKind::ShellV1Activation, transaction, &payload)
}

pub fn decode_shell_v1_activation_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellV1Activation), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1Activation)?;
    require_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let activation = ShellV1Activation {
        connection_epoch: cursor.u64()?,
        candidate_generation: cursor.u64()?,
        presentation_epoch: cursor.u64()?,
        activation: cursor.u64()?,
        action: decode_action(&mut cursor)?,
    };
    cursor.finish()?;
    encode_shell_v1_activation_frame(header.transaction, activation)?;
    Ok((header.transaction, activation))
}

pub fn encode_shell_v1_activation_ack_frame(
    transaction: TransactionId,
    ack: ShellV1ActivationAck,
) -> Result<Vec<u8>, IpcCodecError> {
    require_transaction(transaction)?;
    if ack.connection_epoch == 0 || ack.activation == 0 {
        return Err(IpcCodecError::InvalidRecord("shell_activation_ack"));
    }
    let mut payload = Vec::with_capacity(24);
    push_u64(&mut payload, ack.connection_epoch);
    push_u64(&mut payload, ack.activation);
    push_u16(&mut payload, encode_activation_disposition(ack.disposition));
    push_u16(&mut payload, 0);
    encode_frame(IpcMessageKind::ShellV1ActivationAck, transaction, &payload)
}

pub fn decode_shell_v1_activation_ack_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellV1ActivationAck), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require_kind(header.message_kind, IpcMessageKind::ShellV1ActivationAck)?;
    require_transaction(header.transaction)?;
    let mut cursor = Cursor::new(payload);
    let ack = ShellV1ActivationAck {
        connection_epoch: cursor.u64()?,
        activation: cursor.u64()?,
        disposition: decode_activation_disposition(cursor.u16()?)?,
    };
    require_zero(cursor.u16()?, "shell_activation_ack_reserved")?;
    cursor.finish()?;
    encode_shell_v1_activation_ack_frame(header.transaction, ack)?;
    Ok((header.transaction, ack))
}

fn validate_welcome(welcome: ShellV1ServerWelcome) -> Result<(), IpcCodecError> {
    if welcome.selected_revision == 0
        || welcome.connection_epoch == 0
        || welcome.max_descriptors == 0
        || usize::from(welcome.max_descriptors) > SOPHIA_SHELL_MAX_DESCRIPTORS
        || welcome.max_label_bytes == 0
        || usize::from(welcome.max_label_bytes) > MAX_CHROME_LABEL_LEN
        || welcome.max_pending_activations == 0
    {
        return Err(IpcCodecError::InvalidRecord("shell_welcome"));
    }
    Ok(())
}

pub(super) fn validate_snapshot(snapshot: &ShellV1DescriptorSnapshot) -> Result<(), IpcCodecError> {
    sophia_shell_protocol::shell::descriptor::validate_shell_descriptor_snapshot(snapshot)
        .map_err(Into::into)
}

fn validate_candidate(candidate: &ShellV1Candidate) -> Result<(), IpcCodecError> {
    sophia_shell_protocol::shell::descriptor::validate_shell_descriptor_candidate(candidate)
        .map_err(Into::into)
}

fn encode_descriptor(
    descriptor: &ShellV1Descriptor,
    payload: &mut Vec<u8>,
) -> Result<(), IpcCodecError> {
    push_u16(payload, descriptor.slot);
    push_u8(payload, encode_trust(descriptor.trust_level));
    push_u8(payload, encode_attention(descriptor.attention));
    push_u64(payload, descriptor.generation);
    encode_action(descriptor.action, payload);
    encode_optional_text(
        payload,
        "shell_descriptor_label",
        descriptor.label.as_ref().map(|label| label.text.as_str()),
        MAX_CHROME_LABEL_LEN,
    )?;
    push_u8(
        payload,
        u8::from(
            descriptor
                .label
                .as_ref()
                .is_some_and(|label| label.redacted),
        ),
    );
    Ok(())
}

fn decode_descriptor(cursor: &mut Cursor<'_>) -> Result<ShellV1Descriptor, IpcCodecError> {
    let slot = cursor.u16()?;
    let trust_level = decode_trust(cursor.u8()?)?;
    let attention = decode_attention(cursor.u8()?)?;
    let generation = cursor.u64()?;
    let action = decode_action(cursor)?;
    let label = decode_optional_text(cursor, "shell_descriptor_label", MAX_CHROME_LABEL_LEN)?;
    let redacted = decode_bool(cursor.u8()?, "shell_descriptor_label_redacted")?;
    if label.is_none() && redacted {
        return Err(IpcCodecError::InvalidRecord(
            "shell_descriptor_redacted_without_label",
        ));
    }
    let label = label.map(|text| DisplayLabel { text, redacted });
    Ok(ShellV1Descriptor {
        slot,
        generation,
        label,
        trust_level,
        attention,
        action,
    })
}

fn encode_action(action: ToplevelActionCapabilityRef, payload: &mut Vec<u8>) {
    push_u64(payload, action.token);
    push_u64(payload, action.issuer_epoch);
    push_u64(payload, action.issuer_revocation_epoch);
    push_u64(payload, action.recipient_epoch);
    push_u16(payload, action.target_slot);
    push_u16(payload, 0);
    push_u64(payload, action.target_generation);
}

fn decode_action(cursor: &mut Cursor<'_>) -> Result<ToplevelActionCapabilityRef, IpcCodecError> {
    let action = ToplevelActionCapabilityRef {
        token: cursor.u64()?,
        issuer_epoch: cursor.u64()?,
        issuer_revocation_epoch: cursor.u64()?,
        recipient_epoch: cursor.u64()?,
        target_slot: cursor.u16()?,
        target_generation: {
            require_zero(cursor.u16()?, "shell_action_reserved")?;
            cursor.u64()?
        },
    };
    Ok(action)
}

fn validate_action(
    action: ToplevelActionCapabilityRef,
    expected_slot: u16,
) -> Result<(), IpcCodecError> {
    sophia_shell_protocol::validate_toplevel_action(action)?;
    if action.target_slot != expected_slot {
        return Err(IpcCodecError::InvalidRecord("shell_toplevel_action"));
    }
    Ok(())
}

const fn encode_trust(value: TrustLevel) -> u8 {
    match value {
        TrustLevel::Unknown => 0,
        TrustLevel::Trusted => 1,
        TrustLevel::Untrusted => 2,
        TrustLevel::Isolated => 3,
    }
}

fn decode_trust(value: u8) -> Result<TrustLevel, IpcCodecError> {
    match value {
        0 => Ok(TrustLevel::Unknown),
        1 => Ok(TrustLevel::Trusted),
        2 => Ok(TrustLevel::Untrusted),
        3 => Ok(TrustLevel::Isolated),
        other => Err(IpcCodecError::InvalidEnum {
            field: "shell_descriptor_trust",
            value: u32::from(other),
        }),
    }
}

const fn encode_attention(value: AttentionState) -> u8 {
    match value {
        AttentionState::None => 0,
        AttentionState::Notice => 1,
        AttentionState::Critical => 2,
    }
}

fn decode_attention(value: u8) -> Result<AttentionState, IpcCodecError> {
    match value {
        0 => Ok(AttentionState::None),
        1 => Ok(AttentionState::Notice),
        2 => Ok(AttentionState::Critical),
        other => Err(IpcCodecError::InvalidEnum {
            field: "shell_descriptor_attention",
            value: u32::from(other),
        }),
    }
}

const fn encode_outcome(value: ShellV1CandidateOutcomeKind) -> u16 {
    match value {
        ShellV1CandidateOutcomeKind::Prepared => 1,
        ShellV1CandidateOutcomeKind::Presented => 2,
        ShellV1CandidateOutcomeKind::Rejected => 3,
        ShellV1CandidateOutcomeKind::Superseded => 4,
    }
}

fn decode_outcome(value: u16) -> Result<ShellV1CandidateOutcomeKind, IpcCodecError> {
    match value {
        1 => Ok(ShellV1CandidateOutcomeKind::Prepared),
        2 => Ok(ShellV1CandidateOutcomeKind::Presented),
        3 => Ok(ShellV1CandidateOutcomeKind::Rejected),
        4 => Ok(ShellV1CandidateOutcomeKind::Superseded),
        other => Err(IpcCodecError::InvalidEnum {
            field: "shell_candidate_outcome",
            value: u32::from(other),
        }),
    }
}

const fn encode_reservation_edge(value: ShellV1ReservationEdge) -> u8 {
    match value {
        ShellV1ReservationEdge::Top => 1,
        ShellV1ReservationEdge::Bottom => 2,
        ShellV1ReservationEdge::Left => 3,
        ShellV1ReservationEdge::Right => 4,
    }
}

/// Zero edge and zero thickness together mean no reservation, which is the
/// exact bit pattern the pre-reservation encoder wrote into these reserved
/// fields, so every previously valid frame still decodes to the same record.
fn decode_reservation(
    edge: u8,
    thickness_px: u16,
) -> Result<Option<ShellV1WorkAreaReservation>, IpcCodecError> {
    let edge = match edge {
        0 => {
            if thickness_px != 0 {
                return Err(IpcCodecError::InvalidRecord(
                    "shell_candidate_reservation_thickness",
                ));
            }
            return Ok(None);
        }
        1 => ShellV1ReservationEdge::Top,
        2 => ShellV1ReservationEdge::Bottom,
        3 => ShellV1ReservationEdge::Left,
        4 => ShellV1ReservationEdge::Right,
        other => {
            return Err(IpcCodecError::InvalidEnum {
                field: "shell_candidate_reservation_edge",
                value: u32::from(other),
            });
        }
    };
    Ok(Some(ShellV1WorkAreaReservation { edge, thickness_px }))
}

const fn encode_activation_disposition(value: ShellV1ActivationDisposition) -> u16 {
    match value {
        ShellV1ActivationDisposition::Consumed => 1,
        ShellV1ActivationDisposition::RejectedStale => 2,
    }
}

fn decode_activation_disposition(
    value: u16,
) -> Result<ShellV1ActivationDisposition, IpcCodecError> {
    match value {
        1 => Ok(ShellV1ActivationDisposition::Consumed),
        2 => Ok(ShellV1ActivationDisposition::RejectedStale),
        other => Err(IpcCodecError::InvalidEnum {
            field: "shell_activation_disposition",
            value: u32::from(other),
        }),
    }
}

const fn decode_bool(value: u8, field: &'static str) -> Result<bool, IpcCodecError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(IpcCodecError::InvalidBool {
            field,
            value: other,
        }),
    }
}

fn require_kind(actual: IpcMessageKind, expected: IpcMessageKind) -> Result<(), IpcCodecError> {
    if actual == expected {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidEnum {
            field: "shell_message_kind",
            value: actual as u32,
        })
    }
}

fn require_transaction(transaction: TransactionId) -> Result<(), IpcCodecError> {
    if transaction.is_valid() {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidTransaction(transaction.raw()))
    }
}

fn require_handshake_transaction(transaction: TransactionId) -> Result<(), IpcCodecError> {
    if transaction.is_valid() {
        Err(IpcCodecError::InvalidTransaction(transaction.raw()))
    } else {
        Ok(())
    }
}

fn require_zero(value: u16, _field: &'static str) -> Result<(), IpcCodecError> {
    if value == 0 {
        Ok(())
    } else {
        Err(IpcCodecError::ReservedNonZero(u32::from(value)))
    }
}

fn require_count(count: usize, max: usize) -> Result<(), IpcCodecError> {
    if count <= max {
        Ok(())
    } else {
        Err(IpcCodecError::CountTooLarge { count, max })
    }
}
