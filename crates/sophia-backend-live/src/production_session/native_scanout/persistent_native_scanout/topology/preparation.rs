impl LiveProductionNativeScanout {
    /// Starts nonblocking renderer preparation for both sides of one topology
    /// transaction. No KMS request is submitted here.
    pub fn begin_output_topology_preparation(
        &mut self,
        plan: LiveProductionNativeTopologyPlan,
        rollback: crate::LiveResolvedOutputTopology,
        candidate_frames: Vec<crate::LiveProductionHeadCompositionFrame>,
        rollback_frames: Vec<crate::LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeTopologyPreparationReport, Box<dyn std::error::Error>> {
        self.invalidate_layout_probes();
        self.service_layout_probe_cleanup();
        if self.output_topology_preparation.is_some() {
            return Err("native output topology preparation is already active".into());
        }
        if !self.output_topology_preparation_quiescent() {
            return Err(
                "native output topology preparation requires quiescent frame ownership".into(),
            );
        }

        let mut candidate_frames =
            validate_live_production_topology_frames(&plan, candidate_frames, true)?;
        validate_live_production_rollback_topology(&plan, &rollback)?;
        let rollback_frames =
            validate_live_production_topology_frames(&plan, rollback_frames, false)?;
        self.prepare_output_topology_renderer_images(&candidate_frames)?;
        self.prepare_output_topology_renderer_images(&rollback_frames)?;
        let mut resources = LiveProductionNativeTopologyResources::new(&plan)
            .ok_or("native output topology resource cohort is invalid")?;

        // Disabled heads have no renderer work, but their property handles are
        // still part of prepare-all. Resolve them before mutating exporter queues.
        for head_plan in &plan.heads {
            if head_plan.disposition != LiveProductionNativeTopologyDisposition::Disabled {
                continue;
            }
            let prepared = crate::prepare_native_disabled_topology_head(
                self.groups[head_plan.card_index].session.card(),
                head_plan.previous_selection,
            );
            let owner = prepared.prepared.ok_or_else(|| {
                format!(
                    "native disabled topology head {} property preparation failed: {:?}",
                    head_plan.head.raw(),
                    prepared.status,
                )
            })?;
            resources
                .prepare_candidate_disabled(head_plan.head, owner)
                .map_err(|rejected| {
                    format!(
                        "native disabled topology head {} was rejected: {:?}",
                        head_plan.head.raw(),
                        rejected.transition,
                    )
                })?;
        }
        for head_plan in &plan.heads {
            if head_plan.previous_enabled {
                continue;
            }
            let prepared = crate::prepare_native_disabled_topology_head(
                self.groups[head_plan.card_index].session.card(),
                head_plan.previous_selection,
            );
            let owner = prepared.prepared.ok_or_else(|| {
                format!(
                    "native rollback-disabled head {} property preparation failed: {:?}",
                    head_plan.head.raw(),
                    prepared.status,
                )
            })?;
            resources
                .prepare_rollback_disabled(head_plan.head, owner)
                .map_err(|rejected| {
                    format!(
                        "native rollback-disabled head {} was rejected: {:?}",
                        head_plan.head.raw(),
                        rejected.transition,
                    )
                })?;
        }

        for head_plan in &plan.heads {
            let LiveProductionNativeTopologyDisposition::Enabled { .. } = head_plan.disposition
            else {
                continue;
            };
            let index = self
                .head_index_for_head(head_plan.head)
                .ok_or("topology preparation lost a live head")?;
            let mut frame = candidate_frames
                .remove(&head_plan.head)
                .expect("candidate frame coverage was validated");
            // A topology commit is not a frame: it proves a mode the head does
            // not yet run, and it must own the framebuffer it commits. A
            // client's buffer would be scanned out under a mode nobody has
            // validated it for, so a candidate frame carries no proof
            // regardless of the plan it came from.
            frame.frame.direct_scanout =
                sophia_engine::DirectScanoutVerdict::CompositionRequired("topology_commit");
            self.exporters[index].set_pending_mixed_frame(frame.frame);
        }

        let affected_heads = plan.heads.len();
        self.output_topology_preparation = Some(LiveProductionNativeTopologyPreparation {
            apply: LiveProductionNativeTopologyApplyCoordinator::new(&plan)
                .ok_or("native output topology apply coordinator is invalid")?,
            plan,
            rollback,
            resources,
            rollback_frames,
            phase: LiveProductionNativeTopologyPreparationPhase::PreparingCandidate,
            failure: None,
        });
        Ok(LiveProductionNativeTopologyPreparationReport {
            phase: LiveProductionNativeTopologyPreparationPhase::PreparingCandidate,
            candidate_prepared: self
                .output_topology_preparation
                .as_ref()
                .map_or(0, |state| state.resources.candidate_count()),
            rollback_prepared: 0,
            affected_heads,
        })
    }

    /// Advances renderer workers by one owner turn. The method returns
    /// `Prepared` only when the candidate and rollback resource sets are both
    /// complete; it never submits KMS.
    pub fn service_output_topology_preparation(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyPreparationReport, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        let result = self.service_output_topology_preparation_inner(&mut state);
        if let Err(error) = &result {
            state.failure = Some(error.to_string());
            state.phase = LiveProductionNativeTopologyPreparationPhase::Aborting;
        }
        let report = LiveProductionNativeTopologyPreparationReport {
            phase: state.phase,
            candidate_prepared: state.resources.candidate_count(),
            rollback_prepared: state.resources.rollback_count(),
            affected_heads: state.plan.heads.len(),
        };
        self.output_topology_preparation = Some(state);
        if let Err(error) = result {
            tracing::warn!(
                "sophia_live_output_topology schema=1 status=preparation_aborting error={error} kms_submits=0"
            );
        }
        Ok(report)
    }
}
