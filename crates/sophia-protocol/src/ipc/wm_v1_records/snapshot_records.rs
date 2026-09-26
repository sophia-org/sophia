pub fn encode_policy_snapshot_records(
    connection_epoch: u64,
    scene: &PolicySceneSnapshot,
    actions: &[PolicyActionRegistration],
    classifications: &[PolicySurfaceClassification],
    launch_origins: &[crate::PolicyLaunchContext],
    selected_capabilities: u64,
) -> Result<Vec<super::PolicyRecordSection>, IpcCodecError> {
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
    let mut sections = Vec::new();
    push_policy_section(
        &mut sections,
        SNAPSHOT_OUTPUT_RECORD_KIND,
        outputs.len(),
        encode_wm_v1_snapshot_output_records(&outputs)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_SURFACE_RECORD_KIND,
        surfaces.len(),
        encode_wm_v1_snapshot_surface_records(&surfaces)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_ACTION_RECORD_KIND,
        actions.len(),
        encode_wm_v1_snapshot_action_records(&actions)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_SESSION_OPERATION_RECORD_KIND,
        session_operations.len(),
        encode_wm_v1_snapshot_session_operation_records(&session_operations)?,
    )?;
    push_policy_section(
        &mut sections,
        SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
        classifications.len(),
        encode_wm_v1_snapshot_surface_classification_records(&classifications)?,
    )?;
    sections.extend(super::encode_policy_output_key_records(
        &scene.outputs,
        selected_capabilities,
    )?);
    if selected_capabilities & super::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN != 0 {
        let mut origins =
            super::encode_policy_launch_contexts_records(launch_origins, connection_epoch)?;
        for section in &mut origins {
            section.kind = super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND;
        }
        sections.extend(origins);
    }
    sections.sort_by_key(|s| s.kind);
    Ok(sections)
}

pub fn decode_policy_snapshot_records(
    metadata: super::PolicySnapshotMetadata,
    sections: &[super::PolicyRecordSectionRef<'_>],
) -> Result<super::PolicyDecodedSnapshot, IpcCodecError> {
    if !metadata.active_output.is_valid() {
        return Err(invalid("snapshot_transfer", 0));
    }
    super::validate_policy_record_sections(super::PolicyRecordContext::Snapshot, sections)?;
    decode_snapshot_sections(metadata, sections, None)
}

fn decode_snapshot_sections(
    metadata: super::PolicySnapshotMetadata,
    sections: &[super::PolicyRecordSectionRef<'_>],
    expected: Option<[usize; 4]>,
) -> Result<super::PolicyDecodedSnapshot, IpcCodecError> {
    let mut outputs = Vec::new();
    let mut surfaces = Vec::new();
    let mut actions = Vec::new();
    let mut session_operations = Vec::new();
    let mut classifications = Vec::new();
    for chunk in sections {
        match chunk.kind {
            SNAPSHOT_OUTPUT_RECORD_KIND => outputs.extend(decode_wm_v1_snapshot_output_records(
                chunk.bytes,
                chunk.count,
            )?),
            SNAPSHOT_SURFACE_RECORD_KIND => surfaces.extend(decode_wm_v1_snapshot_surface_records(
                chunk.bytes,
                chunk.count,
            )?),
            SNAPSHOT_ACTION_RECORD_KIND => actions.extend(decode_wm_v1_snapshot_action_records(
                chunk.bytes,
                chunk.count,
            )?),
            SNAPSHOT_SESSION_OPERATION_RECORD_KIND => session_operations.extend(
                decode_wm_v1_snapshot_session_operation_records(chunk.bytes, chunk.count)?,
            ),
            SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND => classifications.extend(
                decode_wm_v1_snapshot_surface_classification_records(chunk.bytes, chunk.count)?,
            ),
            super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND
            | super::SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND => {}
            other => return Err(invalid("snapshot_record_kind", u32::from(other))),
        }
    }
    if let Some(counts) = expected {
        require_count(outputs.len(), counts[0])?;
        require_count(surfaces.len(), counts[1])?;
        require_count(actions.len(), counts[2])?;
        require_count(session_operations.len(), counts[3])?;
    }
    let mut scene = PolicySceneSnapshot {
        generation: metadata.scene_generation,
        active_output: metadata.active_output,
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
    super::apply_policy_output_key_records(sections, &mut scene.outputs)?;
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
    for chunk in sections
        .iter()
        .filter(|c| c.kind == super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND)
    {
        if chunk.count == 0 {
            return Err(invalid("launch_origin_capability", 0));
        }
        launch_origins.extend(super::decode_wm_launch_context_records(
            chunk.bytes,
            chunk.count,
        )?);
    }
    super::encode_wm_launch_context_records(&launch_origins)?;
    for origin in &launch_origins {
        if origin.epoch != metadata.connection_epoch || !live_surfaces.contains(&origin.surface) {
            return Err(invalid("launch_origin_surface", 0));
        }
    }
    Ok(WmV1DecodedSnapshot {
        launch_origins,
        scene,
        actions: decode_policy_action_rows(actions)?,
        classifications,
    })
}
