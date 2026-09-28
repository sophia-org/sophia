use crate::{
    DisplayHeadId, DisplayModeId, MAX_OUTPUT_AUTHORITY_GROUPS, MAX_OUTPUT_AUTHORITY_HEADS,
    MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP, MAX_OUTPUT_AUTHORITY_LABEL_BYTES,
    MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD, OutputAuthoritySnapshot, OutputGroupMember,
    OutputHeadDescriptor, OutputHeadMapping, OutputHeadTargetProposal, OutputId,
    OutputLogicalGroupProposal, OutputLogicalGroupState, OutputModeDescriptor,
    OutputTopologyCandidate, OutputTopologyIntent, OutputTransform, OutputTransformSet,
    OutputVrrPolicy, Rect, Size, TransactionId,
};

use super::cursor::{Cursor, push_i32, push_u16, push_u32, push_u64};
use super::frame::{decode_frame, encode_frame};
use super::types::{IpcCodecError, IpcMessageKind};

// Preserve the old socket API while the role model has a neutral owner.
pub use crate::output_role::*;

pub fn encode_output_v1_client_hello_frame(
    message: OutputV1ClientHello,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut payload = Vec::with_capacity(12);
    push_u16(&mut payload, message.minimum_revision);
    push_u16(&mut payload, message.maximum_revision);
    push_u64(&mut payload, message.capabilities);
    encode_frame(
        IpcMessageKind::OutputV1ClientHello,
        TransactionId::INVALID,
        &payload,
    )
}

pub fn decode_output_v1_client_hello_frame(
    frame: &[u8],
) -> Result<OutputV1ClientHello, IpcCodecError> {
    let (_, mut cursor) = decode_payload(frame, IpcMessageKind::OutputV1ClientHello, false)?;
    let message = OutputV1ClientHello {
        minimum_revision: cursor.u16()?,
        maximum_revision: cursor.u16()?,
        capabilities: cursor.u64()?,
    };
    cursor.finish()?;
    Ok(message)
}

pub fn encode_output_v1_server_welcome_frame(
    message: OutputV1ServerWelcome,
) -> Result<Vec<u8>, IpcCodecError> {
    if message.selected_revision == 0
        || message.connection_epoch == 0
        || message.max_heads == 0
        || message.max_groups == 0
        || message.max_modes_per_head == 0
        || message.max_heads_per_group == 0
    {
        return Err(invalid("server_welcome"));
    }
    let mut payload = Vec::with_capacity(28);
    push_u16(&mut payload, message.selected_revision);
    push_u16(&mut payload, 0);
    push_u64(&mut payload, message.capabilities);
    push_u64(&mut payload, message.connection_epoch);
    push_u16(&mut payload, message.max_heads);
    push_u16(&mut payload, message.max_groups);
    push_u16(&mut payload, message.max_modes_per_head);
    push_u16(&mut payload, message.max_heads_per_group);
    encode_frame(
        IpcMessageKind::OutputV1ServerWelcome,
        TransactionId::INVALID,
        &payload,
    )
}

pub fn decode_output_v1_server_welcome_frame(
    frame: &[u8],
) -> Result<OutputV1ServerWelcome, IpcCodecError> {
    let (_, mut cursor) = decode_payload(frame, IpcMessageKind::OutputV1ServerWelcome, false)?;
    let selected_revision = cursor.u16()?;
    require_reserved_u16(&mut cursor)?;
    let message = OutputV1ServerWelcome {
        selected_revision,
        capabilities: cursor.u64()?,
        connection_epoch: cursor.u64()?,
        max_heads: cursor.u16()?,
        max_groups: cursor.u16()?,
        max_modes_per_head: cursor.u16()?,
        max_heads_per_group: cursor.u16()?,
    };
    cursor.finish()?;
    if message.selected_revision == 0
        || message.connection_epoch == 0
        || message.max_heads == 0
        || message.max_groups == 0
        || message.max_modes_per_head == 0
        || message.max_heads_per_group == 0
    {
        return Err(invalid("server_welcome"));
    }
    Ok(message)
}

