//! Complete passive record sections shared by transport codecs. This owner
//! has no transfer phases, transaction assembly, queue or publication state.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyDecodedSnapshot {
    pub launch_origins: Vec<crate::PolicyLaunchContext>,
    pub scene: crate::PolicySceneSnapshot,
    pub actions: Vec<crate::PolicyActionRegistration>,
    pub classifications: Vec<crate::PolicySurfaceClassification>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRecordSection {
    pub kind: u16,
    pub count: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyRecordSectionRef<'a> {
    pub kind: u16,
    pub count: u32,
    pub bytes: &'a [u8],
}

impl PolicyRecordSection {
    pub fn as_ref(&self) -> PolicyRecordSectionRef<'_> {
        PolicyRecordSectionRef {
            kind: self.kind,
            count: self.count,
            bytes: &self.bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicySnapshotMetadata {
    pub connection_epoch: u64,
    pub scene_generation: u64,
    pub active_output: crate::OutputId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProjectionMetadata {
    pub transaction: crate::TransactionId,
    pub connection_epoch: u64,
    pub request_id: u64,
    pub base_generation: u64,
    pub active_output: crate::OutputId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyConfigurationMetadata {
    pub connection_epoch: u64,
    pub generation: u64,
    pub chrome: crate::WmChromePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyRecordContext {
    Snapshot,
    Projection,
    Configuration,
}

/// Row widths and aggregate bounds, independent of either transport framing.
pub fn policy_record_layout(context: PolicyRecordContext, kind: u16) -> Option<(usize, usize)> {
    use PolicyRecordContext::*;
    Some(match (context, kind) {
        (Configuration, SNAPSHOT_ACTION_RECORD_KIND) => {
            (SNAPSHOT_ACTION_RECORD_SIZE, SNAPSHOT_ACTION_RECORD_MAX)
        }
        (Snapshot, SNAPSHOT_OUTPUT_RECORD_KIND) => {
            (SNAPSHOT_OUTPUT_RECORD_SIZE, SNAPSHOT_OUTPUT_RECORD_MAX)
        }
        (Snapshot, SNAPSHOT_SURFACE_RECORD_KIND) => {
            (SNAPSHOT_SURFACE_RECORD_SIZE, SNAPSHOT_SURFACE_RECORD_MAX)
        }
        (Snapshot, SNAPSHOT_ACTION_RECORD_KIND) => {
            (SNAPSHOT_ACTION_RECORD_SIZE, SNAPSHOT_ACTION_RECORD_MAX)
        }
        (Snapshot, SNAPSHOT_SESSION_OPERATION_RECORD_KIND) => (
            SNAPSHOT_SESSION_OPERATION_RECORD_SIZE,
            SNAPSHOT_SESSION_OPERATION_RECORD_MAX,
        ),
        (Snapshot, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND) => (16, crate::POLICY_MAX_SURFACES),
        (Snapshot, SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND) => {
            (LAUNCH_CONTEXT_RECORD_LEN, crate::POLICY_MAX_SURFACES)
        }
        (Snapshot, SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND) => (24, crate::POLICY_MAX_OUTPUTS),
        (Projection, PROJECTION_OUTPUT_RECORD_KIND) => {
            (PROJECTION_OUTPUT_RECORD_SIZE, PROJECTION_OUTPUT_RECORD_MAX)
        }
        (Projection, PROJECTION_PLACEMENT_RECORD_KIND) => (
            PROJECTION_PLACEMENT_RECORD_SIZE,
            PROJECTION_PLACEMENT_RECORD_MAX,
        ),
        (Projection, PROJECTION_INDICATOR_RECORD_KIND) => (
            PROJECTION_INDICATOR_RECORD_SIZE,
            PROJECTION_INDICATOR_RECORD_MAX,
        ),
        (Projection, PROJECTION_OUTPUT_STATUS_RECORD_KIND) => (
            PROJECTION_OUTPUT_STATUS_RECORD_SIZE,
            PROJECTION_OUTPUT_STATUS_RECORD_MAX,
        ),
        (Projection, PROJECTION_TAB_GROUP_RECORD_KIND) => (
            PROJECTION_TAB_GROUP_RECORD_LEN,
            crate::POLICY_MAX_TAB_GROUPS,
        ),
        (Projection, PROJECTION_TAB_MEMBER_RECORD_KIND) => (
            PROJECTION_TAB_MEMBER_RECORD_LEN,
            crate::POLICY_MAX_TAB_MEMBERS,
        ),
        (Projection, PROJECTION_TRANSLATION_GROUP_RECORD_KIND) => (
            PROJECTION_TRANSLATION_GROUP_RECORD_LEN,
            crate::POLICY_MAX_OUTPUTS,
        ),
        (Projection, PROJECTION_TRANSLATION_MEMBER_RECORD_KIND) => (
            PROJECTION_TRANSLATION_MEMBER_RECORD_LEN,
            crate::POLICY_MAX_SURFACES,
        ),
        (Projection, PROJECTION_LAUNCH_CONTEXT_RECORD_KIND) => {
            (LAUNCH_CONTEXT_RECORD_LEN, crate::POLICY_MAX_SURFACES)
        }
        (Projection, PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND) => {
            (OUTPUT_LAUNCH_CONTEXT_RECORD_LEN, crate::POLICY_MAX_OUTPUTS)
        }
        (Projection, other) => {
            let (size, maximum, _) = wm_presentation_record_layout(other)?;
            (size, maximum)
        }
        _ => return None,
    })
}

fn invalid_section(kind: u16) -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "policy_record_section",
        value: u32::from(kind),
    }
}

/// Validate every aggregate before allocating row vectors or a coalesced copy.
/// No file-envelope limit or legacy chunk order is inferred here.
pub fn validate_policy_record_sections(
    context: PolicyRecordContext,
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<(), IpcCodecError> {
    for (index, section) in sections.iter().enumerate() {
        let (size, maximum) = policy_record_layout(context, section.kind)
            .ok_or_else(|| invalid_section(section.kind))?;
        let count = usize::try_from(section.count).map_err(|_| invalid_section(section.kind))?;
        if count == 0 || count.checked_mul(size) != Some(section.bytes.len()) {
            return Err(invalid_section(section.kind));
        }
        let mut total = 0_usize;
        for prior in &sections[..=index] {
            if prior.kind == section.kind {
                total = total
                    .checked_add(prior.count as usize)
                    .ok_or_else(|| invalid_section(section.kind))?;
            }
        }
        if total > maximum {
            return Err(IpcCodecError::CountTooLarge {
                count: total,
                max: maximum,
            });
        }
    }
    Ok(())
}

/// Coalesce complete rows after bounded preflight. Input order within each
/// kind is retained. Framing owners must validate their own ordering first.
pub fn coalesce_policy_record_sections(
    context: PolicyRecordContext,
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<Vec<PolicyRecordSection>, IpcCodecError> {
    validate_policy_record_sections(context, sections)?;
    let mut result: Vec<PolicyRecordSection> = Vec::new();
    for section in sections {
        if let Some(existing) = result.iter_mut().find(|r| r.kind == section.kind) {
            existing.count += section.count;
            existing.bytes.extend_from_slice(section.bytes);
        } else {
            result.push(PolicyRecordSection {
                kind: section.kind,
                count: section.count,
                bytes: section.bytes.to_vec(),
            });
        }
    }
    result.sort_by_key(|s| s.kind);
    Ok(result)
}
