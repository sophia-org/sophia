use super::{IpcCodecError, PolicyRecordSection, PolicyRecordSectionRef, WmV1ProjectionChunk};
use crate::{
    OutputId, POLICY_MAX_TAB_GROUPS, POLICY_MAX_TAB_MEMBERS, PolicyTabGroup, Rect, SurfaceId,
};

pub const PROJECTION_TAB_GROUP_RECORD_KIND: u16 = 0xff01;
pub const PROJECTION_TAB_MEMBER_RECORD_KIND: u16 = 0xff02;
pub const PROJECTION_TAB_GROUP_RECORD_LEN: usize = 48;
pub const PROJECTION_TAB_MEMBER_RECORD_LEN: usize = 24;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "tab_group",
        value: 0,
    }
}

pub fn encode_wm_tab_groups(
    groups: &[PolicyTabGroup],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    super::wm_record_sections::projection_chunks(
        encode_policy_tab_groups_records(groups)?,
        epoch,
        ordinal,
        |kind| match kind {
            PROJECTION_TAB_GROUP_RECORD_KIND => Some(48),
            PROJECTION_TAB_MEMBER_RECORD_KIND => Some(24),
            _ => None,
        },
    )
    .map_err(|_| invalid())
}

pub fn decode_wm_tab_groups(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyTabGroup>, IpcCodecError> {
    decode_policy_tab_groups_records(&super::wm_record_sections::projection_sections(chunks))
}

pub fn encode_policy_tab_groups_records(
    groups: &[PolicyTabGroup],
) -> Result<Vec<PolicyRecordSection>, IpcCodecError> {
    if groups.len() > POLICY_MAX_TAB_GROUPS
        || groups.iter().map(|g| g.members.len()).sum::<usize>() > POLICY_MAX_TAB_MEMBERS
    {
        return Err(invalid());
    }
    let mut headers = Vec::new();
    let mut members = Vec::new();
    for g in groups {
        headers.extend(g.output.raw().to_le_bytes());
        headers.extend(g.group.to_le_bytes());
        for n in [
            g.geometry.x,
            g.geometry.y,
            g.geometry.width,
            g.geometry.height,
        ] {
            headers.extend(n.to_le_bytes());
        }
        let s = g.selected.unwrap_or(SurfaceId::INVALID);
        headers.extend(s.index().to_le_bytes());
        headers.extend(s.generation().to_le_bytes());
        headers.extend((g.members.len() as u32).to_le_bytes());
        headers.extend(u32::from(g.focused).to_le_bytes());
        for s in &g.members {
            members.extend(g.output.raw().to_le_bytes());
            members.extend(g.group.to_le_bytes());
            members.extend(s.index().to_le_bytes());
            members.extend(s.generation().to_le_bytes());
        }
    }
    let mut sections = Vec::new();
    for (kind, size, data) in [
        (PROJECTION_TAB_GROUP_RECORD_KIND, 48, headers),
        (PROJECTION_TAB_MEMBER_RECORD_KIND, 24, members),
    ] {
        if !data.is_empty() {
            sections.push(PolicyRecordSection {
                kind,
                count: (data.len() / size) as u32,
                bytes: data,
            });
        }
    }
    Ok(sections)
}

pub fn decode_policy_tab_groups_records(
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<Vec<PolicyTabGroup>, IpcCodecError> {
    let mut groups = Vec::new();
    let mut expected = Vec::new();
    let mut member_records = Vec::new();
    for c in sections {
        let size = match c.kind {
            PROJECTION_TAB_GROUP_RECORD_KIND => 48,
            PROJECTION_TAB_MEMBER_RECORD_KIND => 24,
            _ => continue,
        };
        if c.count == 0 || c.bytes.len() != c.count as usize * size {
            return Err(invalid());
        }
        for b in c.bytes.chunks_exact(size) {
            let u64_at = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
            let u32_at = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
            if size == 48 {
                if groups.len() == POLICY_MAX_TAB_GROUPS || u32_at(44) > 1 {
                    return Err(invalid());
                }
                let i32_at = |i| i32::from_le_bytes(b[i..i + 4].try_into().unwrap());
                let selected = match (u32_at(32), u32_at(36)) {
                    (0, 0) => None,
                    (0, _) | (_, 0) => return Err(invalid()),
                    (i, g) => Some(SurfaceId::new(i, g)),
                };
                groups.push(PolicyTabGroup {
                    output: OutputId::from_raw(u64_at(0)),
                    group: u64_at(8),
                    geometry: Rect {
                        x: i32_at(16),
                        y: i32_at(20),
                        width: i32_at(24),
                        height: i32_at(28),
                    },
                    focused: u32_at(44) == 1,
                    selected,
                    members: Vec::new(),
                });
                expected.push(u32_at(40) as usize);
            } else {
                if member_records.len() == POLICY_MAX_TAB_MEMBERS {
                    return Err(invalid());
                }
                member_records.push((u64_at(0), u64_at(8), SurfaceId::new(u32_at(16), u32_at(20))));
            }
        }
    }
    let mut cursor = member_records.into_iter();
    for (g, count) in groups.iter_mut().zip(expected) {
        if count > POLICY_MAX_TAB_MEMBERS {
            return Err(invalid());
        }
        for _ in 0..count {
            let (output, id, surface) = cursor.next().ok_or_else(invalid)?;
            if output != g.output.raw() || id != g.group || !surface.is_valid() {
                return Err(invalid());
            }
            g.members.push(surface);
        }
    }
    if cursor.next().is_some() {
        return Err(invalid());
    }
    Ok(groups)
}