pub fn encode_output_v1_snapshot_frame(
    transaction: TransactionId,
    message: &OutputV1Snapshot,
) -> Result<Vec<u8>, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(transaction.raw()));
    }
    if message.connection_epoch == 0 {
        return Err(invalid("connection_epoch"));
    }
    message
        .snapshot
        .validate()
        .map_err(|_| invalid("snapshot"))?;
    let mut payload = Vec::new();
    push_u64(&mut payload, message.connection_epoch);
    push_u64(&mut payload, message.snapshot.topology_epoch);
    push_u64(&mut payload, message.snapshot.primary_output.raw());
    push_u16(&mut payload, message.snapshot.heads.len() as u16);
    push_u16(&mut payload, message.snapshot.groups.len() as u16);
    push_u32(&mut payload, 0);
    for head in &message.snapshot.heads {
        push_u64(&mut payload, head.head.raw());
        push_u64(&mut payload, head.generation);
        let mut flags = 0u16;
        if head.connected {
            flags |= 1;
        }
        if head.enabled {
            flags |= 1 << 1;
        }
        if head.vrr_capable {
            flags |= 1 << 2;
        }
        push_u16(&mut payload, flags);
        push_u16(&mut payload, head.transforms.bits());
        push_u64(
            &mut payload,
            head.current_mode.unwrap_or(DisplayModeId::INVALID).raw(),
        );
        push_u16(&mut payload, head.label.len() as u16);
        push_u16(&mut payload, head.modes.len() as u16);
        push_u32(&mut payload, 0);
        payload.extend_from_slice(head.label.as_bytes());
        for mode in &head.modes {
            push_u64(&mut payload, mode.mode.raw());
            push_i32(&mut payload, mode.pixel_size.width);
            push_i32(&mut payload, mode.pixel_size.height);
            push_u32(&mut payload, mode.refresh_millihz);
            push_u16(&mut payload, u16::from(mode.preferred));
            push_u16(&mut payload, 0);
        }
    }
    for group in &message.snapshot.groups {
        encode_group_state(&mut payload, group);
    }
    encode_frame(IpcMessageKind::OutputV1Snapshot, transaction, &payload)
}

pub fn decode_output_v1_snapshot_frame(
    frame: &[u8],
) -> Result<(TransactionId, OutputV1Snapshot), IpcCodecError> {
    let (transaction, mut cursor) = decode_payload(frame, IpcMessageKind::OutputV1Snapshot, true)?;
    let connection_epoch = cursor.u64()?;
    if connection_epoch == 0 {
        return Err(invalid("connection_epoch"));
    }
    let topology_epoch = cursor.u64()?;
    let primary_output = OutputId::from_raw(cursor.u64()?);
    let head_count = usize::from(cursor.u16()?);
    let group_count = usize::from(cursor.u16()?);
    require_reserved_u32(&mut cursor)?;
    require_count(head_count, MAX_OUTPUT_AUTHORITY_HEADS)?;
    require_count(group_count, MAX_OUTPUT_AUTHORITY_GROUPS)?;
    let mut heads = Vec::with_capacity(head_count);
    for _ in 0..head_count {
        let head = DisplayHeadId::from_raw(cursor.u64()?);
        let generation = cursor.u64()?;
        let flags = cursor.u16()?;
        if flags & !0b111 != 0 {
            return Err(invalid("head_flags"));
        }
        let transforms = OutputTransformSet::from_bits(cursor.u16()?)
            .ok_or_else(|| invalid("head_transforms"))?;
        let current_mode = DisplayModeId::from_raw(cursor.u64()?);
        let label_len = usize::from(cursor.u16()?);
        let mode_count = usize::from(cursor.u16()?);
        require_reserved_u32(&mut cursor)?;
        require_count(label_len, MAX_OUTPUT_AUTHORITY_LABEL_BYTES)?;
        require_count(mode_count, MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD)?;
        let label = std::str::from_utf8(cursor.slice(label_len)?)
            .map_err(|_| IpcCodecError::InvalidUtf8 {
                field: "head_label",
            })?
            .to_owned();
        let mut modes = Vec::with_capacity(mode_count);
        for _ in 0..mode_count {
            let mode = DisplayModeId::from_raw(cursor.u64()?);
            let pixel_size = Size {
                width: cursor.i32()?,
                height: cursor.i32()?,
            };
            let refresh_millihz = cursor.u32()?;
            let preferred = decode_bool_u16(cursor.u16()?, "mode_preferred")?;
            require_reserved_u16(&mut cursor)?;
            modes.push(OutputModeDescriptor {
                mode,
                pixel_size,
                refresh_millihz,
                preferred,
            });
        }
        heads.push(OutputHeadDescriptor {
            head,
            generation,
            label,
            connected: flags & 1 != 0,
            enabled: flags & (1 << 1) != 0,
            current_mode: current_mode.is_valid().then_some(current_mode),
            transforms,
            vrr_capable: flags & (1 << 2) != 0,
            modes,
        });
    }
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        groups.push(decode_group_state(&mut cursor)?);
    }
    cursor.finish()?;
    let snapshot = OutputAuthoritySnapshot {
        topology_epoch,
        primary_output,
        heads,
        groups,
    };
    snapshot.validate().map_err(|_| invalid("snapshot"))?;
    Ok((
        transaction,
        OutputV1Snapshot {
            connection_epoch,
            snapshot,
        },
    ))
}

