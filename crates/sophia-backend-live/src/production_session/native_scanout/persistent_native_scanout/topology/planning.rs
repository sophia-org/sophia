/// Binds a resolved authority candidate to the live session's existing native
/// objects without changing any output, exporter, runtime, or KMS state.
///
/// `resolve_mode` is the only hardware read. Keeping it injected makes the
/// projection deterministic in tests and makes the ownership boundary explicit:
/// the returned plan contains copied handles, never a second DRM owner.
pub fn plan_live_production_native_topology(
    current: &[LiveProductionNativeTopologyCurrentHead],
    resolved: &crate::LiveResolvedOutputTopology,
    mut resolve_mode: impl FnMut(
        LiveProductionNativeTopologyCurrentHead,
        crate::LibdrmNativeOutputTiming,
    ) -> Result<
        Option<::drm::control::Mode>,
        LiveProductionNativeTopologyPlanError,
    >,
) -> Result<LiveProductionNativeTopologyPlan, LiveProductionNativeTopologyPlanError> {
    if current.is_empty() || resolved.outputs.is_empty() {
        return Err(LiveProductionNativeTopologyPlanError::Empty);
    }
    let mut current_by_head = BTreeMap::new();
    for current in current.iter().copied() {
        if current_by_head.insert(current.head, current).is_some() {
            return Err(LiveProductionNativeTopologyPlanError::DuplicateCurrentHead(
                current.head,
            ));
        }
    }
    let output_ids = resolved
        .outputs
        .iter()
        .map(|output| output.id)
        .collect::<BTreeSet<_>>();
    if output_ids.len() != resolved.outputs.len()
        || resolved.logical_viewports.len() != resolved.outputs.len()
        || !output_ids.contains(&resolved.primary_output)
        || resolved.outputs.iter().any(|output| {
            !output.id.is_valid()
                || output.size.width <= 0
                || output.size.height <= 0
                || output.scale == 0
        })
    {
        return Err(LiveProductionNativeTopologyPlanError::InvalidOutput(
            resolved.primary_output,
        ));
    }
    for (output, viewport) in resolved.outputs.iter().zip(&resolved.logical_viewports) {
        if viewport.output != output.id
            || viewport.logical.width != output.size.width
            || viewport.logical.height != output.size.height
        {
            return Err(LiveProductionNativeTopologyPlanError::InvalidOutput(
                output.id,
            ));
        }
    }

    let enabled = resolved
        .targets
        .iter()
        .map(|target| (target.head, target))
        .collect::<BTreeMap<_, _>>();
    if enabled.len() != resolved.targets.len() {
        let duplicate = resolved
            .targets
            .iter()
            .map(|target| target.head)
            .find(|head| {
                resolved
                    .targets
                    .iter()
                    .filter(|target| target.head == *head)
                    .count()
                    > 1
            })
            .expect("a shorter map proves a duplicate");
        return Err(LiveProductionNativeTopologyPlanError::DuplicateCandidateHead(duplicate));
    }
    if resolved.primary_heads.len() != output_ids.len()
        || resolved
            .primary_heads
            .keys()
            .any(|output| !output_ids.contains(output))
    {
        return Err(LiveProductionNativeTopologyPlanError::InvalidPrimaryHead(
            resolved.primary_output,
        ));
    }
    for output in &output_ids {
        let Some(primary) = resolved.primary_heads.get(output) else {
            return Err(LiveProductionNativeTopologyPlanError::InvalidPrimaryHead(
                *output,
            ));
        };
        if enabled.get(primary).map(|target| target.output) != Some(*output) {
            return Err(LiveProductionNativeTopologyPlanError::InvalidPrimaryHead(
                *output,
            ));
        }
    }
    let disabled = resolved
        .disabled_heads
        .iter()
        .map(|disabled| (disabled.head, disabled))
        .collect::<BTreeMap<_, _>>();
    if disabled.len() != resolved.disabled_heads.len() {
        let duplicate = resolved
            .disabled_heads
            .iter()
            .map(|disabled| disabled.head)
            .find(|head| {
                resolved
                    .disabled_heads
                    .iter()
                    .filter(|disabled| disabled.head == *head)
                    .count()
                    > 1
            })
            .expect("a shorter map proves a duplicate");
        return Err(LiveProductionNativeTopologyPlanError::DuplicateCandidateHead(duplicate));
    }
    if let Some(head) = enabled.keys().find(|head| disabled.contains_key(head)) {
        return Err(LiveProductionNativeTopologyPlanError::DuplicateCandidateHead(*head));
    }
    for head in enabled.keys().chain(disabled.keys()) {
        if !current_by_head.contains_key(head) {
            return Err(LiveProductionNativeTopologyPlanError::MissingCurrentHead(
                *head,
            ));
        }
    }
    if let Some(head) = current_by_head
        .keys()
        .find(|head| !enabled.contains_key(head) && !disabled.contains_key(head))
    {
        return Err(LiveProductionNativeTopologyPlanError::MissingCandidateHead(
            *head,
        ));
    }

    let mut heads = Vec::with_capacity(current.len());
    for current in current.iter().copied() {
        let (candidate_target_generation, disposition) =
            if let Some(target) = enabled.get(&current.head) {
                if !output_ids.contains(&target.output)
                    || target.native_size.width <= 0
                    || target.native_size.height <= 0
                {
                    return Err(LiveProductionNativeTopologyPlanError::InvalidOutput(
                        target.output,
                    ));
                }
                let expected_generation = current.target_generation.checked_add(1).ok_or(
                    LiveProductionNativeTopologyPlanError::InvalidGeneration(current.head),
                )?;
                if target.target_generation != expected_generation {
                    return Err(LiveProductionNativeTopologyPlanError::InvalidGeneration(
                        current.head,
                    ));
                }
                let mode = resolve_mode(current, target.timing)?.ok_or(
                    LiveProductionNativeTopologyPlanError::ModeUnavailable(current.head),
                )?;
                let mut selection = crate::LibdrmNativePrimaryPlaneSelection::new(
                    current.selection.connector_handle(),
                    current.selection.crtc_handle(),
                    current.selection.plane_handle(),
                    target.native_size,
                    Some(mode),
                );
                // An output-policy transaction changes mode and logical
                // binding; it does not rediscover the fixed KMS route. Keep
                // the cursor plane selected beside that route. Dropping it
                // here would leave an already-admitted atomic cursor path
                // without a plane immediately after the topology commits.
                if let Some(cursor) = current.selection.cursor_plane() {
                    selection = selection.with_cursor_plane(cursor);
                }
                (
                    target.target_generation,
                    LiveProductionNativeTopologyDisposition::Enabled {
                        output: target.output,
                        selection,
                        scale: resolved
                            .outputs
                            .iter()
                            .find(|output| output.id == target.output)
                            .expect("candidate output identity was validated")
                            .scale,
                        refresh_millihz: target.timing.refresh_millihz,
                        transform: target.transform,
                        mapping: target.mapping,
                        vrr: target.vrr,
                    },
                )
            } else {
                let disabled = disabled[&current.head];
                let expected_generation = current.target_generation.checked_add(1).ok_or(
                    LiveProductionNativeTopologyPlanError::InvalidGeneration(current.head),
                )?;
                if disabled.target_generation != expected_generation {
                    return Err(LiveProductionNativeTopologyPlanError::InvalidGeneration(
                        current.head,
                    ));
                }
                (
                    disabled.target_generation,
                    LiveProductionNativeTopologyDisposition::Disabled,
                )
            };
        heads.push(LiveProductionNativeTopologyHeadPlan {
            head: current.head,
            card_index: current.card_index,
            previous_output: current.output,
            previous_enabled: current.enabled,
            previous_selection: current.selection,
            previous_target_generation: current.target_generation,
            previous_scale: current.scale,
            previous_refresh_millihz: current.refresh_millihz,
            previous_transform: current.transform,
            previous_mapping: current.mapping,
            previous_vrr: current.vrr,
            candidate_target_generation,
            disposition,
        });
    }
    Ok(LiveProductionNativeTopologyPlan {
        primary_output: resolved.primary_output,
        primary_heads: resolved.primary_heads.clone(),
        outputs: resolved.outputs.clone(),
        logical_viewports: resolved.logical_viewports.clone(),
        heads,
    })
}

