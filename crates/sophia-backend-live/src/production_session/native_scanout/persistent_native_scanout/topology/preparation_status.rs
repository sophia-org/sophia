impl LiveProductionNativeScanout {
    pub fn output_topology_preparation_active(&self) -> bool {
        self.output_topology_preparation.is_some()
    }

    /// Reports whether every ordinary frame/resource owner has retired. The
    /// session owner uses this before transferring scheduling authority to an
    /// output-topology transaction.
    /// Whether the scanout may *begin* a topology preparation.
    ///
    /// Deliberately not the same question as the runtime's
    /// `topology_rebind_quiescent`, despite the similar name, and the two must
    /// not be collapsed into one predicate. This one is false for the whole of
    /// an installed candidate, because `install_applied_output_topology`
    /// restores the preparation with phase `CandidateInstalled` -- and the
    /// runtime's rebind runs at exactly that moment. An AND of the two enforced
    /// at the rebind would reject every candidate installation. The owner's
    /// pre-apply wait ANDs them only because it precedes both.
    ///
    /// Queued work that has not crossed into a renderer worker is deliberately
    /// not counted. Installation discards exactly that state itself -- it
    /// refuses only `worker_in_flight` and then calls `discard_pending_frame`
    /// -- so requiring it to be absent first was stricter than the transition
    /// needs. It was also unreachable: nothing drains a queued frame while the
    /// owner is quarantined waiting on this predicate, so the wait could only
    /// end by expiring. What must still retire is everything holding a renderer
    /// lease or an in-flight KMS resource. A cohort retaining only the currently
    /// displayed owner is intentionally allowed: topology apply replaces that
    /// owner, just as it did before cohorts tracked last-head release.
    pub fn output_topology_preparation_quiescent(&self) -> bool {
        !self.layout_probe_cleanup_pending()
            && self.output_topology_preparation.is_none()
            && self.heads.iter().all(|head| {
                head.rendering_content.is_none()
                    && head.submitted_content.is_none()
                    && head.scanout_custody.submitted().is_none()
                    && head.prepared_scanout.is_none()
                    && !head.scanout_custody.cleanup_pending()
            })
            && self
                .exporters
                .iter()
                .all(|exporter| !exporter.worker_in_flight())
    }

    /// Names the first unmet clause of `output_topology_preparation_quiescent`,
    /// or `None` when quiescent.
    ///
    /// A wait that reports only that it timed out sends its reader back to the
    /// source to guess which owner was still holding a frame.
    pub fn output_topology_preparation_quiescence_blocker(&self) -> Option<&'static str> {
        if self.layout_probe_cleanup_pending() {
            return Some("layout_probe_cleanup");
        }
        if self.output_topology_preparation.is_some() {
            return Some("topology_preparation");
        }
        for head in &self.heads {
            if head.rendering_content.is_some() {
                return Some("head_rendering_content");
            }
            if head.submitted_content.is_some() {
                return Some("head_submitted_content");
            }
            if head.scanout_custody.submitted().is_some() {
                return Some("head_scanout_submission");
            }
            if head.prepared_scanout.is_some() {
                return Some("head_prepared_scanout");
            }
            if head.scanout_custody.cleanup_pending() {
                return Some("head_scanout_cleanup");
            }
        }
        if self
            .exporters
            .iter()
            .any(|exporter| exporter.worker_in_flight())
        {
            return Some("exporter_worker_in_flight");
        }
        None
    }

    /// Per-head pipeline state for the first head blocking quiescence.
    ///
    /// The clause name alone said content was stuck without saying where, and
    /// content only leaves `pending` once a submit reaches the kernel. Naming
    /// the head and every stage beside it is what distinguishes "nothing is
    /// submitting" from "a flip never came back".
    pub fn output_topology_quiescence_head_report(&self) -> Option<String> {
        let index = self.heads.iter().position(|head| {
            head.rendering_content.is_some()
                || head.submitted_content.is_some()
                || head.scanout_custody.submitted().is_some()
                || head.prepared_scanout.is_some()
                || head.scanout_custody.cleanup_pending()
        })?;
        let head = &self.heads[index];
        // Exporters are index-parallel with heads: both are built from one
        // zipped, jointly sorted list at discovery.
        let exporter = self.exporters.get(index);
        Some(format!(
            "head={} output={} enabled={} pending={} rendering={} submitted={} scanout={} prepared={} cleanup={} exporter_pending={} worker_in_flight={}",
            head.head.raw(),
            head.output.id.raw(),
            head.enabled,
            u8::from(head.pending_content.is_some()),
            u8::from(head.rendering_content.is_some()),
            u8::from(head.submitted_content.is_some()),
            u8::from(head.scanout_custody.submitted().is_some()),
            u8::from(head.prepared_scanout.is_some()),
            u8::from(head.scanout_custody.cleanup_pending()),
            exporter.map_or(2, |exporter| u8::from(exporter.pending_frame())),
            exporter.map_or(2, |exporter| u8::from(exporter.worker_in_flight())),
        ))
    }

    pub fn output_topology_preparation_phase(
        &self,
    ) -> Option<LiveProductionNativeTopologyPreparationPhase> {
        self.output_topology_preparation
            .as_ref()
            .map(|state| state.phase)
    }

    pub fn output_topology_failed_without_mutation(&self) -> bool {
        self.output_topology_preparation
            .as_ref()
            .is_some_and(|state| {
                state.phase == LiveProductionNativeTopologyPreparationPhase::Failed
                    && state.apply.applied == 0
            })
    }

    pub fn output_topology_allows_frame_service(&self) -> bool {
        self.output_topology_preparation
            .as_ref()
            .is_none_or(|state| {
                state.phase == LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued
            })
    }

    pub fn output_topology_cleanup_pending(&self) -> bool {
        !self.output_topology_cleanup.is_empty()
    }

    pub fn request_abort_output_topology_preparation(&mut self, reason: impl Into<String>) -> bool {
        let Some(mut state) = self.output_topology_preparation.take() else {
            return false;
        };
        if state.phase == LiveProductionNativeTopologyPreparationPhase::Failed {
            self.output_topology_preparation = Some(state);
            return true;
        }
        state.failure.get_or_insert_with(|| reason.into());
        match state.phase {
            LiveProductionNativeTopologyPreparationPhase::PreparingCandidate
            | LiveProductionNativeTopologyPreparationPhase::PreparingRollback
            | LiveProductionNativeTopologyPreparationPhase::Prepared => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::Aborting;
            }
            LiveProductionNativeTopologyPreparationPhase::Applying => {
                if state.apply.applied == 0 {
                    if let Err(error) = self.cancel_partial_output_topology_resources(&mut state) {
                        state.failure = Some(format!(
                            "topology abort resource cancellation failed: {error}"
                        ));
                    }
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                } else if state.apply.begin_rollback_after_partial_apply()
                    == LiveProductionNativeTopologyApplyTransition::Accepted
                {
                    state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
                } else {
                    state.failure =
                        Some("topology abort could not enter partial-apply rollback".to_owned());
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::Applied
            | LiveProductionNativeTopologyPreparationPhase::CandidateInstalled
            | LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued => {
                if state.apply.begin_rollback_after_apply()
                    == LiveProductionNativeTopologyApplyTransition::Accepted
                {
                    state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
                } else {
                    state.failure = Some("topology abort could not enter full rollback".to_owned());
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::RollingBack
            | LiveProductionNativeTopologyPreparationPhase::RolledBack => {}
            LiveProductionNativeTopologyPreparationPhase::Aborting
            | LiveProductionNativeTopologyPreparationPhase::Failed => {}
        }
        self.output_topology_preparation = Some(state);
        true
    }

    pub fn retry_output_topology_cleanup(&mut self) -> usize {
        self.service_layout_probe_cleanup();
        let pending = core::mem::take(&mut self.output_topology_cleanup);
        for (head, cleanup) in pending {
            let Some(index) = self.head_index_for_head(head) else {
                self.output_topology_cleanup.push((head, cleanup));
                continue;
            };
            let retried =
                crate::retry_rendered_primary_plane_scanout_cleanup(self.card(index), cleanup);
            if let Some(cleanup) = retried.cleanup {
                self.output_topology_cleanup.push((head, cleanup));
            }
        }
        self.output_topology_cleanup.len()
    }
}