pub fn encode_output_v1_proposal_frame(
    transaction: TransactionId,
    message: &OutputV1Proposal,
) -> Result<Vec<u8>, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(transaction.raw()));
    }
    if message.connection_epoch == 0
        || message.candidate.base_topology_epoch == 0
        || message.candidate.heads.is_empty()
        || message.candidate.groups.is_empty()
        || message.candidate.heads.len() > MAX_OUTPUT_AUTHORITY_HEADS
        || message.candidate.groups.len() > MAX_OUTPUT_AUTHORITY_GROUPS
    {
        return Err(invalid("proposal"));
    }
    let mut payload = Vec::new();
    push_u64(&mut payload, message.connection_epoch);
    push_u64(&mut payload, message.candidate.base_topology_epoch);
    push_u16(&mut payload, encode_intent(message.candidate.intent));
    push_u16(&mut payload, message.candidate.primary_group_index);
    push_u16(&mut payload, message.candidate.heads.len() as u16);
    push_u16(&mut payload, message.candidate.groups.len() as u16);
    for target in &message.candidate.heads {
        push_u64(&mut payload, target.head.raw());
        push_u64(&mut payload, target.head_generation);
        push_u64(&mut payload, target.mode.raw());
        push_u16(&mut payload, encode_transform(target.transform));
        push_u16(&mut payload, encode_vrr(target.vrr));
        push_u32(&mut payload, 0);
    }
    for group in &message.candidate.groups {
        if group.members.is_empty() || group.members.len() > MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP {
            return Err(invalid("group_members"));
        }
        push_u64(&mut payload, group.output.raw());
        push_i32(&mut payload, group.logical.x);
        push_i32(&mut payload, group.logical.y);
        push_i32(&mut payload, group.logical.width);
        push_i32(&mut payload, group.logical.height);
        push_u16(&mut payload, group.members.len() as u16);
        push_u16(&mut payload, 0);
        for member in &group.members {
            encode_member(&mut payload, *member);
        }
    }
    encode_frame(IpcMessageKind::OutputV1Proposal, transaction, &payload)
}

pub fn decode_output_v1_proposal_frame(
    frame: &[u8],
) -> Result<(TransactionId, OutputV1Proposal), IpcCodecError> {
    let (transaction, mut cursor) = decode_payload(frame, IpcMessageKind::OutputV1Proposal, true)?;
    let connection_epoch = cursor.u64()?;
    let base_topology_epoch = cursor.u64()?;
    let intent = decode_intent(cursor.u16()?)?;
    let primary_group_index = cursor.u16()?;
    let head_count = usize::from(cursor.u16()?);
    let group_count = usize::from(cursor.u16()?);
    require_count(head_count, MAX_OUTPUT_AUTHORITY_HEADS)?;
    require_count(group_count, MAX_OUTPUT_AUTHORITY_GROUPS)?;
    if connection_epoch == 0 || base_topology_epoch == 0 || head_count == 0 || group_count == 0 {
        return Err(invalid("proposal"));
    }
    let mut heads = Vec::with_capacity(head_count);
    for _ in 0..head_count {
        let head = DisplayHeadId::from_raw(cursor.u64()?);
        let head_generation = cursor.u64()?;
        let mode = DisplayModeId::from_raw(cursor.u64()?);
        let transform = decode_transform(cursor.u16()?)?;
        let vrr = decode_vrr(cursor.u16()?)?;
        require_reserved_u32(&mut cursor)?;
        heads.push(OutputHeadTargetProposal {
            head,
            head_generation,
            mode,
            transform,
            vrr,
        });
    }
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let output = OutputId::from_raw(cursor.u64()?);
        let logical = Rect {
            x: cursor.i32()?,
            y: cursor.i32()?,
            width: cursor.i32()?,
            height: cursor.i32()?,
        };
        let member_count = usize::from(cursor.u16()?);
        require_reserved_u16(&mut cursor)?;
        require_count(member_count, MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP)?;
        if member_count == 0 {
            return Err(invalid("group_members"));
        }
        let mut members = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            members.push(decode_member(&mut cursor)?);
        }
        groups.push(OutputLogicalGroupProposal {
            output,
            logical,
            members,
        });
    }
    cursor.finish()?;
    Ok((
        transaction,
        OutputV1Proposal {
            connection_epoch,
            candidate: OutputTopologyCandidate {
                base_topology_epoch,
                intent,
                primary_group_index,
                heads,
                groups,
            },
        },
    ))
}