/// Projects the published authority snapshot back into native render targets.
///
/// This is the rollback-side twin of candidate resolution. It deliberately
/// joins logical state from the published snapshot with physical size and
/// generation from the live head owner, preventing a provisional candidate
/// from contaminating the rollback image set.
pub fn project_live_production_published_topology(
    current: &[LiveProductionNativeTopologyCurrentHead],
    snapshot: &sophia_protocol::OutputAuthoritySnapshot,
    mut selected_timing: impl FnMut(
        LiveProductionNativeTopologyCurrentHead,
    ) -> Result<
        crate::LibdrmNativeOutputTiming,
        LiveProductionNativeTopologyPlanError,
    >,
) -> Result<crate::LiveResolvedOutputTopology, LiveProductionNativeTopologyPlanError> {
    snapshot
        .validate()
        .map_err(|_| LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)?;
    let mut output_by_head = BTreeMap::new();
    let mut mapping_by_head = BTreeMap::new();
    for group in &snapshot.groups {
        for member in &group.members {
            let head = sophia_engine::RenderHeadId::from_raw(member.head.raw());
            if output_by_head.insert(head, group.output).is_some()
                || mapping_by_head.insert(head, member.mapping).is_some()
            {
                return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
            }
        }
    }
    let descriptor_by_head = snapshot
        .heads
        .iter()
        .map(|head| (sophia_engine::RenderHeadId::from_raw(head.head.raw()), head))
        .collect::<BTreeMap<_, _>>();
    if descriptor_by_head.len() != snapshot.heads.len() || descriptor_by_head.len() != current.len()
    {
        return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
    }

    let mut targets = Vec::with_capacity(current.len());
    let mut disabled_heads = Vec::new();
    for native in current.iter().copied() {
        let descriptor = descriptor_by_head
            .get(&native.head)
            .ok_or(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)?;
        if !descriptor.connected
            || descriptor.enabled != native.enabled
            || descriptor.generation != native.target_generation
        {
            return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
        }
        if !native.enabled {
            if output_by_head.contains_key(&native.head) || descriptor.current_mode.is_some() {
                return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
            }
            disabled_heads.push(crate::LiveOutputAuthorityDisabledHead {
                head: native.head,
                target_generation: native.target_generation,
            });
            continue;
        }
        let output = output_by_head
            .get(&native.head)
            .copied()
            .ok_or(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)?;
        let timing = selected_timing(native)?;
        if output != native.output
            || mapping_by_head.get(&native.head).copied() != Some(native.mapping)
            || u32::try_from(native.selection.size().width).ok() != Some(timing.width)
            || u32::try_from(native.selection.size().height).ok() != Some(timing.height)
            || timing.refresh_millihz != native.refresh_millihz
        {
            return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
        }
        targets.push(crate::LiveOutputAuthorityHeadTarget {
            head: native.head,
            target_generation: native.target_generation,
            output,
            timing,
            native_size: native.selection.size(),
            transform: native.transform,
            mapping: native.mapping,
            vrr: native.vrr,
        });
    }
    let logical_viewports = snapshot
        .groups
        .iter()
        .map(|group| crate::LiveOutputAuthorityLogicalViewport {
            output: group.output,
            logical: group.logical,
        })
        .collect::<Vec<_>>();
    let outputs = snapshot
        .groups
        .iter()
        .map(|group| sophia_engine::HeadlessOutput {
            id: group.output,
            size: sophia_protocol::Size {
                width: group.logical.width,
                height: group.logical.height,
            },
            scale: 1,
        })
        .collect::<Vec<_>>();
    Ok(crate::LiveResolvedOutputTopology {
        primary_output: snapshot.primary_output,
        primary_heads: snapshot
            .groups
            .iter()
            .filter_map(|group| {
                group.members.first().map(|member| {
                    (
                        group.output,
                        sophia_engine::RenderHeadId::from_raw(member.head.raw()),
                    )
                })
            })
            .collect(),
        outputs,
        logical_viewports,
        disabled_heads,
        targets,
        // Connector grouping is a discovery/configuration input. Rendering
        // rollback targets needs only the already-resolved opaque members.
        mirror_grouping: crate::NativeMirrorGrouping::none(),
    })
}
