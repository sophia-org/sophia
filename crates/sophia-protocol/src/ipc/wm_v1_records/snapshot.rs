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
    if !scene.active_output.is_valid()
        || !scene
            .outputs
            .iter()
            .any(|output| output.output == scene.active_output)
    {
        return Err(invalid("snapshot_active_output", 0));
    }
    validate_wm_v1_snapshot_focus(scene)?;
    let outputs = scene
        .outputs
        .iter()
        .map(|output| {
            let (focus_index, focus_generation) = output
                .focus
                .map(|surface| (surface.index(), surface.generation()))
                .unwrap_or((0, 0));
            WmV1SnapshotOutputRecord {
                output: output.output.raw(),
                generation: output.generation,
                focus_index,
                focus_generation,
                x: output.bounds.x,
                y: output.bounds.y,
                width: output.bounds.width,
                height: output.bounds.height,
                work_x: output.work_area.x,
                work_y: output.work_area.y,
                work_width: output.work_area.width,
                work_height: output.work_area.height,
            }
        })
        .collect::<Vec<_>>();
    let surfaces = scene
        .surfaces
        .iter()
        .map(encode_surface_record)
        .collect::<Vec<_>>();
    let actions = if selected_capabilities & SOPHIA_WM_CAPABILITY_ACTIONS == 0 {
        Vec::new()
    } else {
        actions
            .iter()
            .map(|action| {
                let (name_len, name) = encode_action_name(&action.name)?;
                Ok(WmV1SnapshotActionRecord {
                    action: action.action.raw(),
                    session_operation_slot: action.session_operation_slot.unwrap_or(0),
                    name_len,
                    name,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?
    };
    let session_operations = if selected_capabilities & SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS == 0
    {
        Vec::new()
    } else {
        scene
            .session_operations
            .iter()
            .map(|operation| WmV1SnapshotSessionOperationRecord {
                operation: operation.token,
                slot: operation.slot,
                target_bits: u16::from(operation.permits_surface_target)
                    * POLICY_SESSION_OPERATION_SURFACE_TARGET,
            })
            .collect::<Vec<_>>()
    };
    let classifications = if selected_capabilities & SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT == 0 {
        Vec::new()
    } else {
        let live_surfaces = scene
            .surfaces
            .iter()
            .map(|surface| surface.surface)
            .collect::<std::collections::BTreeSet<_>>();
        let mut seen = std::collections::BTreeSet::new();
        classifications
            .iter()
            .map(|classification| {
                if !classification.surface.is_valid()
                    || classification.classification == 0
                    || !live_surfaces.contains(&classification.surface)
                    || !seen.insert(classification.surface)
                {
                    return Err(invalid(
                        "surface_classification",
                        classification.surface.index(),
                    ));
                }
                Ok(WmV1SnapshotSurfaceClassificationRecord {
                    surface_index: classification.surface.index(),
                    surface_generation: classification.surface.generation(),
                    classification: classification.classification,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?
    };
    let mut chunks = Vec::new();
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_OUTPUT_RECORD_KIND,
        outputs.len(),
        encode_wm_v1_snapshot_output_records(&outputs)?,
    )?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_SURFACE_RECORD_KIND,
        surfaces.len(),
        encode_wm_v1_snapshot_surface_records(&surfaces)?,
    )?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_ACTION_RECORD_KIND,
        actions.len(),
        encode_wm_v1_snapshot_action_records(&actions)?,
    )?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_SESSION_OPERATION_RECORD_KIND,
        session_operations.len(),
        encode_wm_v1_snapshot_session_operation_records(&session_operations)?,
    )?;
    // The frozen count describes only ordinary record chunks. Negotiated
    // extensions append after them with dense ordinals but do not spend any
    // SnapshotBegin or SnapshotEnd field.
    let chunk_count = u16::try_from(chunks.len()).map_err(|_| IpcCodecError::CountTooLarge {
        count: chunks.len(),
        max: u16::MAX as usize,
    })?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
        classifications.len(),
        encode_wm_v1_snapshot_surface_classification_records(&classifications)?,
    )?;
    let begin = WmV1SnapshotBegin {
        connection_epoch,
        scene_generation: scene.generation,
        active_output: scene.active_output.raw(),
        chunk_count,
        output_count: u16::try_from(outputs.len()).map_err(|_| IpcCodecError::CountTooLarge {
            count: outputs.len(),
            max: u16::MAX as usize,
        })?,
        surface_count: surfaces.len() as u32,
        action_count: actions.len() as u16,
        session_operation_count: session_operations.len() as u16,
    };
    let end = WmV1SnapshotEnd {
        connection_epoch,
        scene_generation: scene.generation,
        chunk_count,
    };
    let mut transfer = WmV1SnapshotTransfer {
        transaction,
        begin,
        chunks,
        end,
    };
    super::append_wm_output_policy_keys(&mut transfer, &scene.outputs, selected_capabilities)?;
    Ok(transfer)
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
    let mut outputs = Vec::new();
    let mut surfaces = Vec::new();
    let mut actions = Vec::new();
    let mut session_operations = Vec::new();
    let mut classifications = Vec::new();
    let ordinary_chunk_count = usize::from(transfer.begin.chunk_count);
    for (ordinal, chunk) in transfer.chunks.iter().enumerate() {
        if chunk.connection_epoch != transfer.begin.connection_epoch
            || usize::from(chunk.ordinal) != ordinal
        {
            return Err(invalid("snapshot_chunk_identity", chunk.ordinal as u32));
        }
        match (ordinal < ordinary_chunk_count, chunk.record_kind) {
            (true, SNAPSHOT_OUTPUT_RECORD_KIND) => outputs.extend(
                decode_wm_v1_snapshot_output_records(&chunk.data, chunk.item_count)?,
            ),
            (true, SNAPSHOT_SURFACE_RECORD_KIND) => surfaces.extend(
                decode_wm_v1_snapshot_surface_records(&chunk.data, chunk.item_count)?,
            ),
            (true, SNAPSHOT_ACTION_RECORD_KIND) => actions.extend(
                decode_wm_v1_snapshot_action_records(&chunk.data, chunk.item_count)?,
            ),
            (true, SNAPSHOT_SESSION_OPERATION_RECORD_KIND) => session_operations.extend(
                decode_wm_v1_snapshot_session_operation_records(&chunk.data, chunk.item_count)?,
            ),
            (false, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND) => {
                classifications.extend(decode_wm_v1_snapshot_surface_classification_records(
                    &chunk.data,
                    chunk.item_count,
                )?)
            }
            (
                false,
                super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND
                | super::SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND,
            ) => {}
            (_, other) => return Err(invalid("snapshot_record_kind", u32::from(other))),
        }
    }
    require_count(outputs.len(), transfer.begin.output_count as usize)?;
    require_count(surfaces.len(), transfer.begin.surface_count as usize)?;
    require_count(actions.len(), transfer.begin.action_count as usize)?;
    require_count(
        session_operations.len(),
        transfer.begin.session_operation_count as usize,
    )?;
    let mut scene = PolicySceneSnapshot {
        generation: transfer.begin.scene_generation,
        active_output: OutputId::from_raw(transfer.begin.active_output),
        outputs: outputs
            .into_iter()
            .map(|record| {
                Ok(PolicyOutputSnapshot {
                    policy_key: None,
                    output: OutputId::from_raw(record.output),
                    generation: record.generation,
                    focus: decode_optional_surface(
                        record.focus_index,
                        record.focus_generation,
                        "output_focus",
                    )?,
                    bounds: Rect {
                        x: record.x,
                        y: record.y,
                        width: record.width,
                        height: record.height,
                    },
                    work_area: Rect {
                        x: record.work_x,
                        y: record.work_y,
                        width: record.work_width,
                        height: record.work_height,
                    },
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
        surfaces: surfaces
            .into_iter()
            .map(decode_surface_record)
            .collect::<Result<Vec<_>, _>>()?,
        session_operations: session_operations
            .into_iter()
            .map(|record| {
                if record.operation == 0
                    || record.slot == 0
                    || record.target_bits & !POLICY_SESSION_OPERATION_SURFACE_TARGET != 0
                {
                    return Err(invalid("session_operation", record.target_bits.into()));
                }
                Ok(PolicySessionOperation {
                    token: record.operation,
                    slot: record.slot,
                    permits_surface_target: record.target_bits
                        & POLICY_SESSION_OPERATION_SURFACE_TARGET
                        != 0,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
    };
    super::apply_wm_output_policy_keys(transfer, &mut scene.outputs)?;
    validate_wm_v1_snapshot_focus(&scene)?;
    let live_surfaces = scene
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<std::collections::BTreeSet<_>>();
    let mut seen_classifications = std::collections::BTreeSet::new();
    let classifications = classifications
        .into_iter()
        .map(|record| {
            let surface = SurfaceId::new(record.surface_index, record.surface_generation);
            if !surface.is_valid()
                || record.classification == 0
                || !live_surfaces.contains(&surface)
                || !seen_classifications.insert(surface)
            {
                return Err(invalid("surface_classification", record.surface_index));
            }
            Ok(PolicySurfaceClassification {
                surface,
                classification: record.classification,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
    let mut launch_origins = Vec::new();
    for chunk in transfer
        .chunks
        .iter()
        .filter(|c| c.record_kind == super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND)
    {
        if chunk.item_count == 0 {
            return Err(invalid("launch_origin_capability", 0));
        }
        launch_origins.extend(super::decode_wm_launch_context_records(
            &chunk.data,
            chunk.item_count,
        )?);
    }
    super::encode_wm_launch_context_records(&launch_origins)?;
    for origin in &launch_origins {
        if origin.epoch != transfer.begin.connection_epoch
            || !live_surfaces.contains(&origin.surface)
        {
            return Err(invalid("launch_origin_surface", 0));
        }
    }
    Ok(WmV1DecodedSnapshot {
        launch_origins,
        scene,
        actions: actions
            .into_iter()
            .map(|record| {
                Ok(PolicyActionRegistration {
                    action: WmActionId::from_raw(record.action),
                    name: decode_action_name(record.name_len, &record.name)?,
                    session_operation_slot: (record.session_operation_slot != 0)
                        .then_some(record.session_operation_slot),
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
        classifications,
    })
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
