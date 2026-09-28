use super::{invalid, nonzero, reserved};
use crate::byte_cursor::{Cursor, push_i32, push_u16, push_u32, push_u64};
use crate::{
    BinaryCodecError, DisplayHeadId, DisplayModeId, MAX_OUTPUT_AUTHORITY_GROUPS,
    MAX_OUTPUT_AUTHORITY_HEADS, MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP,
    MAX_OUTPUT_AUTHORITY_LABEL_BYTES, MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD, OutputAuthoritySnapshot,
    OutputGroupMember, OutputHeadDescriptor, OutputHeadMapping, OutputId, OutputLogicalGroupState,
    OutputModeDescriptor, OutputTransformSet, OutputV1Snapshot, Rect, Size,
};

const PREFIX_BYTES: usize = 24;
const HEAD_BYTES: usize = 104;
const MODE_BYTES: usize = 24;
const GROUP_BYTES: usize = 84;
const MAX_MODES: usize = MAX_OUTPUT_AUTHORITY_HEADS * MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD;

pub const OUTPUT_FILE_MAX_TOPOLOGY_BYTES: usize = super::OUTPUT_FILE_HEADER_BYTES
    + PREFIX_BYTES
    + MAX_OUTPUT_AUTHORITY_HEADS * HEAD_BYTES
    + MAX_MODES * MODE_BYTES
    + MAX_OUTPUT_AUTHORITY_GROUPS * GROUP_BYTES;

fn count(value: usize, maximum: usize) -> Result<usize, BinaryCodecError> {
    if value == 0 || value > maximum {
        return Err(invalid("topology_count"));
    }
    Ok(value)
}

/// Decode a whole topology object. The flattened mode table must be tiled
/// exactly once, in head order. Both wire shape and snapshot invariants are
/// checked: unlike a proposal, an advertised snapshot is already authoritative.
pub fn decode_output_file_topology(
    bytes: &[u8],
    connection_epoch: u64,
) -> Result<OutputV1Snapshot, BinaryCodecError> {
    nonzero(connection_epoch, "connection_epoch")?;
    let mut cursor = Cursor::new(bytes);
    let topology_epoch = cursor.u64()?;
    let primary_output = OutputId::from_raw(cursor.u64()?);
    let head_count = count(cursor.u16()?.into(), MAX_OUTPUT_AUTHORITY_HEADS)?;
    let group_count = count(cursor.u16()?.into(), MAX_OUTPUT_AUTHORITY_GROUPS)?;
    let mode_count = count(cursor.u16()?.into(), MAX_MODES)?;
    reserved(&mut cursor, 2)?;
    if bytes.len()
        != PREFIX_BYTES
            + head_count * HEAD_BYTES
            + mode_count * MODE_BYTES
            + group_count * GROUP_BYTES
    {
        return Err(invalid("topology_length"));
    }
    let mut heads = Vec::with_capacity(head_count);
    let mut mode_counts = Vec::with_capacity(head_count);
    let mut covered_modes = 0;
    for _ in 0..head_count {
        let head = DisplayHeadId::from_raw(cursor.u64()?);
        let generation = cursor.u64()?;
        let flags = cursor.u16()?;
        if flags & !7 != 0 {
            return Err(invalid("head_flags"));
        }
        let transforms =
            OutputTransformSet::from_bits(cursor.u16()?).ok_or_else(|| invalid("transforms"))?;
        let label_len = count(cursor.u16()?.into(), MAX_OUTPUT_AUTHORITY_LABEL_BYTES)?;
        let modes_len = count(cursor.u16()?.into(), MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD)?;
        let current_mode = DisplayModeId::from_raw(cursor.u64()?);
        let first_mode = usize::from(cursor.u16()?);
        reserved(&mut cursor, 6)?;
        if first_mode != covered_modes || covered_modes + modes_len > mode_count {
            return Err(invalid("mode_range"));
        }
        covered_modes += modes_len;
        let label = std::str::from_utf8(cursor.slice(label_len)?)
            .map_err(|_| BinaryCodecError::InvalidUtf8 { field: "label" })?
            .to_owned();
        reserved(&mut cursor, MAX_OUTPUT_AUTHORITY_LABEL_BYTES - label_len)?;
        heads.push(OutputHeadDescriptor {
            head,
            generation,
            label,
            connected: flags & 1 != 0,
            enabled: flags & 2 != 0,
            vrr_capable: flags & 4 != 0,
            transforms,
            current_mode: current_mode.is_valid().then_some(current_mode),
            modes: Vec::new(),
        });
        mode_counts.push(modes_len);
    }
    if covered_modes != mode_count {
        return Err(invalid("mode_range"));
    }
    for (head, count) in heads.iter_mut().zip(mode_counts) {
        head.modes.reserve_exact(count);
        for _ in 0..count {
            let mode = DisplayModeId::from_raw(cursor.u64()?);
            let pixel_size = Size {
                width: cursor.i32()?,
                height: cursor.i32()?,
            };
            let refresh_millihz = cursor.u32()?;
            let preferred = match cursor.u16()? {
                0 => false,
                1 => true,
                _ => return Err(invalid("preferred")),
            };
            reserved(&mut cursor, 2)?;
            head.modes.push(OutputModeDescriptor {
                mode,
                pixel_size,
                refresh_millihz,
                preferred,
            });
        }
    }
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let output = OutputId::from_raw(cursor.u64()?);
        let generation = cursor.u64()?;
        let logical = Rect {
            x: cursor.i32()?,
            y: cursor.i32()?,
            width: cursor.i32()?,
            height: cursor.i32()?,
        };
        let members_len = count(cursor.u16()?.into(), MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP)?;
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
        groups.push(OutputLogicalGroupState {
            output,
            generation,
            logical,
            members,
        });
    }
    cursor.finish()?;
    let snapshot = OutputAuthoritySnapshot {
        topology_epoch,
        primary_output,
        heads,
        groups,
    };
    snapshot.validate().map_err(|_| invalid("snapshot"))?;
    Ok(OutputV1Snapshot {
        connection_epoch,
        snapshot,
    })
}

