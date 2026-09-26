pub fn encode_wm_v1_policy_projection(
    proposal: &PolicyProjectionProposal,
) -> Result<WmV1ProjectionTransfer, IpcCodecError> {
    let sections = encode_policy_projection_records(proposal)?;
    let count = |kind| {
        sections
            .iter()
            .find(|s| s.kind == kind)
            .map_or(0, |s| s.count)
    };
    let chunk_count = legacy_record_count_u16(sections.iter().filter(|s| s.kind < 0xff00).count())?;
    let begin = WmV1ProjectionBegin {
        connection_epoch: proposal.connection_epoch,
        request_id: proposal.request_id,
        base_generation: proposal.base_generation,
        active_output: proposal.active_output.raw(),
        chunk_count,
        output_count: legacy_record_count_u16(count(PROJECTION_OUTPUT_RECORD_KIND) as usize)?,
        placement_count: count(PROJECTION_PLACEMENT_RECORD_KIND),
        indicator_count: legacy_record_count_u16(count(PROJECTION_INDICATOR_RECORD_KIND) as usize)?,
        status_count: legacy_record_count_u16(count(PROJECTION_OUTPUT_STATUS_RECORD_KIND) as usize)?,
    };
    let end = WmV1ProjectionEnd {
        connection_epoch: proposal.connection_epoch,
        request_id: proposal.request_id,
        base_generation: proposal.base_generation,
        chunk_count,
    };
    let mut chunks = Vec::new();
    for section in sections {
        let kind = section.kind;
        // Frozen ordinary arrays and bookmark extensions are single chunks;
        // only group/presentation extensions historically split their rows.
        let size = match kind {
            super::PROJECTION_TAB_GROUP_RECORD_KIND => Some(super::PROJECTION_TAB_GROUP_RECORD_LEN),
            super::PROJECTION_TAB_MEMBER_RECORD_KIND => {
                Some(super::PROJECTION_TAB_MEMBER_RECORD_LEN)
            }
            super::PROJECTION_TRANSLATION_GROUP_RECORD_KIND => {
                Some(super::PROJECTION_TRANSLATION_GROUP_RECORD_LEN)
            }
            super::PROJECTION_TRANSLATION_MEMBER_RECORD_KIND => {
                Some(super::PROJECTION_TRANSLATION_MEMBER_RECORD_LEN)
            }
            other => super::wm_presentation_record_layout(other).map(|r| r.0),
        };
        if let Some(size) = size {
            chunks.extend(super::wm_record_sections::projection_chunks(
                vec![section],
                proposal.connection_epoch,
                legacy_record_count_u16(chunks.len())?,
                |_| Some(size),
            )?);
        } else {
            push_projection_chunk(
                &mut chunks,
                proposal.connection_epoch,
                kind,
                section.count as usize,
                section.bytes,
            )?;
        }
    }
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
            PROJECTION_OUTPUT_RECORD_KIND => {}
            PROJECTION_PLACEMENT_RECORD_KIND => {}
            PROJECTION_INDICATOR_RECORD_KIND => {}
            PROJECTION_OUTPUT_STATUS_RECORD_KIND => {}
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
            | super::PROJECTION_PRESENTATION_BINDING_RECORD_KIND
                if ordinal >= usize::from(transfer.begin.chunk_count) => {}
            other => return Err(invalid("projection_record_kind", u32::from(other))),
        }
    }
    let sections = transfer
        .chunks
        .iter()
        .map(|c| super::PolicyRecordSectionRef {
            kind: c.record_kind,
            count: c.item_count,
            bytes: &c.data,
        })
        .collect::<Vec<_>>();
    decode_projection_sections(
        super::PolicyProjectionMetadata {
            connection_epoch: transfer.begin.connection_epoch,
            active_output: OutputId::from_raw(transfer.begin.active_output),
            transaction: transfer.transaction,
            request_id: transfer.begin.request_id,
            base_generation: transfer.begin.base_generation,
        },
        &sections,
        Some([
            transfer.begin.output_count as usize,
            transfer.begin.placement_count as usize,
            transfer.begin.indicator_count as usize,
            transfer.begin.status_count as usize,
        ]),
    )
}
