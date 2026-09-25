impl LiveProductionNativeScanout {
    fn retire_topology_displayed_owner(&mut self, index: usize) -> Result<(), &'static str> {
        let group = self.heads[index].group;
        self.heads[index].scanout_custody.retire_displayed(self.groups[group].session.card())
            .map(|_| ())
            .map_err(|()| "topology replacement retains displayed owner until cleanup capacity is available")
    }

    fn service_output_topology_preparation_inner(
        &mut self,
        state: &mut LiveProductionNativeTopologyPreparation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match state.phase {
            LiveProductionNativeTopologyPreparationPhase::PreparingCandidate => {
                for head_plan in &state.plan.heads {
                    let LiveProductionNativeTopologyDisposition::Enabled { selection, vrr, .. } =
                        head_plan.disposition
                    else {
                        continue;
                    };
                    if state.resources.candidate(head_plan.head).is_some() {
                        continue;
                    }
                    let Some(owner) = self.prepare_output_topology_head(
                        head_plan.head,
                        selection,
                        topology_vrr_enabled(vrr),
                    )?
                    else {
                        continue;
                    };
                    state
                        .resources
                        .prepare_candidate_enabled(head_plan.head, owner)
                        .map_err(|rejected| {
                            format!(
                                "candidate topology owner for head {} was rejected: {:?}",
                                head_plan.head.raw(),
                                rejected.transition,
                            )
                        })?;
                }
                if state.resources.candidate_count() == state.plan.heads.len() {
                    for head_plan in &state.plan.heads {
                        if !head_plan.previous_enabled {
                            continue;
                        }
                        let index = self
                            .head_index_for_head(head_plan.head)
                            .ok_or("rollback topology preparation lost a live head")?;
                        if self.exporters[index].pending_frame() {
                            return Err(
                                "candidate exporter remained occupied after preparation".into()
                            );
                        }
                        let mut frame = state
                            .rollback_frames
                            .remove(&head_plan.head)
                            .expect("rollback frame coverage was validated");
                        // A topology commit is not a frame: it proves a mode
                        // the head does not yet run, and it must own the
                        // framebuffer it commits. A client's buffer would be
                        // scanned out under a mode nobody has validated it
                        // for, so a candidate or rollback frame carries no
                        // proof regardless of the plan it came from.
                        frame.frame.direct_scanout =
                            sophia_engine::DirectScanoutVerdict::CompositionRequired(
                                "topology_commit",
                            );
                        self.exporters[index].set_pending_mixed_frame(frame.frame);
                    }
                    state.phase = LiveProductionNativeTopologyPreparationPhase::PreparingRollback;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::PreparingRollback => {
                for head_plan in &state.plan.heads {
                    if state.resources.rollback(head_plan.head).is_some() {
                        continue;
                    }
                    if !head_plan.previous_enabled {
                        return Err("disabled rollback head lost its prepared detach owner".into());
                    }
                    let Some(owner) = self.prepare_output_topology_head(
                        head_plan.head,
                        head_plan.previous_selection,
                        Some(false),
                    )?
                    else {
                        continue;
                    };
                    state
                        .resources
                        .prepare_rollback(head_plan.head, owner)
                        .map_err(|rejected| {
                            format!(
                                "rollback topology owner for head {} was rejected: {:?}",
                                head_plan.head.raw(),
                                rejected.transition,
                            )
                        })?;
                }
                if state.resources.ready() {
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Prepared;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::Prepared => {}
            LiveProductionNativeTopologyPreparationPhase::Applying
            | LiveProductionNativeTopologyPreparationPhase::RollingBack
            | LiveProductionNativeTopologyPreparationPhase::Applied
            | LiveProductionNativeTopologyPreparationPhase::CandidateInstalled
            | LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued
            | LiveProductionNativeTopologyPreparationPhase::RolledBack => {
                return Err("topology renderer preparation was serviced after apply began".into());
            }
            LiveProductionNativeTopologyPreparationPhase::Aborting => {
                let mut renderer_drained = true;
                for head_plan in &state.plan.heads {
                    let index = self
                        .head_index_for_head(head_plan.head)
                        .ok_or("topology abort lost a live head")?;
                    if !self.exporters[index].pending_frame() {
                        continue;
                    }
                    if !self.exporters[index].worker_in_flight() {
                        self.exporters[index].discard_pending_frame();
                        continue;
                    }
                    let selection = if state.resources.candidate_count() < state.plan.heads.len() {
                        match head_plan.disposition {
                            LiveProductionNativeTopologyDisposition::Enabled {
                                selection, ..
                            } => selection,
                            LiveProductionNativeTopologyDisposition::Disabled => {
                                head_plan.previous_selection
                            }
                        }
                    } else {
                        head_plan.previous_selection
                    };
                    let export =
                        crate::LiveRenderedScanoutBufferExporter::export_rendered_scanout_buffer(
                            &mut self.exporters[index],
                            crate::LiveGbmEglFrameTargetRecord::new(selection.size()),
                        );
                    if export.status == crate::LiveRendererScanoutBufferExportStatus::Pending {
                        renderer_drained = false;
                    }
                    // Dropping an exported worker owner returns its lease. No
                    // native framebuffer was built for this abort-only poll.
                    drop(export);
                }
                if renderer_drained
                    && self
                        .exporters
                        .iter()
                        .all(|exporter| !exporter.pending_frame())
                {
                    self.cancel_partial_output_topology_resources(state)?;
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::Failed => {
                return Err(state
                    .failure
                    .clone()
                    .unwrap_or_else(|| "native topology preparation failed".to_owned())
                    .into());
            }
        }
        Ok(())
    }

    fn cancel_partial_output_topology_resources(
        &mut self,
        state: &mut LiveProductionNativeTopologyPreparation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for head_plan in &state.plan.heads {
            self.head_index_for_head(head_plan.head)
                .ok_or("topology resource cancellation lost a live head")?;
            if let Some(candidate) = state.resources.take_candidate(head_plan.head) {
                match candidate {
                    LiveProductionNativeTopologyCandidateResource::Enabled(owner) => {
                        let cancelled = crate::cancel_prepared_rendered_topology_head(
                            self.groups[head_plan.card_index].session.card(),
                            owner,
                        );
                        if let Some(cleanup) = cancelled.cleanup {
                            self.output_topology_cleanup.push((
                                head_plan.head,
                                cleanup.map_scanout_buffer(|owner| {
                                    Box::new(owner) as Box<dyn std::any::Any>
                                }),
                            ));
                        }
                    }
                    LiveProductionNativeTopologyCandidateResource::Disabled(_) => {}
                }
            }
            if let Some(rollback) = state.resources.take_rollback(head_plan.head)
                && let LiveProductionNativeTopologyCandidateResource::Enabled(owner) = rollback
            {
                let cancelled = crate::cancel_prepared_rendered_topology_head(
                    self.groups[head_plan.card_index].session.card(),
                    owner,
                );
                if let Some(cleanup) = cancelled.cleanup {
                    self.output_topology_cleanup.push((
                        head_plan.head,
                        cleanup
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    ));
                }
            }
        }
        Ok(())
    }

    fn prepare_output_topology_head(
        &mut self,
        head: sophia_engine::RenderHeadId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        vrr_enabled: Option<bool>,
    ) -> Result<Option<LiveProductionPreparedTopologyHead>, Box<dyn std::error::Error>> {
        let index = self
            .head_index_for_head(head)
            .ok_or("topology renderer preparation targets an unknown head")?;
        let group = self.heads[index].group;
        let mut prepared =
            crate::prepare_rendered_primary_plane_topology_head_from_target_and_selection_with(
                crate::LiveKmsScanoutTargetStatus::Ready,
                Some(crate::LiveGbmEglFrameTargetRecord::new(selection.size())),
                crate::LibdrmNativePrimaryPlaneSelectionResult {
                    status: crate::LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                    selection: Some(selection),
                },
                vrr_enabled,
                self.groups[group].session.card(),
                &mut self.exporters[index],
            );
        match prepared.status {
            crate::LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutExportPending => Ok(None),
            crate::LiveRenderedPrimaryPlaneScanoutPrepareStatus::Prepared => {
                let owner = prepared
                    .prepared
                    .take()
                    .ok_or("prepared topology renderer omitted its affine owner")?;
                match crate::prepare_rendered_topology_head_from_prepared_scanout(
                    owner,
                    vrr_enabled,
                ) {
                    Ok(owner) => Ok(Some(owner)),
                    Err(owner) => {
                        let cancelled = crate::cancel_prepared_rendered_primary_plane_scanout(
                            self.groups[group].session.card(),
                            owner,
                        );
                        if let Some(cleanup) = cancelled.cleanup {
                            self.output_topology_cleanup.push((
                                head,
                                cleanup.map_scanout_buffer(|owner| {
                                    Box::new(owner) as Box<dyn std::any::Any>
                                }),
                            ));
                        }
                        Err("modeset preparation produced a non-topology resource owner".into())
                    }
                }
            }
            status => {
                if let Some(cleanup) = prepared.cleanup.take() {
                    self.output_topology_cleanup.push((
                        head,
                        cleanup
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    ));
                }
                Err(format!(
                    "topology renderer preparation failed for head {}: {status:?}",
                    head.raw(),
                )
                .into())
            }
        }
    }

    fn prepare_output_topology_renderer_images(
        &mut self,
        frames: &BTreeMap<sophia_engine::RenderHeadId, crate::LiveProductionHeadCompositionFrame>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let requirements = crate::live_topology_frame_renderer_image_requirements(frames);
        for (head, image_ids) in requirements {
            let target_index = self
                .head_index_for_head(head)
                .ok_or("topology renderer-image preparation targets an unknown head")?;
            for image_id in image_ids {
                if self.exporters[target_index]
                    .export_promoted_renderer_image(image_id)?
                    .is_some()
                {
                    continue;
                }
                if self.exporters[target_index].promote_renderer_image(image_id)?
                    && self.exporters[target_index]
                        .export_promoted_renderer_image(image_id)?
                        .is_some()
                {
                    continue;
                }

                let mut snapshot = None;
                for donor_index in 0..self.exporters.len() {
                    if donor_index == target_index {
                        continue;
                    }
                    if let Some(available) =
                        self.exporters[donor_index].export_promoted_renderer_image(image_id)?
                    {
                        snapshot = Some(available);
                        break;
                    }
                }
                let snapshot = snapshot.ok_or_else(|| {
                    format!(
                        "topology renderer image {} for head {} has no live donor",
                        image_id.raw(),
                        head.raw(),
                    )
                })?;
                if !self.exporters[target_index].restore_promoted_renderer_image(snapshot)?
                    && self.exporters[target_index]
                        .export_promoted_renderer_image(image_id)?
                        .is_none()
                {
                    return Err(format!(
                        "topology renderer image {} was not installed for head {}",
                        image_id.raw(),
                        head.raw(),
                    )
                    .into());
                }
                tracing::info!(
                    "sophia_live_output_topology schema=1 status=renderer_image_replicated head={} image={} kms_submits=0",
                    head.raw(),
                    image_id.raw(),
                );
            }
        }
        Ok(())
    }
}
