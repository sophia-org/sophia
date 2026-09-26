//! Complete file bodies delegate all row conversion to the neutral owner.
//! Negotiated capabilities and file identities are checked before delivery;
//! Session still owns correlation, admission, publication and final validation,
//! including capability-dependent row content and output coverage. Section
//! capability refusal here is deliberately stricter than the legacy codec.
use super::codec::{u16_at, u32_at, u64_at, validate_header};
use super::*;
use crate::*;

impl From<WmFileCodecError> for WmFilePayloadError {
    fn from(error: WmFileCodecError) -> Self {
        Self::Envelope(error)
    }
}

impl From<IpcCodecError> for WmFilePayloadError {
    fn from(error: IpcCodecError) -> Self {
        Self::Records(error)
    }
}

pub(super) fn require_capabilities(selected: u64, required: u64) -> Result<(), WmFilePayloadError> {
    let missing = required & !selected;
    if missing == 0 {
        Ok(())
    } else {
        Err(WmFilePayloadError::Capabilities { missing })
    }
}

fn header_kind(header: WmFileHeader, kind: WmFileKind) -> Result<(), WmFilePayloadError> {
    validate_header(header)?;
    if header.kind != kind {
        return Err(WmFileCodecError::Kind.into());
    }
    Ok(())
}

fn record(
    bytes: &[u8],
    kind: WmFileKind,
    prefix: usize,
) -> Result<WmFileRecord<'_>, WmFilePayloadError> {
    let record = decode_wm_file_record(bytes, wm_file_class(kind))?;
    header_kind(record.header, kind)?;
    if record.body.len() < prefix {
        return Err(WmFileCodecError::Length.into());
    }
    Ok(record)
}

fn reserved(bytes: &[u8]) -> Result<(), WmFilePayloadError> {
    if bytes.iter().any(|value| *value != 0) {
        return Err(WmFileCodecError::Reserved.into());
    }
    Ok(())
}

fn section_capabilities(context: PolicyRecordContext, kind: u16) -> u64 {
    match (context, kind) {
        (
            PolicyRecordContext::Snapshot | PolicyRecordContext::Configuration,
            SNAPSHOT_ACTION_RECORD_KIND,
        ) => SOPHIA_WM_CAPABILITY_ACTIONS,
        (PolicyRecordContext::Snapshot, SNAPSHOT_SESSION_OPERATION_RECORD_KIND) => {
            SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS
        }
        (PolicyRecordContext::Snapshot, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND) => {
            SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT
        }
        (PolicyRecordContext::Snapshot, SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND) => {
            SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
        }
        (PolicyRecordContext::Snapshot, SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND) => {
            SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS
        }
        (
            PolicyRecordContext::Projection,
            PROJECTION_INDICATOR_RECORD_KIND | PROJECTION_OUTPUT_STATUS_RECORD_KIND,
        ) => SOPHIA_WM_CAPABILITY_INDICATORS,
        (
            PolicyRecordContext::Projection,
            PROJECTION_TAB_GROUP_RECORD_KIND | PROJECTION_TAB_MEMBER_RECORD_KIND,
        ) => SOPHIA_WM_CAPABILITY_TAB_GROUPS,
        (
            PolicyRecordContext::Projection,
            PROJECTION_TRANSLATION_GROUP_RECORD_KIND | PROJECTION_TRANSLATION_MEMBER_RECORD_KIND,
        ) => SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS,
        (PolicyRecordContext::Projection, PROJECTION_LAUNCH_CONTEXT_RECORD_KIND) => {
            SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
        }
        (PolicyRecordContext::Projection, PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND) => {
            SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN | SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT
        }
        (PolicyRecordContext::Projection, other) => {
            wm_presentation_record_layout(other).map_or(0, |layout| layout.2)
        }
        _ => 0,
    }
}

