//! Projection rows: per-output placements, indicators and statuses, plus the
//! group, bookmark and presentation extensions. Connection and scene
//! authority belong to the projection owner, not to these rows.
use crate::{
    BinaryCodecError, OutputId, PolicyOutputProjection, PolicyProjectionIndicator,
    PolicyProjectionOutputStatus, PolicyProjectionProposal, PolicySurfacePlacement,
    PolicyTransform, Rect, SurfaceId, WmActionId,
};
// Raw generated rows: root-exported names until `crate::wm_rows` owns them.
use crate::{
    PROJECTION_INDICATOR_RECORD_KIND, PROJECTION_OUTPUT_RECORD_KIND,
    PROJECTION_OUTPUT_STATUS_RECORD_KIND, PROJECTION_PLACEMENT_RECORD_KIND,
    WmV1ProjectionIndicatorRecord, WmV1ProjectionOutputRecord, WmV1ProjectionOutputStatusRecord,
    WmV1ProjectionPlacementRecord, decode_wm_v1_projection_indicator_records,
    decode_wm_v1_projection_output_records, decode_wm_v1_projection_output_status_records,
    decode_wm_v1_projection_placement_records, encode_wm_v1_projection_indicator_records,
    encode_wm_v1_projection_output_records, encode_wm_v1_projection_output_status_records,
    encode_wm_v1_projection_placement_records,
};

use super::values::{
    decode_optional_size, decode_optional_surface, decode_presentation, encode_optional_size,
    encode_presentation, invalid, push_policy_section, require_count,
};
use super::{
    PROJECTION_LAUNCH_CONTEXT_RECORD_KIND, PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
    PROJECTION_PRESENTATION_BINDING_RECORD_KIND, PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND,
    PROJECTION_PRESENTATION_RECORD_KIND, PROJECTION_PRESENTATION_REGION_RECORD_KIND,
    PROJECTION_SURFACE_INSTANCE_RECORD_KIND, PROJECTION_TAB_GROUP_RECORD_KIND,
    PROJECTION_TAB_MEMBER_RECORD_KIND, PROJECTION_TRANSLATION_GROUP_RECORD_KIND,
    PROJECTION_TRANSLATION_MEMBER_RECORD_KIND, PolicyProjectionMetadata, PolicyRecordContext,
    PolicyRecordSection, PolicyRecordSectionRef, decode_policy_launch_contexts_records,
    decode_policy_output_launch_contexts_records, decode_policy_presentation_records,
    decode_policy_tab_groups_records, decode_policy_translation_groups_records,
    encode_policy_launch_contexts_records, encode_policy_output_launch_contexts_records,
    encode_policy_presentation_records, encode_policy_tab_groups_records,
    encode_policy_translation_groups_records, validate_policy_record_sections,
};

pub fn encode_policy_projection_records(
    proposal: &PolicyProjectionProposal,
) -> Result<Vec<PolicyRecordSection>, BinaryCodecError> {
    if !proposal.transaction.is_valid() {
        return Err(BinaryCodecError::InvalidTransaction(0));
    }
    if !proposal.active_output.is_valid() {
        return Err(invalid("projection_active_output", 0));
    }
    let outputs = proposal
        .outputs
        .iter()
        .map(|output| {
            let (focus_index, focus_generation) = output
                .focus
                .map(|surface| (surface.index(), surface.generation()))
                .unwrap_or((0, 0));
            Ok(WmV1ProjectionOutputRecord {
                output: output.output.raw(),
                placement_count: u32::try_from(output.placements.len()).map_err(|_| {
                    BinaryCodecError::CountTooLarge {
                        count: output.placements.len(),
                        max: u32::MAX as usize,
                    }
                })?,
                focus_index,
                focus_generation,
            })
        })
        .collect::<Result<Vec<_>, BinaryCodecError>>()?;
    let placements = proposal
        .outputs
        .iter()
        .flat_map(|output| output.placements.iter())
        .map(encode_placement_record)
        .collect::<Vec<_>>();
    let indicators = proposal
        .indicators
        .iter()
        .map(|indicator| {
            let (label_len, label) = encode_indicator_text(&indicator.label, "indicator_label")?;
            Ok(WmV1ProjectionIndicatorRecord {
                output: indicator.output.raw(),
                slot: indicator.slot,
                indicator: indicator.indicator,
                action: indicator.action.map_or(0, WmActionId::raw),
                state_bits: indicator.state_bits,
                label_len,
                label,
            })
        })
        .collect::<Result<Vec<_>, BinaryCodecError>>()?;
    let statuses = proposal
        .output_statuses
        .iter()
        .map(|status| {
            let (layout_len, layout) = encode_indicator_text(&status.layout, "status_layout")?;
            Ok(WmV1ProjectionOutputStatusRecord {
                output: status.output.raw(),
                focus_bits: status.focus_bits,
                layout_len,
                layout,
            })
        })
        .collect::<Result<Vec<_>, BinaryCodecError>>()?;
    let mut sections = Vec::new();
    push_policy_section(
        &mut sections,
        PROJECTION_OUTPUT_RECORD_KIND,
        outputs.len(),
        encode_wm_v1_projection_output_records(&outputs)?,
    )?;
    push_policy_section(
        &mut sections,
        PROJECTION_PLACEMENT_RECORD_KIND,
        placements.len(),
        encode_wm_v1_projection_placement_records(&placements)?,
    )?;
    push_policy_section(
        &mut sections,
        PROJECTION_INDICATOR_RECORD_KIND,
        indicators.len(),
        encode_wm_v1_projection_indicator_records(&indicators)?,
    )?;
    push_policy_section(
        &mut sections,
        PROJECTION_OUTPUT_STATUS_RECORD_KIND,
        statuses.len(),
        encode_wm_v1_projection_output_status_records(&statuses)?,
    )?;
    sections.extend(encode_policy_tab_groups_records(&proposal.tab_groups)?);
    sections.extend(encode_policy_translation_groups_records(
        &proposal.translation_groups,
    )?);
    sections.extend(encode_policy_launch_contexts_records(
        &proposal.launch_contexts,
        proposal.connection_epoch,
    )?);
    sections.extend(encode_policy_output_launch_contexts_records(
        &proposal.output_launch_contexts,
        proposal.connection_epoch,
    )?);
    sections.extend(encode_policy_presentation_records(
        proposal.presentation.as_ref(),
        proposal.connection_epoch,
    )?);
    Ok(sections)
}