pub fn encode_output_file_topology(
    message: &OutputV1Snapshot,
) -> Result<Vec<u8>, BinaryCodecError> {
    nonzero(message.connection_epoch, "connection_epoch")?;
    let snapshot = &message.snapshot;
    snapshot.validate().map_err(|_| invalid("snapshot"))?;
    // A zero current_mode encodes None. Refuse a noncanonical Some(INVALID)
    // rather than silently changing that value on a decode/encode round trip.
    if snapshot
        .heads
        .iter()
        .any(|head| head.current_mode.is_some_and(|mode| !mode.is_valid()))
    {
        return Err(invalid("current_mode"));
    }
    let modes: usize = snapshot.heads.iter().map(|head| head.modes.len()).sum();
    let size = PREFIX_BYTES
        + snapshot.heads.len() * HEAD_BYTES
        + modes * MODE_BYTES
        + snapshot.groups.len() * GROUP_BYTES;
    let mut bytes = Vec::with_capacity(size);
    push_u64(&mut bytes, snapshot.topology_epoch);
    push_u64(&mut bytes, snapshot.primary_output.raw());
    push_u16(&mut bytes, snapshot.heads.len() as u16);
    push_u16(&mut bytes, snapshot.groups.len() as u16);
    push_u16(&mut bytes, modes as u16);
    push_u16(&mut bytes, 0);
    let mut first_mode = 0;
    for head in &snapshot.heads {
        push_u64(&mut bytes, head.head.raw());
        push_u64(&mut bytes, head.generation);
        let flags = u16::from(head.connected)
            | (u16::from(head.enabled) << 1)
            | (u16::from(head.vrr_capable) << 2);
        push_u16(&mut bytes, flags);
        push_u16(&mut bytes, head.transforms.bits());
        push_u16(&mut bytes, head.label.len() as u16);
        push_u16(&mut bytes, head.modes.len() as u16);
        push_u64(&mut bytes, head.current_mode.map_or(0, DisplayModeId::raw));
        push_u16(&mut bytes, first_mode);
        first_mode += head.modes.len() as u16;
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(head.label.as_bytes());
        bytes.resize(
            bytes.len() + MAX_OUTPUT_AUTHORITY_LABEL_BYTES - head.label.len(),
            0,
        );
    }
    for head in &snapshot.heads {
        for mode in &head.modes {
            push_u64(&mut bytes, mode.mode.raw());
            push_i32(&mut bytes, mode.pixel_size.width);
            push_i32(&mut bytes, mode.pixel_size.height);
            push_u32(&mut bytes, mode.refresh_millihz);
            push_u16(&mut bytes, u16::from(mode.preferred));
            push_u16(&mut bytes, 0);
        }
    }
    for group in &snapshot.groups {
        push_u64(&mut bytes, group.output.raw());
        push_u64(&mut bytes, group.generation);
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