fn validate_sections(
    context: PolicyRecordContext,
    sections: &[PolicyRecordSectionRef<'_>],
    capabilities: u64,
) -> Result<(), WmFilePayloadError> {
    // Shape and aggregate validation precede every row allocation. Unknown
    // kinds cannot inherit the zero-capability result of the mapping above.
    validate_policy_record_sections(context, sections)?;
    let output_kind = match context {
        PolicyRecordContext::Snapshot => Some(SNAPSHOT_OUTPUT_RECORD_KIND),
        PolicyRecordContext::Projection => Some(PROJECTION_OUTPUT_RECORD_KIND),
        PolicyRecordContext::Configuration => None,
    };
    if let Some(kind) = output_kind
        && !sections.iter().any(|section| section.kind == kind)
    {
        return Err(WmFileCodecError::Sections.into());
    }
    for section in sections {
        require_capabilities(capabilities, section_capabilities(context, section.kind))?;
    }
    Ok(())
}

fn decode_sections(
    bytes: &[u8],
    count: u16,
    context: PolicyRecordContext,
    capabilities: u64,
) -> Result<Vec<PolicyRecordSectionRef<'_>>, WmFilePayloadError> {
    let sections = decode_wm_file_sections(bytes, count)?
        .into_iter()
        .map(|s| PolicyRecordSectionRef {
            kind: s.kind,
            count: s.count,
            bytes: s.bytes,
        })
        .collect::<Vec<_>>();
    validate_sections(context, &sections, capabilities)?;
    Ok(sections)
}

fn finish(
    header: WmFileHeader,
    mut prefix: Vec<u8>,
    sections: &[PolicyRecordSection],
    count_offset: usize,
    context: PolicyRecordContext,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    let refs = sections
        .iter()
        .map(PolicyRecordSection::as_ref)
        .collect::<Vec<_>>();
    validate_sections(context, &refs, capabilities)?;
    let wire = refs
        .iter()
        .map(|s| WmFileSection {
            kind: s.kind,
            count: s.count,
            bytes: s.bytes,
        })
        .collect::<Vec<_>>();
    let rows = encode_wm_file_sections(&wire)?;
    let size = prefix
        .len()
        .checked_add(rows.len())
        .and_then(|size| size.checked_add(WM_FILE_HEADER_BYTES))
        .ok_or(WmFileCodecError::Length)?;
    if size > WM_FILE_MAX_BYTES {
        return Err(WmFileCodecError::Length.into());
    }
    let count = u16::try_from(sections.len()).map_err(|_| WmFileCodecError::Sections)?;
    prefix[count_offset..count_offset + 2].copy_from_slice(&count.to_le_bytes());
    prefix.extend(rows);
    Ok(encode_wm_file_record(header, &prefix)?)
}

fn identity(values: &[u64]) -> Result<(), WmFilePayloadError> {
    if values.contains(&0) {
        return Err(WmFilePayloadError::Identity);
    }
    Ok(())
}

pub fn encode_wm_file_snapshot(
    header: WmFileHeader,
    snapshot: &WmFileSnapshot,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Snapshot)?;
    let scene = &snapshot.snapshot.scene;
    identity(&[
        snapshot.transaction.raw(),
        scene.generation,
        scene.active_output.raw(),
    ])?;
    let sections = encode_policy_snapshot_records(
        header.connection_epoch,
        scene,
        &snapshot.snapshot.actions,
        &snapshot.snapshot.classifications,
        &snapshot.snapshot.launch_origins,
        capabilities,
    )?;
    let mut prefix = Vec::with_capacity(WM_FILE_SNAPSHOT_PREFIX_BYTES);
    prefix.extend(snapshot.transaction.raw().to_le_bytes());
    prefix.extend(scene.generation.to_le_bytes());
    prefix.extend(scene.active_output.raw().to_le_bytes());
    prefix.extend([0; 8]);
    finish(
        header,
        prefix,
        &sections,
        24,
        PolicyRecordContext::Snapshot,
        capabilities,
    )
}

