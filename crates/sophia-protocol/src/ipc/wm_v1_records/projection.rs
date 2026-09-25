pub fn encode_wm_v1_policy_projection(
    proposal: &PolicyProjectionProposal,
) -> Result<WmV1ProjectionTransfer, IpcCodecError> {
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
    let mut chunks = Vec::new();
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_OUTPUT_RECORD_KIND,
        outputs.len(),
        encode_wm_v1_projection_output_records(&outputs)?,
    )?;
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_PLACEMENT_RECORD_KIND,
        placements.len(),
        encode_wm_v1_projection_placement_records(&placements)?,
    )?;
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_INDICATOR_RECORD_KIND,
        indicators.len(),
        encode_wm_v1_projection_indicator_records(&indicators)?,
    )?;
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_OUTPUT_STATUS_RECORD_KIND,
        statuses.len(),
        encode_wm_v1_projection_output_status_records(&statuses)?,
    )?;
    let chunk_count = chunks.len() as u16;
    chunks.extend(super::encode_wm_tab_groups(
        &proposal.tab_groups,
        proposal.connection_epoch,
        chunk_count,
    )?);
    chunks.extend(super::encode_wm_translation_groups(
        &proposal.translation_groups,
        proposal.connection_epoch,
        chunks.len() as u16,
    )?);
    chunks.extend(super::encode_wm_launch_contexts(
        &proposal.launch_contexts,
        proposal.connection_epoch,
        chunks.len() as u16,
    )?);
    chunks.extend(super::encode_wm_output_launch_contexts(
        &proposal.output_launch_contexts,
        proposal.connection_epoch,
        chunks.len() as u16,
    )?);
    let begin = WmV1ProjectionBegin {
        connection_epoch: proposal.connection_epoch,
        request_id: proposal.request_id,
        base_generation: proposal.base_generation,
        active_output: proposal.active_output.raw(),
        chunk_count,
        output_count: outputs.len() as u16,
        placement_count: placements.len() as u32,
        indicator_count: indicators.len() as u16,
        status_count: statuses.len() as u16,
    };
    let end = WmV1ProjectionEnd {
        connection_epoch: proposal.connection_epoch,
        request_id: proposal.request_id,
        base_generation: proposal.base_generation,
        chunk_count,
    };
    Ok(WmV1ProjectionTransfer {
        transaction: proposal.transaction,
        begin,
        chunks,
        end,
    })
}

pub fn decode_wm_v1_policy_projection(
    transfer: &WmV1ProjectionTransfer,
) -> Result<PolicyProjectionProposal, IpcCodecError> {
    if !transfer.transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    if transfer.begin.connection_epoch != transfer.end.connection_epoch
        || transfer.begin.request_id != transfer.end.request_id
        || transfer.begin.base_generation != transfer.end.base_generation
        || transfer.begin.chunk_count != transfer.end.chunk_count
        || usize::from(transfer.begin.chunk_count) > transfer.chunks.len()
        || transfer.begin.active_output == 0
    {
        return Err(invalid("projection_transfer", 0));
    }
    let mut outputs = Vec::new();
    let mut placements = Vec::new();
    let mut indicators = Vec::new();
    let mut statuses = Vec::new();
    for (ordinal, chunk) in transfer.chunks.iter().enumerate() {
        if chunk.connection_epoch != transfer.begin.connection_epoch
            || usize::from(chunk.ordinal) != ordinal
        {
            return Err(invalid("projection_chunk_identity", chunk.ordinal as u32));
        }
        if ordinal >= usize::from(transfer.begin.chunk_count) && chunk.record_kind < 0xff00 {
            return Err(invalid(
                "projection_extension_order",
                u32::from(chunk.record_kind),
            ));
        }
        match chunk.record_kind {
            PROJECTION_OUTPUT_RECORD_KIND => outputs.extend(
                decode_wm_v1_projection_output_records(&chunk.data, chunk.item_count)?,
            ),
            PROJECTION_PLACEMENT_RECORD_KIND => placements.extend(
                decode_wm_v1_projection_placement_records(&chunk.data, chunk.item_count)?,
            ),
            PROJECTION_INDICATOR_RECORD_KIND => indicators.extend(
                decode_wm_v1_projection_indicator_records(&chunk.data, chunk.item_count)?,
            ),
            PROJECTION_OUTPUT_STATUS_RECORD_KIND => statuses.extend(
                decode_wm_v1_projection_output_status_records(&chunk.data, chunk.item_count)?,
            ),
            super::PROJECTION_TAB_GROUP_RECORD_KIND
            | super::PROJECTION_TAB_MEMBER_RECORD_KIND
            | super::PROJECTION_TRANSLATION_GROUP_RECORD_KIND
            | super::PROJECTION_TRANSLATION_MEMBER_RECORD_KIND
            | super::PROJECTION_LAUNCH_CONTEXT_RECORD_KIND
            | super::PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND
                if ordinal >= usize::from(transfer.begin.chunk_count) => {}
            other => return Err(invalid("projection_record_kind", u32::from(other))),
        }
    }
    require_count(outputs.len(), transfer.begin.output_count as usize)?;
    require_count(placements.len(), transfer.begin.placement_count as usize)?;
    require_count(indicators.len(), transfer.begin.indicator_count as usize)?;
    require_count(statuses.len(), transfer.begin.status_count as usize)?;
    let mut placement_cursor = placements.into_iter();
    let mut projected_outputs = Vec::with_capacity(outputs.len());
    for output in outputs {
        let focus = decode_optional_surface(output.focus_index, output.focus_generation, "focus")?;
        let mut projected = Vec::with_capacity(output.placement_count as usize);
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
        return Err(invalid("placement_count", transfer.begin.placement_count));
    }
    Ok(PolicyProjectionProposal {
        launch_contexts: super::decode_wm_launch_contexts(&transfer.chunks)?,
        output_launch_contexts: super::decode_wm_output_launch_contexts(&transfer.chunks)?,
        translation_groups: super::decode_wm_translation_groups(&transfer.chunks)?,
        tab_groups: super::decode_wm_tab_groups(&transfer.chunks)?,
        transaction: transfer.transaction,
        connection_epoch: transfer.begin.connection_epoch,
        request_id: transfer.begin.request_id,
        base_generation: transfer.begin.base_generation,
        active_output: OutputId::from_raw(transfer.begin.active_output),
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
