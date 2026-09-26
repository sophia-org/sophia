/// Encodes one complete scene snapshot for a client that negotiated
/// `selected_capabilities`.
///
/// Capability-governed record kinds are omitted, along with their declared
/// counts, when the client did not negotiate them. Omission keeps a frozen client
/// from ever receiving a record kind it must reject, which is what makes
/// server-to-client additions reversible after a revision freezes. Callers must
/// pass the capability set the server actually selected during negotiation, not
/// the set it supports; see `docs/sophia-policy-ipc.md`.
///
/// Scene outputs and surfaces are not gated. They are the interface's core
/// semantics, and a client that negotiated nothing still requires a complete
/// scene to propose against.
pub fn encode_wm_v1_policy_snapshot(
    transaction: TransactionId,
    connection_epoch: u64,
    scene: &PolicySceneSnapshot,
    actions: &[PolicyActionRegistration],
    classifications: &[PolicySurfaceClassification],
    selected_capabilities: u64,
) -> Result<WmV1SnapshotTransfer, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    let sections = encode_policy_snapshot_records(
        connection_epoch,
        scene,
        actions,
        classifications,
        &[],
        selected_capabilities,
    )?;
    let count = |kind| {
        sections
            .iter()
            .find(|s| s.kind == kind)
            .map_or(0, |s| s.count)
    };
    let chunk_count = legacy_record_count_u16(sections.iter().filter(|s| s.kind < 0xff00).count())?;
    let begin = WmV1SnapshotBegin {
        connection_epoch,
        scene_generation: scene.generation,
        active_output: scene.active_output.raw(),
        chunk_count,
        output_count: legacy_record_count_u16(count(SNAPSHOT_OUTPUT_RECORD_KIND) as usize)?,
        surface_count: count(SNAPSHOT_SURFACE_RECORD_KIND),
        action_count: legacy_record_count_u16(count(SNAPSHOT_ACTION_RECORD_KIND) as usize)?,
        session_operation_count: legacy_record_count_u16(count(
            SNAPSHOT_SESSION_OPERATION_RECORD_KIND,
        ) as usize)?,
    };
    let end = WmV1SnapshotEnd {
        connection_epoch,
        scene_generation: scene.generation,
        chunk_count,
    };
    let chunks = sections
        .into_iter()
        .enumerate()
        .map(|(ordinal, s)| {
            Ok(WmV1SnapshotChunk {
                connection_epoch,
                ordinal: legacy_record_count_u16(ordinal)?,
                record_kind: s.kind,
                item_count: s.count,
                data: s.bytes,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
    Ok(WmV1SnapshotTransfer {
        transaction,
        begin,
        chunks,
        end,
    })
}

pub fn decode_wm_v1_policy_snapshot(
    transfer: &WmV1SnapshotTransfer,
) -> Result<WmV1DecodedSnapshot, IpcCodecError> {
    if !transfer.transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    if transfer.begin.connection_epoch != transfer.end.connection_epoch
        || transfer.begin.scene_generation != transfer.end.scene_generation
        || transfer.begin.chunk_count != transfer.end.chunk_count
        || usize::from(transfer.begin.chunk_count) > transfer.chunks.len()
        || transfer.begin.active_output == 0
    {
        return Err(invalid("snapshot_transfer", 0));
    }
    let ordinary_chunk_count = usize::from(transfer.begin.chunk_count);
    for (ordinal, chunk) in transfer.chunks.iter().enumerate() {
        if chunk.connection_epoch != transfer.begin.connection_epoch
            || usize::from(chunk.ordinal) != ordinal
        {
            return Err(invalid("snapshot_chunk_identity", chunk.ordinal as u32));
        }
        match (ordinal < ordinary_chunk_count, chunk.record_kind) {
            (true, SNAPSHOT_OUTPUT_RECORD_KIND) => {}
            (true, SNAPSHOT_SURFACE_RECORD_KIND) => {}
            (true, SNAPSHOT_ACTION_RECORD_KIND) => {}
            (true, SNAPSHOT_SESSION_OPERATION_RECORD_KIND) => {}
            (false, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND) => {}
            (
                false,
                super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND
                | super::SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND,
            ) => {}
            (_, other) => return Err(invalid("snapshot_record_kind", u32::from(other))),
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
    decode_snapshot_sections(
        super::PolicySnapshotMetadata {
            connection_epoch: transfer.begin.connection_epoch,
            active_output: OutputId::from_raw(transfer.begin.active_output),
            scene_generation: transfer.begin.scene_generation,
        },
        &sections,
        Some([
            transfer.begin.output_count as usize,
            transfer.begin.surface_count as usize,
            transfer.begin.action_count as usize,
            transfer.begin.session_operation_count as usize,
        ]),
    )
}

fn validate_wm_v1_snapshot_focus(scene: &PolicySceneSnapshot) -> Result<(), IpcCodecError> {
    for output in &scene.outputs {
        let Some(focus) = output.focus else {
            continue;
        };
        if !scene.surfaces.iter().any(|surface| {
            surface.surface == focus
                && surface.current_output == Some(output.output)
                && surface.capabilities.focusable
                && !surface.current_state.minimized
        }) {
            return Err(invalid("snapshot_output_focus", focus.index()));
        }
    }
    Ok(())
}