pub fn decode_wm_file_snapshot(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFileSnapshot, WmFilePayloadError> {
    let r = record(bytes, WmFileKind::Snapshot, WM_FILE_SNAPSHOT_PREFIX_BYTES)?;
    reserved(&r.body[26..32])?;
    let transaction = u64_at(r.body, 0)?;
    let scene_generation = u64_at(r.body, 8)?;
    let active_output = u64_at(r.body, 16)?;
    identity(&[transaction, scene_generation, active_output])?;
    let sections = decode_sections(
        &r.body[32..],
        u16_at(r.body, 24)?,
        PolicyRecordContext::Snapshot,
        capabilities,
    )?;
    let snapshot = decode_policy_snapshot_records(
        PolicySnapshotMetadata {
            connection_epoch: r.header.connection_epoch,
            scene_generation,
            active_output: OutputId::from_raw(active_output),
        },
        &sections,
    )?;
    if !snapshot
        .scene
        .outputs
        .iter()
        .any(|output| output.output == snapshot.scene.active_output)
    {
        return Err(WmFilePayloadError::Identity);
    }
    Ok(WmFileSnapshot {
        transaction: TransactionId::from_raw(transaction),
        snapshot,
    })
}

fn presentation_capabilities(
    proposal: &PolicyProjectionProposal,
    capabilities: u64,
) -> Result<(), WmFilePayloadError> {
    if let Some(presentation) = &proposal.presentation
        && (!presentation.bindings.is_empty()
            || presentation.instances.iter().any(|i| i.action.is_some())
            || presentation.regions.iter().any(|r| r.action.is_some()))
    {
        require_capabilities(
            capabilities,
            SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        )?;
    }
    Ok(())
}

pub fn encode_wm_file_projection(
    header: WmFileHeader,
    proposal: &PolicyProjectionProposal,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Projection)?;
    if proposal.connection_epoch != header.connection_epoch {
        return Err(WmFilePayloadError::Identity);
    }
    identity(&[
        proposal.transaction.raw(),
        proposal.request_id,
        proposal.base_generation,
        proposal.active_output.raw(),
    ])?;
    presentation_capabilities(proposal, capabilities)?;
    let sections = encode_policy_projection_records(proposal)?;
    let mut prefix = Vec::with_capacity(WM_FILE_PROJECTION_PREFIX_BYTES);
    for value in [
        proposal.transaction.raw(),
        proposal.request_id,
        proposal.base_generation,
        proposal.active_output.raw(),
    ] {
        prefix.extend(value.to_le_bytes());
    }
    prefix.extend([0; 8]);
    finish(
        header,
        prefix,
        &sections,
        32,
        PolicyRecordContext::Projection,
        capabilities,
    )
}

pub fn decode_wm_file_projection(
    bytes: &[u8],
    capabilities: u64,
) -> Result<PolicyProjectionProposal, WmFilePayloadError> {
    let r = record(
        bytes,
        WmFileKind::Projection,
        WM_FILE_PROJECTION_PREFIX_BYTES,
    )?;
    reserved(&r.body[34..40])?;
    let transaction = u64_at(r.body, 0)?;
    let request_id = u64_at(r.body, 8)?;
    let base_generation = u64_at(r.body, 16)?;
    let active_output = u64_at(r.body, 24)?;
    identity(&[transaction, request_id, base_generation, active_output])?;
    let sections = decode_sections(
        &r.body[40..],
        u16_at(r.body, 32)?,
        PolicyRecordContext::Projection,
        capabilities,
    )?;
    let proposal = decode_policy_projection_records(
        PolicyProjectionMetadata {
            transaction: TransactionId::from_raw(transaction),
            connection_epoch: r.header.connection_epoch,
            request_id,
            base_generation,
            active_output: OutputId::from_raw(active_output),
        },
        &sections,
    )?;
    presentation_capabilities(&proposal, capabilities)?;
    Ok(proposal)
}

fn rgb(color: WmRgb8) -> u32 {
    u32::from(color.red) << 16 | u32::from(color.green) << 8 | u32::from(color.blue)
}