pub fn encode_output_v1_outcome_frame(
    transaction: TransactionId,
    message: OutputV1Outcome,
) -> Result<Vec<u8>, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(transaction.raw()));
    }
    if message.connection_epoch == 0 || message.topology_epoch == 0 {
        return Err(invalid("outcome"));
    }
    let mut payload = Vec::with_capacity(20);
    push_u64(&mut payload, message.connection_epoch);
    push_u64(&mut payload, message.topology_epoch);
    push_u16(&mut payload, encode_outcome(message.kind));
    push_u16(&mut payload, message.reason);
    encode_frame(IpcMessageKind::OutputV1Outcome, transaction, &payload)
}

pub fn decode_output_v1_outcome_frame(
    frame: &[u8],
) -> Result<(TransactionId, OutputV1Outcome), IpcCodecError> {
    let (transaction, mut cursor) = decode_payload(frame, IpcMessageKind::OutputV1Outcome, true)?;
    let message = OutputV1Outcome {
        connection_epoch: cursor.u64()?,
        topology_epoch: cursor.u64()?,
        kind: decode_outcome(cursor.u16()?)?,
        reason: cursor.u16()?,
    };
    cursor.finish()?;
    if message.connection_epoch == 0 || message.topology_epoch == 0 {
        return Err(invalid("outcome"));
    }
    Ok((transaction, message))
}

fn encode_group_state(payload: &mut Vec<u8>, group: &OutputLogicalGroupState) {
    push_u64(payload, group.output.raw());
    push_u64(payload, group.generation);
    push_i32(payload, group.logical.x);
    push_i32(payload, group.logical.y);
    push_i32(payload, group.logical.width);
    push_i32(payload, group.logical.height);
    push_u16(payload, group.members.len() as u16);
    push_u16(payload, 0);
    for member in &group.members {
        encode_member(payload, *member);
    }
}

fn decode_group_state(cursor: &mut Cursor<'_>) -> Result<OutputLogicalGroupState, IpcCodecError> {
    let output = OutputId::from_raw(cursor.u64()?);
    let generation = cursor.u64()?;
    let logical = Rect {
        x: cursor.i32()?,
        y: cursor.i32()?,
        width: cursor.i32()?,
        height: cursor.i32()?,
    };
    let member_count = usize::from(cursor.u16()?);
    require_reserved_u16(cursor)?;
    require_count(member_count, MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP)?;
    let mut members = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        members.push(decode_member(cursor)?);
    }
    Ok(OutputLogicalGroupState {
        output,
        generation,
        logical,
        members,
    })
}

fn encode_member(payload: &mut Vec<u8>, member: OutputGroupMember) {
    push_u64(payload, member.head.raw());
    push_u16(payload, encode_mapping(member.mapping));
    push_u16(payload, 0);
}

fn decode_member(cursor: &mut Cursor<'_>) -> Result<OutputGroupMember, IpcCodecError> {
    let head = DisplayHeadId::from_raw(cursor.u64()?);
    let mapping = decode_mapping(cursor.u16()?)?;
    require_reserved_u16(cursor)?;
    Ok(OutputGroupMember { head, mapping })
}

fn decode_payload(
    frame: &[u8],
    expected: IpcMessageKind,
    transaction_required: bool,
) -> Result<(TransactionId, Cursor<'_>), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    if header.message_kind != expected {
        return Err(IpcCodecError::InvalidEnum {
            field: "message_kind",
            value: header.message_kind as u32,
        });
    }
    if header.transaction.is_valid() != transaction_required {
        return Err(IpcCodecError::InvalidTransaction(header.transaction.raw()));
    }
    Ok((header.transaction, Cursor::new(payload)))
}

fn require_count(count: usize, maximum: usize) -> Result<(), IpcCodecError> {
    if count > maximum {
        Err(IpcCodecError::CountTooLarge {
            count,
            max: maximum,
        })
    } else {
        Ok(())
    }
}

