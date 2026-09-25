impl LivePublicPolicyState {
    fn initial_scene(
        outputs: &[sophia_engine::HeadlessOutput],
        active_output: sophia_protocol::OutputId,
        session_operations: Vec<sophia_protocol::PolicySessionOperation>,
    ) -> sophia_protocol::PolicySceneSnapshot {
        let bounds = wm_output_bounds(outputs);
        sophia_protocol::PolicySceneSnapshot {
            generation: 1,
            active_output,
            outputs: bounds
                .into_iter()
                .map(|(output, bounds)| sophia_protocol::PolicyOutputSnapshot {
                    policy_key: None,
                    output,
                    generation: 1,
                    focus: None,
                    bounds,
                    work_area: bounds,
                })
                .collect(),
            surfaces: Vec::new(),
            session_operations,
        }
    }

    fn mint_transaction(&mut self) -> Result<TransactionId, Box<dyn std::error::Error>> {
        mint_public_policy_transaction(&mut self.next_transaction)
    }

    fn all_outputs(&self, active: sophia_protocol::OutputId) -> Vec<sophia_protocol::OutputId> {
        let mut outputs = self.outputs.iter().map(|output| output.id).collect::<Vec<_>>();
        outputs.sort_by_key(|output| output.raw());
        if let Some(index) = outputs.iter().position(|output| *output == active) {
            outputs.swap(0, index);
        }
        outputs
    }

    fn queue_cause(&mut self, cause: LivePublicPolicyCause) -> LiveWmRequestAdmission {
        enqueue_public_policy_cause(
            &mut self.queue,
            self.in_flight_source,
            self.in_flight_request.is_some(),
            cause,
        )
    }

    fn queue_security_cancel(
        &mut self,
        cause: LivePublicPolicyCause,
    ) -> LiveWmRequestAdmission {
        enqueue_public_policy_security_cancel(
            &mut self.queue,
            self.in_flight_request.is_some(),
            cause,
        )
    }

    fn admit_dirty(
        &mut self,
        request: sophia_protocol::PolicyDirtyRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if request.connection_epoch != self.connection_epoch || request.affected_outputs.is_empty() {
            return Err("public WM dirty request has an invalid connection or empty scope".into());
        }
        let affected = request
            .affected_outputs
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if affected.len() != request.affected_outputs.len()
            || !affected.is_subset(&self.live_output_ids)
        {
            return Err("public WM dirty request has duplicate or unknown outputs".into());
        }
        self.reducer
            .admit_policy_generation(request.policy_generation)?;
        self.pending_dirty_outputs.extend(affected);
        Ok(())
    }

    fn materialize_pending_dirty(&mut self) {
        materialize_public_dirty_cause(
            &mut self.queue,
            &mut self.pending_dirty_outputs,
            self.in_flight_source,
        );
    }

    /// Records a terminal projection settlement, re-arming the owner when the
    /// client can only recover from a cycle the owner has to offer.
    ///
    /// Invariant: after rejecting a response the owner never leaves the
    /// connection with no outstanding request and nothing queued. A physical
    /// run stranded exactly there — the client waited for a snapshot that was
    /// never coming and died on its socket deadline, and the resulting restarts
    /// exhausted the supervisor budget.
    fn settle_public_projection(&mut self, outcome: sophia_protocol::PolicyProjectionOutcome) {
        if outcome == sophia_protocol::PolicyProjectionOutcome::Committed
            && let Ok(mut origins) = self.launch_origins.lock()
        {
            origins.publish(self.connection_epoch, &self.staged_launch_contexts);
            origins.publish_outputs(self.connection_epoch, &self.staged_output_launch_contexts);
            for surface in &self.in_flight_origin_surfaces {
                if let Some((transaction, destination)) = origins.catalog_attribution(*surface) {
                    let actual = self.reducer.committed().iter().find(|output|
                        output.placements.iter().any(|p| p.surface == *surface)).map(|o| o.output.raw()).unwrap_or(0);
                    crate::session_println!("sophia_catalog_placement schema=1 status=committed transaction={} surface={} surface_generation={} output={} output_generation={} actual_output={} wm_epoch={} token={}",
                        transaction.raw(), surface.index(), surface.generation(), destination.output.raw(),
                        destination.output_generation, actual, destination.epoch, destination.token);
                }
            }
            origins.committed(self.in_flight_origin_surfaces.iter().copied());
        }
        self.staged_launch_contexts.clear();
        self.staged_output_launch_contexts.clear();
        self.in_flight_origin_surfaces.clear();

        if let Some(sophia_protocol::PolicyProjectionRequest {
            cause: sophia_protocol::PolicyRequestCause::Action { activation_serial, .. } | sophia_protocol::PolicyRequestCause::OutputAction { activation_serial, .. }, ..
        }) = self.in_flight_request.as_ref()
            && let Some(ticket) = self.control_tickets.remove(activation_serial)
        {
            ticket.finish(match outcome {
                sophia_protocol::PolicyProjectionOutcome::Committed => sophia_protocol::ControlOutcome::Committed,
                sophia_protocol::PolicyProjectionOutcome::RejectedInvalid | sophia_protocol::PolicyProjectionOutcome::RejectedStale => sophia_protocol::ControlOutcome::Rejected,
                _ => sophia_protocol::ControlOutcome::Indeterminate,
            });
        }
        if let Some((surface, _)) = consume_public_launch_classification(
            &mut self.launch_classifications,
            self.in_flight_source,
            outcome,
        )
        {
            crate::session_println!(
                "sophia_session_launch_placement schema=1 status=consumed surface={} metadata=none",
                surface.index(),
            );
        }
        self.cycle_submitted = false;
        self.in_flight_request = None;
        self.in_flight_source = None;
        if public_policy_rearm_after_outcome(outcome) {
            // The whole live set: a stale rejection means the canonical scene
            // moved in a way the owner cannot attribute to particular outputs,
            // and replaying the original cause would replay a user action or
            // name a surface that has just been withdrawn.
            self.pending_dirty_outputs
                .extend(self.live_output_ids.iter().copied());
        }
    }

    fn submit_or_defer(
        &mut self,
        command: PolicyTransportCommand,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.deferred_command.is_some() {
            return Err("public WM already has a deferred transport command".into());
        }
        if self.transport_unavailable {
            return Ok(());
        }
        let worker = self
            .worker
            .as_ref()
            .ok_or("public WM transport is unavailable")?;
        if let Err(command) = worker.try_command(command) {
            self.deferred_command = Some(command);
        }
        Ok(())
    }

    fn flush_deferred_command(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(command) = self.deferred_command.take() else {
            return Ok(());
        };
        if self.transport_unavailable {
            return Ok(());
        }
        let worker = self
            .worker
            .as_ref()
            .ok_or("public WM transport is unavailable")?;
        if let Err(command) = worker.try_command(command) {
            self.deferred_command = Some(command);
        }
        Ok(())
    }

    fn settle_rejected_projection(
        &mut self,
        projection: &sophia_protocol::PolicyProjectionProposal,
        outcome: sophia_protocol::PolicyProjectionOutcome,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.reducer.timeout(projection.request_id);
        self.submit_or_defer(PolicyTransportCommand::ProjectionOutcome {
            transaction: projection.transaction,
            request_id: projection.request_id,
            scene_generation: self.reducer.scene().generation,
            outcome,
            expect_session_operation: false,
        })?;
        self.settle_public_projection(outcome);
        self.expected_operation_slot = None;
        self.staged = None;
        Ok(())
    }

    fn snapshot(
        &self,
        layout: &PersistentLiveLayout,
        chrome: sophia_engine::SurfaceChromeStyle,
    ) -> Result<sophia_protocol::PolicySceneSnapshot, Box<dyn std::error::Error>> {
        let previous = self.reducer.scene();
        let committed = self.reducer.committed();
        let mut current_output = BTreeMap::new();
        let mut committed_geometry = BTreeMap::new();
        let mut committed_presentation = BTreeMap::new();
        for projection in &committed {
            for placement in &projection.placements {
                current_output.insert(placement.surface, projection.output);
                committed_geometry.insert(placement.surface, placement.geometry);
                committed_presentation.insert(placement.surface, placement.presentation);
            }
        }
        let surfaces = public_policy_surface_snapshots(
            layout,
            &current_output,
            &committed_geometry,
            &committed_presentation,
            chrome,
        )?;
        crate::session_println!(
            "sophia_live_wm_snapshot schema=1 status=complete surfaces={} minimized={} unassigned={}",
            surfaces.len(),
            surfaces
                .iter()
                .filter(|surface| surface.current_state.minimized)
                .count(),
            surfaces
                .iter()
                .filter(|surface| surface.current_output.is_none())
                .count(),
        );
        let outputs = self
            .outputs
            .iter()
            .map(|descriptor| descriptor.id)
            .map(|output| {
                let bounds = self
                    .output_bounds
                    .get(&output)
                    .copied()
                    .ok_or("public WM snapshot lost logical output bounds")?;
                Ok(sophia_protocol::PolicyOutputSnapshot {
                    policy_key: resolve_output_policy_key(output, &self.output_policy_keys, &self.output_capabilities)?,
                    output,
                    generation: self.output_generations.get(&output).copied().unwrap_or(1),
                    focus: public_policy_snapshot_focus(
                        output,
                        committed
                            .iter()
                            .find(|projection| projection.output == output)
                            .and_then(|projection| projection.focus),
                        &surfaces,
                    ),
                    bounds,
                    work_area: self.work_areas.get(&output).copied().unwrap_or(bounds),
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        let mut candidate = sophia_protocol::PolicySceneSnapshot {
            generation: previous.generation,
            active_output: self.active_output,
            outputs,
            surfaces,
            session_operations: self.session_operations.clone(),
        };
        let same_facts = candidate.active_output == previous.active_output
            && candidate.outputs == previous.outputs
            && candidate.surfaces == previous.surfaces
            && candidate.session_operations == previous.session_operations;
        if !same_facts {
            candidate.generation = previous
                .generation
                .checked_add(1)
                .ok_or("public WM scene generation exhausted")?;
        }
        Ok(candidate)
    }
}
