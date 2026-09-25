impl LiveProductionNativeScanout {
    /// Cancels a fully prepared transaction without submitting KMS and returns
    /// every affine renderer/native owner to the ordinary cleanup path.
    pub fn cancel_prepared_output_topology(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyPlan, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Prepared
            || !state.resources.ready()
            || self
                .exporters
                .iter()
                .any(|exporter| exporter.pending_frame())
        {
            self.output_topology_preparation = Some(state);
            return Err("native output topology resources are not ready to cancel".into());
        }
        let mut cleanup_pending = 0usize;
        for head_plan in &state.plan.heads {
            self.head_index_for_head(head_plan.head)
                .ok_or("topology cancellation lost a live head")?;
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
                            cleanup_pending = cleanup_pending.saturating_add(1);
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
                    cleanup_pending = cleanup_pending.saturating_add(1);
                }
            }
        }
        tracing::info!(
            "sophia_live_output_topology schema=1 status=prepared_cancelled heads={} cleanup_pending={} kms_submits=0",
            state.plan.heads.len(),
            cleanup_pending,
        );
        Ok(state.plan)
    }

    pub fn finish_failed_output_topology_preparation(
        &mut self,
    ) -> Result<(LiveProductionNativeTopologyPlan, String), Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Failed {
            self.output_topology_preparation = Some(state);
            return Err("native output topology preparation has not finished aborting".into());
        }
        Ok((
            state.plan,
            state
                .failure
                .unwrap_or_else(|| "native topology preparation failed".to_owned()),
        ))
    }

    pub fn begin_prepared_output_topology_apply(
        &mut self,
    ) -> Result<Vec<sophia_engine::RenderHeadId>, Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .as_mut()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Prepared
            || !state.resources.ready()
            || state.apply.begin_apply() != LiveProductionNativeTopologyApplyTransition::Accepted
        {
            return Err("native output topology resources are not ready to apply".into());
        }
        state.phase = LiveProductionNativeTopologyPreparationPhase::Applying;
        Ok(state.plan.heads.iter().map(|head| head.head).collect())
    }

    /// Submits at most one blocking card effect per owner turn.
    pub fn service_prepared_output_topology_apply(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyApplyTransition, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if !matches!(
            state.phase,
            LiveProductionNativeTopologyPreparationPhase::Applying
                | LiveProductionNativeTopologyPreparationPhase::RollingBack
        ) {
            self.output_topology_preparation = Some(state);
            return Err("native output topology apply is not active".into());
        }
        let card_index = state
            .apply
            .current_card_index()
            .ok_or("native topology apply coordinator has no current card")?;
        let rollback = state.phase == LiveProductionNativeTopologyPreparationPhase::RollingBack;
        let changes = topology_card_changes(&state, card_index, rollback)?;
        let group = self
            .groups
            .get(card_index)
            .ok_or("native topology apply references an unknown card")?;
        let outcome =
            crate::submit_native_topology_change_on_device(group.session.card(), &changes);
        let transition = if rollback {
            state.apply.observe_rollback(card_index, outcome)
        } else {
            state.apply.observe_apply(card_index, outcome)
        };
        match transition {
            LiveProductionNativeTopologyApplyTransition::RollbackRequired { .. } => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
            }
            LiveProductionNativeTopologyApplyTransition::Applied { .. } => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::Applied;
            }
            LiveProductionNativeTopologyApplyTransition::RolledBack { .. } => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::RolledBack;
            }
            LiveProductionNativeTopologyApplyTransition::FailedWithoutMutation { .. } => {
                state.failure = Some("the first card rejected the topology candidate".to_owned());
                self.cancel_partial_output_topology_resources(&mut state)?;
                state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
            }
            LiveProductionNativeTopologyApplyTransition::RollbackFailed { .. } => {
                state.failure =
                    Some("topology rollback failed after physical candidate mutation".to_owned());
                state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
            }
            LiveProductionNativeTopologyApplyTransition::Accepted
            | LiveProductionNativeTopologyApplyTransition::Retry
            | LiveProductionNativeTopologyApplyTransition::CardApplied { .. }
            | LiveProductionNativeTopologyApplyTransition::CardRolledBack { .. } => {}
            LiveProductionNativeTopologyApplyTransition::OutOfOrder
            | LiveProductionNativeTopologyApplyTransition::Terminal => {
                self.output_topology_preparation = Some(state);
                return Err("native topology apply coordinator rejected its own effect".into());
            }
        }
        tracing::info!(
            "sophia_live_output_topology schema=1 status=card_effect card={} rollback={} outcome={outcome:?} transition={transition:?}",
            card_index,
            rollback,
        );
        self.output_topology_preparation = Some(state);
        Ok(transition)
    }

    pub fn request_output_topology_rollback(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .as_mut()
            .ok_or("native output topology preparation is not active")?;
        if !matches!(
            state.phase,
            LiveProductionNativeTopologyPreparationPhase::Applied
                | LiveProductionNativeTopologyPreparationPhase::CandidateInstalled
                | LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued
        ) || state.apply.begin_rollback_after_apply()
            != LiveProductionNativeTopologyApplyTransition::Accepted
        {
            return Err("native output topology cannot begin post-apply rollback".into());
        }
        state.failure = Some(reason.into());
        state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
        Ok(())
    }
}