pub fn decode_policy_projection_records(
    metadata: PolicyProjectionMetadata,
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<PolicyProjectionProposal, BinaryCodecError> {
    if !metadata.transaction.is_valid() {
        return Err(BinaryCodecError::InvalidTransaction(0));
    }
    if !metadata.active_output.is_valid() {
        return Err(invalid("projection_transfer", 0));
    }
    validate_policy_record_sections(PolicyRecordContext::Projection, sections)?;
    decode_projection_sections(metadata, sections, None)
}

/// Decode complete sections. `declared` holds the output, placement,
/// indicator and status counts when an envelope announces them apart from its
/// rows; they are checked after the rows decode, before any value is built.
pub(crate) fn decode_projection_sections(
    metadata: PolicyProjectionMetadata,
    sections: &[PolicyRecordSectionRef<'_>],
    declared: Option<[usize; 4]>,
) -> Result<PolicyProjectionProposal, BinaryCodecError> {
    let mut outputs = Vec::new();
    let mut placements = Vec::new();
    let mut indicators = Vec::new();
    let mut statuses = Vec::new();
    for chunk in sections {
        match chunk.kind {
            PROJECTION_OUTPUT_RECORD_KIND => outputs.extend(
                decode_wm_v1_projection_output_records(chunk.bytes, chunk.count)?,
            ),
            PROJECTION_PLACEMENT_RECORD_KIND => placements.extend(
                decode_wm_v1_projection_placement_records(chunk.bytes, chunk.count)?,
            ),
            PROJECTION_INDICATOR_RECORD_KIND => indicators.extend(
                decode_wm_v1_projection_indicator_records(chunk.bytes, chunk.count)?,
            ),
            PROJECTION_OUTPUT_STATUS_RECORD_KIND => statuses.extend(
                decode_wm_v1_projection_output_status_records(chunk.bytes, chunk.count)?,
            ),
            PROJECTION_TAB_GROUP_RECORD_KIND
            | PROJECTION_TAB_MEMBER_RECORD_KIND
            | PROJECTION_TRANSLATION_GROUP_RECORD_KIND
            | PROJECTION_TRANSLATION_MEMBER_RECORD_KIND
            | PROJECTION_LAUNCH_CONTEXT_RECORD_KIND
            | PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND
            | PROJECTION_PRESENTATION_RECORD_KIND
            | PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND
            | PROJECTION_SURFACE_INSTANCE_RECORD_KIND
            | PROJECTION_PRESENTATION_REGION_RECORD_KIND
            | PROJECTION_PRESENTATION_BINDING_RECORD_KIND => {}
            other => return Err(invalid("projection_record_kind", u32::from(other))),
        }
    }
    if let Some(counts) = declared {
        require_count(outputs.len(), counts[0])?;
        require_count(placements.len(), counts[1])?;
        require_count(indicators.len(), counts[2])?;
        require_count(statuses.len(), counts[3])?;
    }
    let mut placement_cursor = placements.into_iter();
    let mut projected_outputs = Vec::with_capacity(outputs.len());
    for output in outputs {
        let focus = decode_optional_surface(output.focus_index, output.focus_generation, "focus")?;
        // The declared count is untrusted; do not reserve from it before the
        // shared cursor establishes that the corresponding rows exist.
        let mut projected = Vec::new();
        for _ in 0..output.placement_count {
            let record = placement_cursor
                .next()
                .ok_or_else(|| invalid("placement_count", output.placement_count))?;
            projected.push(decode_placement_record(record)?);
        }
        projected_outputs.push(PolicyOutputProjection {
            output: OutputId::from_raw(output.output),
            placements: projected,
            focus,
        });
    }
    if placement_cursor.next().is_some() {
        return Err(invalid(
            "placement_count",
            declared.map_or(0, |c| c[1] as u32),
        ));
    }
    Ok(PolicyProjectionProposal {
        presentation: decode_policy_presentation_records(sections)?,
        launch_contexts: decode_policy_launch_contexts_records(
            metadata.connection_epoch,
            sections,
        )?,
        output_launch_contexts: decode_policy_output_launch_contexts_records(
            metadata.connection_epoch,
            sections,
        )?,
        translation_groups: decode_policy_translation_groups_records(sections)?,
        tab_groups: decode_policy_tab_groups_records(sections)?,
        transaction: metadata.transaction,
        connection_epoch: metadata.connection_epoch,
        request_id: metadata.request_id,
        base_generation: metadata.base_generation,
        active_output: metadata.active_output,
        outputs: projected_outputs,
        indicators: indicators
            .into_iter()
            .map(|record| {
                Ok(PolicyProjectionIndicator {
                    output: OutputId::from_raw(record.output),
                    slot: record.slot,
                    indicator: record.indicator,
                    action: (record.action != 0).then(|| WmActionId::from_raw(record.action)),
                    state_bits: record.state_bits,
                    label: decode_indicator_text(
                        record.label_len,
                        &record.label,
                        "indicator_label",
                    )?,
                })
            })
            .collect::<Result<Vec<_>, BinaryCodecError>>()?,
        output_statuses: statuses
            .into_iter()
            .map(|record| {
                Ok(PolicyProjectionOutputStatus {
                    output: OutputId::from_raw(record.output),
                    focus_bits: record.focus_bits,
                    layout: decode_indicator_text(
                        record.layout_len,
                        &record.layout,
                        "status_layout",
                    )?,
                })
            })
            .collect::<Result<Vec<_>, BinaryCodecError>>()?,
    })
}

fn encode_placement_record(placement: &PolicySurfacePlacement) -> WmV1ProjectionPlacementRecord {
    let (requested_width, requested_height) = encode_optional_size(placement.requested_size);
    let crop = placement.crop.unwrap_or_default();
    WmV1ProjectionPlacementRecord {
        surface_index: placement.surface.index(),
        surface_generation: placement.surface.generation(),
        state_generation: placement.surface_generation,
        x: placement.geometry.x,
        y: placement.geometry.y,
        width: placement.geometry.width,
        height: placement.geometry.height,
        requested_width,
        requested_height,
        crop_x: crop.x,
        crop_y: crop.y,
        crop_width: crop.width,
        crop_height: crop.height,
        transform: placement.transform as u16,
        presentation_bits: encode_presentation(placement.presentation),
    }
}

fn decode_placement_record(
    record: WmV1ProjectionPlacementRecord,
) -> Result<PolicySurfacePlacement, BinaryCodecError> {
    let crop = if record.crop_width == 0 && record.crop_height == 0 {
        if record.crop_x != 0 || record.crop_y != 0 {
            return Err(invalid("crop", 0));
        }
        None
    } else if record.crop_width > 0 && record.crop_height > 0 {
        Some(Rect {
            x: record.crop_x,
            y: record.crop_y,
            width: record.crop_width,
            height: record.crop_height,
        })
    } else {
        return Err(invalid("crop", 0));
    };
    Ok(PolicySurfacePlacement {
        surface: SurfaceId::new(record.surface_index, record.surface_generation),
        surface_generation: record.state_generation,
        geometry: Rect {
            x: record.x,
            y: record.y,
            width: record.width,
            height: record.height,
        },
        requested_size: decode_optional_size(
            record.requested_width,
            record.requested_height,
            "requested_size",
        )?,
        crop,
        transform: match record.transform {
            1 => PolicyTransform::Identity,
            other => return Err(invalid("policy_transform", u32::from(other))),
        },
        presentation: decode_presentation(record.presentation_bits, "presentation")?,
    })
}

fn encode_indicator_text(
    text: &str,
    field: &'static str,
) -> Result<(u16, [u8; 32]), BinaryCodecError> {
    if text.is_empty() || text.len() > 32 || text.chars().any(char::is_control) {
        return Err(invalid(field, text.len() as u32));
    }
    let mut bytes = [0; 32];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    Ok((text.len() as u16, bytes))
}

fn decode_indicator_text(
    length: u16,
    bytes: &[u8; 32],
    field: &'static str,
) -> Result<String, BinaryCodecError> {
    let length = usize::from(length);
    if length == 0 || length > bytes.len() || bytes[length..].iter().any(|byte| *byte != 0) {
        return Err(invalid(field, length as u32));
    }
    let text = core::str::from_utf8(&bytes[..length]).map_err(|_| invalid(field, length as u32))?;
    if text.chars().any(char::is_control) {
        return Err(invalid(field, length as u32));
    }
    Ok(text.to_owned())
}
