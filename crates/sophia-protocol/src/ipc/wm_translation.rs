use super::{IpcCodecError, PolicyRecordSection, PolicyRecordSectionRef, WmV1ProjectionChunk};
use crate::{OutputId, POLICY_MAX_OUTPUTS, POLICY_MAX_SURFACES, PolicyTranslationGroup, SurfaceId};

pub const PROJECTION_TRANSLATION_GROUP_RECORD_KIND: u16 = 0xff03;
pub const PROJECTION_TRANSLATION_MEMBER_RECORD_KIND: u16 = 0xff04;
pub const PROJECTION_TRANSLATION_GROUP_RECORD_LEN: usize = 32;
pub const PROJECTION_TRANSLATION_MEMBER_RECORD_LEN: usize = 24;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "translation_group",
        value: 0,
    }
}

pub fn encode_wm_translation_groups(
    groups: &[PolicyTranslationGroup],
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    super::wm_record_sections::projection_chunks(
        encode_policy_translation_groups_records(groups)?,
        epoch,
        ordinal,
        |kind| match kind {
            PROJECTION_TRANSLATION_GROUP_RECORD_KIND => {
                Some(PROJECTION_TRANSLATION_GROUP_RECORD_LEN)
            }
            PROJECTION_TRANSLATION_MEMBER_RECORD_KIND => {
                Some(PROJECTION_TRANSLATION_MEMBER_RECORD_LEN)
            }
            _ => None,
        },
    )
    .map_err(|_| invalid())
}

pub fn decode_wm_translation_groups(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Vec<PolicyTranslationGroup>, IpcCodecError> {
    decode_policy_translation_groups_records(&super::wm_record_sections::projection_sections(
        chunks,
    ))
}

pub fn encode_policy_translation_groups_records(
    groups: &[PolicyTranslationGroup],
) -> Result<Vec<PolicyRecordSection>, IpcCodecError> {
    if groups.len() > POLICY_MAX_OUTPUTS
        || groups.iter().map(|g| g.members.len()).sum::<usize>() > POLICY_MAX_SURFACES
    {
        return Err(invalid());
    }
    let mut headers = Vec::new();
    let mut members = Vec::new();
    for group in groups {
        headers.extend(group.output.raw().to_le_bytes());
        headers.extend(group.group.to_le_bytes());
        headers.extend(group.x.to_le_bytes());
        headers.extend(group.y.to_le_bytes());
        headers.extend((group.members.len() as u32).to_le_bytes());
        headers.extend(0_u32.to_le_bytes());
        for surface in &group.members {
            members.extend(group.output.raw().to_le_bytes());
            members.extend(group.group.to_le_bytes());
            members.extend(surface.index().to_le_bytes());
            members.extend(surface.generation().to_le_bytes());
        }
    }
    let mut sections = Vec::new();
    for (kind, size, bytes) in [
        (
            PROJECTION_TRANSLATION_GROUP_RECORD_KIND,
            PROJECTION_TRANSLATION_GROUP_RECORD_LEN,
            headers,
        ),
        (
            PROJECTION_TRANSLATION_MEMBER_RECORD_KIND,
            PROJECTION_TRANSLATION_MEMBER_RECORD_LEN,
            members,
        ),
    ] {
        if !bytes.is_empty() {
            sections.push(PolicyRecordSection {
                kind,
                count: (bytes.len() / size) as u32,
                bytes,
            });
        }
    }
    Ok(sections)
}

pub fn decode_policy_translation_groups_records(
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<Vec<PolicyTranslationGroup>, IpcCodecError> {
    let mut groups = Vec::new();
    let mut counts = Vec::new();
    let mut members = Vec::new();
    for chunk in sections {
        let size = match chunk.kind {
            PROJECTION_TRANSLATION_GROUP_RECORD_KIND => PROJECTION_TRANSLATION_GROUP_RECORD_LEN,
            PROJECTION_TRANSLATION_MEMBER_RECORD_KIND => PROJECTION_TRANSLATION_MEMBER_RECORD_LEN,
            _ => continue,
        };
        if chunk.count == 0 || chunk.bytes.len() != chunk.count as usize * size {
            return Err(invalid());
        }
        for b in chunk.bytes.chunks_exact(size) {
            let u64_at = |i| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
            let u32_at = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
            if size == 32 {
                if groups.len() == POLICY_MAX_OUTPUTS
                    || u32_at(28) != 0
                    || u32_at(24) == 0
                    || u32_at(24) as usize > POLICY_MAX_SURFACES
                {
                    return Err(invalid());
                }
                groups.push(PolicyTranslationGroup {
                    output: OutputId::from_raw(u64_at(0)),
                    group: u64_at(8),
                    x: u32_at(16) as i32,
                    y: u32_at(20) as i32,
                    members: Vec::new(),
                });
                counts.push(u32_at(24) as usize);
            } else {
                if members.len() == POLICY_MAX_SURFACES {
                    return Err(invalid());
                }
                members.push((u64_at(0), u64_at(8), SurfaceId::new(u32_at(16), u32_at(20))));
            }
        }
    }
    let mut members = members.into_iter();
    for (group, count) in groups.iter_mut().zip(counts) {
        if !group.output.is_valid() || group.group == 0 {
            return Err(invalid());
        }
        for _ in 0..count {
            let (output, id, surface) = members.next().ok_or_else(invalid)?;
            if output != group.output.raw() || id != group.group || !surface.is_valid() {
                return Err(invalid());
            }
            group.members.push(surface);
        }
    }
    if members.next().is_some() {
        return Err(invalid());
    }
    Ok(groups)
}
