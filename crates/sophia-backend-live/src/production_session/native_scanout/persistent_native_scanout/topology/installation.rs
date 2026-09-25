impl LiveProductionNativeScanout {
    /// Adopts the candidate buffers accepted by every card while retaining the
    /// complete rollback side. Authority must not publish yet.
    pub fn install_applied_output_topology(
        &mut self,
    ) -> Result<Vec<sophia_engine::HeadlessOutput>, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Applied {
            self.output_topology_preparation = Some(state);
            return Err("native output topology candidate is not physically applied".into());
        }
        let result = self.install_output_topology_side(&mut state, true);
        match result {
            Ok(outputs) => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::CandidateInstalled;
                self.output_topology_preparation = Some(state);
                Ok(outputs)
            }
            Err(error) => {
                self.output_topology_preparation = Some(state);
                Err(error)
            }
        }
    }

    /// Releases the rollback pool only after every replacement logical output
    /// has completed its first ordinary presentation cohort.
    pub fn commit_installed_output_topology(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyPlan, Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .as_ref()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued {
            return Err("native output topology is not awaiting first presentation".into());
        }
        if state
            .plan
            .heads
            .iter()
            .any(|head| self.head_index_for_head(head.head).is_none())
        {
            return Err("topology commit lost a live head before finalization".into());
        }
        let mut state = self
            .output_topology_preparation
            .take()
            .expect("topology preparation was validated above");
        if let Err(error) = self.cancel_partial_output_topology_resources(&mut state) {
            self.output_topology_preparation = Some(state);
            return Err(error);
        }
        tracing::info!(
            "sophia_live_output_topology schema=1 status=committed heads={} outputs={} cleanup_pending={}",
            state.plan.heads.len(),
            self.logical_outputs.len(),
            self.output_topology_cleanup.len(),
        );
        Ok(state.plan)
    }

    /// Opens ordinary frame service only after one complete native-size cohort
    /// has been queued for every replacement logical output.
    pub fn arm_installed_output_topology_first_presentation(
        &mut self,
        first_frames: &BTreeMap<OutputId, LiveProductionNativeFrameId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self
            .output_topology_preparation
            .as_ref()
            .map(|state| state.phase)
            != Some(LiveProductionNativeTopologyPreparationPhase::CandidateInstalled)
        {
            return Err("native output topology candidate is not ready to arm".into());
        }
        if first_frames.len() != self.logical_outputs.len() {
            return Err("native output topology first-frame output coverage is incomplete".into());
        }
        let mut expected = BTreeMap::new();
        for output in &self.logical_outputs {
            let frame = first_frames
                .get(&output.id)
                .ok_or("native output topology first frame is absent")?;
            expected.insert(
                output.id,
                self.head_indices(output.id)
                    .into_iter()
                    .map(|index| {
                        let head = &self.heads[index];
                        (
                            index,
                            self.native_frame_owner.frame(
                                output.id,
                                head.head,
                                head.target_generation,
                                frame.raw(),
                            ),
                        )
                    })
                    .collect(),
            );
        }
        self.deferred_mirror_generations
            .validate_first_frames(&expected)?;
        self.output_topology_preparation
            .as_mut()
            .expect("topology state checked above")
            .phase = LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued;
        Ok(())
    }

    /// Installs the published rollback buffers after every required reverse
    /// card effect succeeded, then releases any never-adopted candidate owner.
    pub fn install_rolled_back_output_topology(
        &mut self,
    ) -> Result<(LiveProductionNativeTopologyPlan, String), Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::RolledBack {
            self.output_topology_preparation = Some(state);
            return Err("native output topology rollback is not physically complete".into());
        }
        if let Err(error) = self.install_output_topology_side(&mut state, false) {
            self.output_topology_preparation = Some(state);
            return Err(error);
        }
        self.cancel_partial_output_topology_resources(&mut state)?;
        let reason = state
            .failure
            .take()
            .unwrap_or_else(|| "candidate apply rolled back".to_owned());
        tracing::info!(
            "sophia_live_output_topology schema=1 status=rolled_back heads={} outputs={} cleanup_pending={}",
            state.plan.heads.len(),
            self.logical_outputs.len(),
            self.output_topology_cleanup.len(),
        );
        Ok((state.plan, reason))
    }

    fn install_output_topology_side(
        &mut self,
        state: &mut LiveProductionNativeTopologyPreparation,
        candidate: bool,
    ) -> Result<Vec<sophia_engine::HeadlessOutput>, Box<dyn std::error::Error>> {
        self.invalidate_layout_probes();
        let logical_outputs = if candidate {
            state.plan.outputs.clone()
        } else {
            state.rollback.outputs.clone()
        };
        if logical_outputs.is_empty() {
            return Err("installed native topology has no logical output".into());
        }
        let logical_by_id = logical_outputs
            .iter()
            .map(|output| (output.id, *output))
            .collect::<BTreeMap<_, _>>();
        if logical_by_id.len() != logical_outputs.len() {
            return Err("installed native topology repeats a logical output".into());
        }

        let mut installed = Vec::with_capacity(state.plan.heads.len());
        let mut registry = sophia_engine::EngineHeadRegistry::new();
        for head_plan in &state.plan.heads {
            let index = self
                .head_index_for_head(head_plan.head)
                .ok_or("topology installation lost a physical head")?;
            let (
                enabled,
                output,
                selection,
                target_generation,
                scale,
                refresh_millihz,
                transform,
                mapping,
                vrr,
            ) = if candidate {
                match head_plan.disposition {
                    LiveProductionNativeTopologyDisposition::Enabled {
                        output,
                        selection,
                        scale,
                        refresh_millihz,
                        transform,
                        mapping,
                        vrr,
                    } => (
                        true,
                        output,
                        selection,
                        head_plan.candidate_target_generation,
                        scale,
                        refresh_millihz,
                        transform,
                        mapping,
                        vrr,
                    ),
                    LiveProductionNativeTopologyDisposition::Disabled => (
                        false,
                        head_plan.previous_output,
                        head_plan.previous_selection,
                        head_plan.candidate_target_generation,
                        head_plan.previous_scale,
                        head_plan.previous_refresh_millihz,
                        head_plan.previous_transform,
                        head_plan.previous_mapping,
                        head_plan.previous_vrr,
                    ),
                }
            } else {
                (
                    head_plan.previous_enabled,
                    head_plan.previous_output,
                    head_plan.previous_selection,
                    head_plan.previous_target_generation,
                    head_plan.previous_scale,
                    head_plan.previous_refresh_millihz,
                    head_plan.previous_transform,
                    head_plan.previous_mapping,
                    head_plan.previous_vrr,
                )
            };
            let resource = if candidate {
                state.resources.candidate(head_plan.head)
            } else {
                state.resources.rollback(head_plan.head)
            }
            .ok_or("topology installation lost a prepared physical owner")?;
            if enabled
                != matches!(
                    resource,
                    LiveProductionNativeTopologyCandidateResource::Enabled(_)
                )
            {
                return Err("topology installation resource disposition mismatch".into());
            }
            let physical_output = sophia_engine::HeadlessOutput {
                id: output,
                size: selection.size(),
                scale,
            };
            let output_frames = OutputFramePresentationState::new(physical_output)?;
            if enabled
                && !registry
                    .admit(sophia_engine::HeadRenderTarget {
                        head: head_plan.head,
                        output,
                        target_generation,
                        native_size: selection.size(),
                        scale,
                        refresh_millihz,
                        transform,
                        mapping,
                    })
                    .is_admitted()
            {
                return Err("installed Engine head registry rejected a physical target".into());
            }
            installed.push(LiveProductionNativeInstalledHead {
                index,
                enabled,
                output,
                selection,
                target_generation,
                scale,
                refresh_millihz,
                transform,
                mapping,
                vrr,
                output_frames,
            });
        }
        if registry.output_count() != logical_outputs.len() {
            return Err("installed topology has a logical output without a physical head".into());
        }
        for (output, primary) in &state.plan.primary_heads {
            if registry.set_primary_head(*output, *primary)
                != sophia_engine::EngineLogicalOutputUpdate::Updated
            {
                return Err("installed topology names a primary outside its output".into());
            }
        }

        let mut lifecycles = BTreeMap::new();
        for output in logical_by_id.keys().copied() {
            let mut members = installed
                .iter()
                .filter(|head| head.enabled && head.output == output)
                .map(|head| self.heads[head.index].head)
                .collect::<Vec<_>>();
            if let Some(primary) = state.plan.primary_heads.get(&output)
                && let Some(index) = members.iter().position(|head| head == primary)
            {
                members.swap(0, index);
            }
            let mut lifecycle = LiveProductionMirrorGroupLifecycle::new(output, members.clone())
                .ok_or("installed logical output has no lifecycle members")?;
            for head in members {
                if !matches!(
                    lifecycle.mark_initialized(head),
                    LiveProductionMirrorHeadTransition::Accepted
                        | LiveProductionMirrorHeadTransition::GroupReady
                ) {
                    return Err("installed logical output lifecycle rejected initialization".into());
                }
            }
            lifecycles.insert(output, lifecycle);
        }

        for install in &installed {
            let custody = &self.heads[install.index].scanout_custody;
            if custody.submitted().is_some() || !custody.can_retire_displayed() {
                return Err(
                    "topology adoption waits for submitted ownership and cleanup capacity".into(),
                );
            }
            if self.exporters[install.index].worker_in_flight() {
                return Err("topology installation cannot replace active renderer work".into());
            }
        }
        for install in &installed {
            self.exporters[install.index].discard_pending_frame();
        }

        for install in installed {
            self.retire_topology_displayed_owner(install.index)?;
            let selected = if candidate {
                state
                    .resources
                    .take_candidate(self.heads[install.index].head)
            } else {
                state
                    .resources
                    .take_rollback(self.heads[install.index].head)
            }
            .expect("selected topology resources were prevalidated");
            if let LiveProductionNativeTopologyCandidateResource::Enabled(owner) = selected {
                self.heads[install.index]
                    .scanout_custody
                    .adopt_displayed(
                        crate::adopt_prepared_rendered_topology_head_after_commit(owner)
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    )
                    .expect("topology adoption prevalidated before resource take");
            }
            let head = &mut self.heads[install.index];
            head.enabled = install.enabled;
            head.selection = install.selection;
            head.target_generation = install.target_generation;
            head.scale = install.scale;
            head.refresh_millihz = install.refresh_millihz;
            head.transform = install.transform;
            head.mapping = install.mapping;
            head.vrr = install.vrr;
            head.output = sophia_engine::HeadlessOutput {
                id: install.output,
                size: install.selection.size(),
                scale: install.scale,
            };
            head.pending_callback = None;
            head.completion_mode = LiveProductionKmsCompletionMode::PageFlipPreferred;
            head.completion_fence_status = crate::LibdrmNativeCompletionFenceStatus::Unsupported;
            head.last_callback_serial = None;
            head.pending_content = None;
            head.rendering_content = None;
            head.submitted_content = None;
            head.presented_content = None;
            head.submitted_group_frame = None;
            head.prepared_group_frame = None;
            head.submitted_at = None;
            head.submitted_ust_usec = None;
            head.output_frames = install.output_frames;
        }
        self.logical_outputs = logical_outputs.clone();
        self.presentation_outputs = logical_outputs.len();
        self.output_lifecycles = lifecycles;
        self.output_cohorts.clear();
        self.deferred_mirror_generations.clear();
        self.production_page_flips = crate::LiveProductionPageFlipTracker::from_outputs(&registry);
        self.kernel_page_flip_ust.clear();
        Ok(logical_outputs)
    }
}