fn decode_rgb(value: u32) -> Result<WmRgb8, WmFilePayloadError> {
    let [blue, green, red, reserved] = value.to_le_bytes();
    if reserved != 0 {
        return Err(WmFileCodecError::Reserved.into());
    }
    Ok(WmRgb8 { red, green, blue })
}

fn configuration_capabilities(
    configuration: &PolicyConfiguration,
    capabilities: u64,
) -> Result<(), WmFilePayloadError> {
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_CONFIGURATION)?;
    if configuration.chrome.focus_ring.enabled || configuration.chrome.frame.enabled {
        require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_CHROME)?;
    }
    Ok(())
}

pub fn encode_wm_file_configuration(
    header: WmFileHeader,
    configuration: &WmFileConfiguration,
    capabilities: u64,
) -> Result<Vec<u8>, WmFilePayloadError> {
    header_kind(header, WmFileKind::Configuration)?;
    let value = &configuration.configuration;
    if value.connection_epoch != header.connection_epoch {
        return Err(WmFilePayloadError::Identity);
    }
    identity(&[configuration.transaction.raw(), value.generation])?;
    configuration_capabilities(value, capabilities)?;
    let sections = encode_policy_configuration_records(value)?;
    let chrome = value.chrome;
    let mut prefix = Vec::with_capacity(WM_FILE_CONFIGURATION_PREFIX_BYTES);
    prefix.extend(configuration.transaction.raw().to_le_bytes());
    prefix.extend(value.generation.to_le_bytes());
    prefix.extend(
        (u16::from(chrome.focus_ring.enabled) | u16::from(chrome.frame.enabled) << 1).to_le_bytes(),
    );
    prefix.extend(0u16.to_le_bytes());
    for value in [
        chrome.focus_ring.width,
        rgb(chrome.focus_ring.color),
        chrome.frame.width,
        rgb(chrome.frame.focused_color),
        rgb(chrome.frame.unfocused_color),
    ] {
        prefix.extend(value.to_le_bytes());
    }
    prefix.extend([0; 8]);
    finish(
        header,
        prefix,
        &sections,
        18,
        PolicyRecordContext::Configuration,
        capabilities,
    )
}

pub fn decode_wm_file_configuration(
    bytes: &[u8],
    capabilities: u64,
) -> Result<WmFileConfiguration, WmFilePayloadError> {
    let r = record(
        bytes,
        WmFileKind::Configuration,
        WM_FILE_CONFIGURATION_PREFIX_BYTES,
    )?;
    require_capabilities(capabilities, SOPHIA_WM_CAPABILITY_CONFIGURATION)?;
    reserved(&r.body[40..48])?;
    let transaction = u64_at(r.body, 0)?;
    let generation = u64_at(r.body, 8)?;
    identity(&[transaction, generation])?;
    let style = u16_at(r.body, 16)?;
    if style & !3 != 0 {
        return Err(WmFileCodecError::Reserved.into());
    }
    let sections = decode_sections(
        &r.body[48..],
        u16_at(r.body, 18)?,
        PolicyRecordContext::Configuration,
        capabilities,
    )?;
    let configuration = decode_policy_configuration_records(
        PolicyConfigurationMetadata {
            connection_epoch: r.header.connection_epoch,
            generation,
            chrome: WmChromePolicy {
                focus_ring: WmFocusRingStyle {
                    enabled: style & 1 != 0,
                    width: u32_at(r.body, 20)?,
                    color: decode_rgb(u32_at(r.body, 24)?)?,
                },
                frame: WmFrameStyle {
                    enabled: style & 2 != 0,
                    width: u32_at(r.body, 28)?,
                    focused_color: decode_rgb(u32_at(r.body, 32)?)?,
                    unfocused_color: decode_rgb(u32_at(r.body, 36)?)?,
                },
            },
        },
        &sections,
    )?;
    configuration_capabilities(&configuration, capabilities)?;
    Ok(WmFileConfiguration {
        transaction: TransactionId::from_raw(transaction),
        configuration,
    })
}