fn require_reserved_u16(cursor: &mut Cursor<'_>) -> Result<(), IpcCodecError> {
    let reserved = cursor.u16()?;
    if reserved == 0 {
        Ok(())
    } else {
        Err(IpcCodecError::ReservedNonZero(u32::from(reserved)))
    }
}

fn require_reserved_u32(cursor: &mut Cursor<'_>) -> Result<(), IpcCodecError> {
    let reserved = cursor.u32()?;
    if reserved == 0 {
        Ok(())
    } else {
        Err(IpcCodecError::ReservedNonZero(reserved))
    }
}

fn decode_bool_u16(value: u16, field: &'static str) -> Result<bool, IpcCodecError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(IpcCodecError::InvalidBool {
            field,
            value: u8::MAX,
        }),
    }
}

const fn encode_intent(value: OutputTopologyIntent) -> u16 {
    match value {
        OutputTopologyIntent::ValidateOnly => 1,
        OutputTopologyIntent::Apply => 2,
    }
}

fn decode_intent(value: u16) -> Result<OutputTopologyIntent, IpcCodecError> {
    match value {
        1 => Ok(OutputTopologyIntent::ValidateOnly),
        2 => Ok(OutputTopologyIntent::Apply),
        _ => Err(invalid_value("topology_intent", value)),
    }
}

const fn encode_transform(value: OutputTransform) -> u16 {
    value as u16 + 1
}

fn decode_transform(value: u16) -> Result<OutputTransform, IpcCodecError> {
    match value {
        1 => Ok(OutputTransform::Normal),
        2 => Ok(OutputTransform::Rotate90),
        3 => Ok(OutputTransform::Rotate180),
        4 => Ok(OutputTransform::Rotate270),
        5 => Ok(OutputTransform::Flipped),
        6 => Ok(OutputTransform::Flipped90),
        7 => Ok(OutputTransform::Flipped180),
        8 => Ok(OutputTransform::Flipped270),
        _ => Err(invalid_value("transform", value)),
    }
}

const fn encode_vrr(value: OutputVrrPolicy) -> u16 {
    match value {
        OutputVrrPolicy::Disabled => 1,
        OutputVrrPolicy::Automatic => 2,
        OutputVrrPolicy::Always => 3,
    }
}

fn decode_vrr(value: u16) -> Result<OutputVrrPolicy, IpcCodecError> {
    match value {
        1 => Ok(OutputVrrPolicy::Disabled),
        2 => Ok(OutputVrrPolicy::Automatic),
        3 => Ok(OutputVrrPolicy::Always),
        _ => Err(invalid_value("vrr", value)),
    }
}

const fn encode_mapping(value: OutputHeadMapping) -> u16 {
    match value {
        OutputHeadMapping::Fit => 1,
        OutputHeadMapping::Cover => 2,
        OutputHeadMapping::Exact => 3,
    }
}

fn decode_mapping(value: u16) -> Result<OutputHeadMapping, IpcCodecError> {
    match value {
        1 => Ok(OutputHeadMapping::Fit),
        2 => Ok(OutputHeadMapping::Cover),
        3 => Ok(OutputHeadMapping::Exact),
        _ => Err(invalid_value("head_mapping", value)),
    }
}

const fn encode_outcome(value: OutputV1OutcomeKind) -> u16 {
    match value {
        OutputV1OutcomeKind::Validated => 1,
        OutputV1OutcomeKind::Committed => 2,
        OutputV1OutcomeKind::Stale => 3,
        OutputV1OutcomeKind::Rejected => 4,
        OutputV1OutcomeKind::RolledBack => 5,
        OutputV1OutcomeKind::Failed => 6,
    }
}

fn decode_outcome(value: u16) -> Result<OutputV1OutcomeKind, IpcCodecError> {
    match value {
        1 => Ok(OutputV1OutcomeKind::Validated),
        2 => Ok(OutputV1OutcomeKind::Committed),
        3 => Ok(OutputV1OutcomeKind::Stale),
        4 => Ok(OutputV1OutcomeKind::Rejected),
        5 => Ok(OutputV1OutcomeKind::RolledBack),
        6 => Ok(OutputV1OutcomeKind::Failed),
        _ => Err(invalid_value("output_outcome", value)),
    }
}

fn invalid(field: &'static str) -> IpcCodecError {
    IpcCodecError::InvalidEnum { field, value: 0 }
}

fn invalid_value(field: &'static str, value: u16) -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field,
        value: u32::from(value),
    }
}
