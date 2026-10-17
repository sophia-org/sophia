use super::{invalid, nonzero, reserved};
use crate::byte_cursor::{Cursor, push_i32, push_u16, push_u32, push_u64};
use crate::{
    BinaryCodecError, DisplayHeadId, DisplayModeId, MAX_OUTPUT_AUTHORITY_GROUPS,
    MAX_OUTPUT_AUTHORITY_HEADS, MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP, OutputGroupMember,
    OutputHeadMapping, OutputHeadTargetProposal, OutputId, OutputLogicalGroupProposal,
    OutputTopologyCandidate, OutputTopologyIntent, OutputTransform, OutputV1Proposal,
    OutputVrrPolicy, Rect, TransactionId,
};

const PREFIX_BYTES: usize = 24;
const HEAD_BYTES: usize = 32;
const GROUP_BYTES: usize = 76;

fn count(value: usize, maximum: usize) -> Result<usize, BinaryCodecError> {
    if value == 0 || value > maximum {
        return Err(invalid("row_count"));
    }
    Ok(value)
}

/// Decodes fixed rows without consulting a topology snapshot. Unknown heads,
/// modes, generations, negative geometry and out-of-range primary indices must
/// reach the existing owner's semantic refusal path, rather than being lost as
/// file syntax errors. Counts and enum encodings are structural constraints.
pub fn decode_output_file_proposal(
    bytes: &[u8],
    connection_epoch: u64,
) -> Result<(TransactionId, OutputV1Proposal), BinaryCodecError> {
    nonzero(connection_epoch, "connection_epoch")?;
    let mut cursor = Cursor::new(bytes);
    let transaction = TransactionId::from_raw(nonzero(cursor.u64()?, "transaction")?);
    let base_topology_epoch = nonzero(cursor.u64()?, "base_topology_epoch")?;
    let intent = match cursor.u16()? {
        1 => OutputTopologyIntent::ValidateOnly,
        2 => OutputTopologyIntent::Apply,
        _ => return Err(invalid("intent")),
    };
    let primary_group_index = cursor.u16()?;
    let heads_len = count(usize::from(cursor.u16()?), MAX_OUTPUT_AUTHORITY_HEADS)?;
    let groups_len = count(usize::from(cursor.u16()?), MAX_OUTPUT_AUTHORITY_GROUPS)?;
    // Check the complete shape before allocating, including the fixed member
    // padding. Counts are bounded above, so this arithmetic cannot overflow.
    if bytes.len() != PREFIX_BYTES + heads_len * HEAD_BYTES + groups_len * GROUP_BYTES {
        return Err(invalid("proposal_length"));
    }
    let mut heads = Vec::with_capacity(heads_len);
    for _ in 0..heads_len {
        let target = OutputHeadTargetProposal {
            head: DisplayHeadId::from_raw(cursor.u64()?),
            head_generation: cursor.u64()?,
            mode: DisplayModeId::from_raw(cursor.u64()?),
            transform: match cursor.u16()? {
                1 => OutputTransform::Normal,
                2 => OutputTransform::Rotate90,
                3 => OutputTransform::Rotate180,
                4 => OutputTransform::Rotate270,
                5 => OutputTransform::Flipped,
                6 => OutputTransform::Flipped90,
                7 => OutputTransform::Flipped180,
                8 => OutputTransform::Flipped270,
                _ => return Err(invalid("transform")),
            },
            vrr: match cursor.u16()? {
                1 => OutputVrrPolicy::Disabled,
                2 => OutputVrrPolicy::Automatic,
                3 => OutputVrrPolicy::Always,
                _ => return Err(invalid("vrr")),
            },
        };
        reserved(&mut cursor, 4)?;
        heads.push(target);
    }
    let mut groups = Vec::with_capacity(groups_len);
    for _ in 0..groups_len {
        let output = OutputId::from_raw(cursor.u64()?);
        let logical = Rect {
            x: cursor.i32()?,
            y: cursor.i32()?,
            width: cursor.i32()?,
            height: cursor.i32()?,
        };
        let members_len = count(
            usize::from(cursor.u16()?),
            MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP,
        )?;
        reserved(&mut cursor, 2)?;
        let mut members = Vec::with_capacity(members_len);
        for _ in 0..members_len {
            let head = DisplayHeadId::from_raw(cursor.u64()?);
            let mapping = match cursor.u16()? {
                1 => OutputHeadMapping::Fit,
                2 => OutputHeadMapping::Cover,
                3 => OutputHeadMapping::Exact,
                _ => return Err(invalid("mapping")),
            };
            reserved(&mut cursor, 2)?;
            members.push(OutputGroupMember { head, mapping });
        }
        reserved(
            &mut cursor,
            (MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP - members_len) * 12,
        )?;
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

pub fn encode_output_file_proposal(
    transaction: TransactionId,
    proposal: &OutputV1Proposal,
) -> Result<Vec<u8>, BinaryCodecError> {
    nonzero(transaction.raw(), "transaction")?;
    nonzero(proposal.connection_epoch, "connection_epoch")?;
    let candidate = &proposal.candidate;
    nonzero(candidate.base_topology_epoch, "base_topology_epoch")?;
    let heads = count(candidate.heads.len(), MAX_OUTPUT_AUTHORITY_HEADS)?;
    let groups = count(candidate.groups.len(), MAX_OUTPUT_AUTHORITY_GROUPS)?;
    for group in &candidate.groups {
        count(group.members.len(), MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP)?;
    }
    let mut bytes = Vec::with_capacity(PREFIX_BYTES + heads * HEAD_BYTES + groups * GROUP_BYTES);
    push_u64(&mut bytes, transaction.raw());
    push_u64(&mut bytes, candidate.base_topology_epoch);
    push_u16(
        &mut bytes,
        match candidate.intent {
            OutputTopologyIntent::ValidateOnly => 1,
            OutputTopologyIntent::Apply => 2,
        },
    );
    push_u16(&mut bytes, candidate.primary_group_index);
    push_u16(&mut bytes, heads as u16);
    push_u16(&mut bytes, groups as u16);
    for head in &candidate.heads {
        push_u64(&mut bytes, head.head.raw());
        push_u64(&mut bytes, head.head_generation);
        push_u64(&mut bytes, head.mode.raw());
        push_u16(
            &mut bytes,
            match head.transform {
                OutputTransform::Normal => 1,
                OutputTransform::Rotate90 => 2,
                OutputTransform::Rotate180 => 3,
                OutputTransform::Rotate270 => 4,
                OutputTransform::Flipped => 5,
                OutputTransform::Flipped90 => 6,
                OutputTransform::Flipped180 => 7,
                OutputTransform::Flipped270 => 8,
            },
        );
        push_u16(
            &mut bytes,
            match head.vrr {
                OutputVrrPolicy::Disabled => 1,
                OutputVrrPolicy::Automatic => 2,
                OutputVrrPolicy::Always => 3,
            },
        );
        push_u32(&mut bytes, 0);
    }
    for group in &candidate.groups {
        push_u64(&mut bytes, group.output.raw());
        push_i32(&mut bytes, group.logical.x);
        push_i32(&mut bytes, group.logical.y);
        push_i32(&mut bytes, group.logical.width);
        push_i32(&mut bytes, group.logical.height);
        push_u16(&mut bytes, group.members.len() as u16);
        push_u16(&mut bytes, 0);
        for member in &group.members {
            push_u64(&mut bytes, member.head.raw());
            push_u16(
                &mut bytes,
                match member.mapping {
                    OutputHeadMapping::Fit => 1,
                    OutputHeadMapping::Cover => 2,
                    OutputHeadMapping::Exact => 3,
                },
            );
            push_u16(&mut bytes, 0);
        }
        bytes.resize(
            bytes.len() + (MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP - group.members.len()) * 12,
            0,
        );
    }
    Ok(bytes)
}
