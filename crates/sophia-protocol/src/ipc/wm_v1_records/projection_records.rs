pub fn encode_policy_projection_records(
    proposal: &PolicyProjectionProposal,
) -> Result<Vec<super::PolicyRecordSection>, IpcCodecError> {
    if !proposal.transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
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
                    IpcCodecError::CountTooLarge {
                        count: output.placements.len(),
                        max: u32::MAX as usize,
                    }
                })?,
                focus_index,
                focus_generation,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
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
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
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
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
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
    sections.extend(super::encode_policy_tab_groups_records(
        &proposal.tab_groups,
    )?);
    sections.extend(super::encode_policy_translation_groups_records(
        &proposal.translation_groups,
    )?);
    sections.extend(super::encode_policy_launch_contexts_records(
        &proposal.launch_contexts,
        proposal.connection_epoch,
    )?);
    sections.extend(super::encode_policy_output_launch_contexts_records(
        &proposal.output_launch_contexts,
        proposal.connection_epoch,
    )?);
    sections.extend(super::encode_policy_presentation_records(
        proposal.presentation.as_ref(),
        proposal.connection_epoch,
    )?);
    Ok(sections)
}

pub fn decode_policy_projection_records(
    metadata: super::PolicyProjectionMetadata,
    sections: &[super::PolicyRecordSectionRef<'_>],
) -> Result<PolicyProjectionProposal, IpcCodecError> {
    if !metadata.transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    if !metadata.active_output.is_valid() {
        return Err(invalid("projection_transfer", 0));
    }
    super::validate_policy_record_sections(super::PolicyRecordContext::Projection, sections)?;
    decode_projection_sections(metadata, sections, None)
}

fn decode_projection_sections(
    metadata: super::PolicyProjectionMetadata,
    sections: &[super::PolicyRecordSectionRef<'_>],
    expected: Option<[usize; 4]>,
) -> Result<PolicyProjectionProposal, IpcCodecError> {
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
            super::PROJECTION_TAB_GROUP_RECORD_KIND
            | super::PROJECTION_TAB_MEMBER_RECORD_KIND
            | super::PROJECTION_TRANSLATION_GROUP_RECORD_KIND
            | super::PROJECTION_TRANSLATION_MEMBER_RECORD_KIND
            | super::PROJECTION_LAUNCH_CONTEXT_RECORD_KIND
            | super::PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND
            | super::PROJECTION_PRESENTATION_RECORD_KIND
            | super::PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND
            | super::PROJECTION_SURFACE_INSTANCE_RECORD_KIND
            | super::PROJECTION_PRESENTATION_REGION_RECORD_KIND
            | super::PROJECTION_PRESENTATION_BINDING_RECORD_KIND => {}
            other => return Err(invalid("projection_record_kind", u32::from(other))),
        }
    }
    if let Some(counts) = expected {
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
            expected.map_or(0, |c| c[1] as u32),
        ));
    }
    Ok(PolicyProjectionProposal {
        presentation: super::decode_policy_presentation_records(sections)?,
        launch_contexts: super::decode_policy_launch_contexts_records(
            metadata.connection_epoch,
            sections,
        )?,
        output_launch_contexts: super::decode_policy_output_launch_contexts_records(
            metadata.connection_epoch,
            sections,
        )?,
        translation_groups: super::decode_policy_translation_groups_records(sections)?,
        tab_groups: super::decode_policy_tab_groups_records(sections)?,
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
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
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
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
    })
}
