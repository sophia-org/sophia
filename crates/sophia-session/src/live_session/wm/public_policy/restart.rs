impl LiveWmSession {
    fn poll_public_restart(
        &mut self,
        layout: &mut PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) -> Result<Option<LiveWmProposal>, Box<dyn std::error::Error>> {
        if self.control_restart.is_some() {
            self.poll_control_restart(layout, output);
            return Ok(None);
        }
        if self.degraded {
            return Ok(None);
        }
        self.poll_public_proof_restart()?;
        let restart_requested = self.force_transport_restart;
        let process_exited = self.supervisor.poll()?.is_some();
        let settlement_pending = public_policy_restart_settlement_pending(
            layout
                .pending
                .as_ref()
                .is_some_and(|pending| pending.policy_settlement.is_some()),
            self.public
                .as_ref()
                .is_some_and(|public| public.output_effect_dispatched),
        );
        match public_policy_restart_decision(
            restart_requested,
            process_exited,
            settlement_pending,
        ) {
            PublicPolicyRestartDecision::Idle => return Ok(None),
            PublicPolicyRestartDecision::AbortSettlement => {
                if !process_exited {
                    self.supervisor.terminate()?;
                }
                let public = self.public.as_mut().expect("public WM state is present");
                public.request_output_candidate_cancellation(
                    "supervised WM restart requested during output apply".to_owned(),
                    None,
                )?;
                public.worker.take();
                public.transport_unavailable = true;
                public.deferred_command = None;
                self.force_transport_restart = true;
                layout.force_pending_timeout();
                crate::session_println!(
                    "sophia_live_wm schema=4 status=settlement_aborting adapter=sophia_wm_v1 reason=transport_lost preserved_layout=true"
                );
                return Ok(None);
            }
            PublicPolicyRestartDecision::Restart => {}
        }
        if restart_requested && !process_exited {
            self.supervisor.terminate()?;
        }
        if self.desktop_reload.as_ref().is_some_and(|pending| pending.replacement_spec.is_none()) {
            self.pending_policy_launch_spec = self.rollback_desktop_reload();
        }
        let replacement = self.pending_policy_launch_spec.take().or_else(|| {
            self.desktop_reload.as_mut().and_then(|pending| pending.replacement_spec.take())
        });
        if let Some(spec) = replacement {
            self.supervisor = ProcessSupervisor::new(SupervisedProcessKind::WindowManager, spec);
        }
        let mut public = self.public.take().expect("public WM state is present");
        public.worker.take();
        self.pending_policy_configuration = None;
        self.control_lifetime.take();
        for (_, ticket) in std::mem::take(&mut public.control_tickets) {
            ticket.finish(if ticket.dispatched() { sophia_protocol::ControlOutcome::Indeterminate } else { sophia_protocol::ControlOutcome::Stale });
        }
        let _ = public.reducer.disconnect(public.connection_epoch);
        // Retain the key ledger until replacement acceptance; never synthesize releases.

        self.force_transport_restart = false;
        self.restarts = self.restarts.saturating_add(1);
        if let Some(output_service) = public.output_service.as_ref() {
            let abandoned = output_service
                .pause_acceptance(Duration::from_secs(1))
                .map_err(|error| format!("output authority restart barrier failed: {error}"))?;
            if !abandoned.is_empty() {
                public.abandon_output_candidate()?;
            }
            crate::session_println!(
                "sophia_live_output_authority schema=2 status=acceptance_paused abandoned={} preserved_topology=true",
                abandoned.len(),
            );
        }
        let next_epoch = public.next_connection_epoch;
        public.next_connection_epoch = public
            .next_connection_epoch
            .checked_add(1)
            .ok_or("public WM connection epoch exhausted")?;
        let mut transport = bind_public_policy_transport(&public.directory, public.profile_key)?;
        let (state, command) = update_supervisor(
            self.supervisor_state.clone(),
            SupervisorEvent::ProcessExited,
            self.restart_policy,
        );
        self.supervisor_state = state;
        let started = match self.supervisor.apply(command) {
            Ok(Some(started)) => started,
            Ok(None) => return Err("public WM supervisor did not restart the policy process".into()),
            Err(error) => {
                if self.desktop_reload.is_some() {
                    self.public = Some(public);
                    self.pending_policy_launch_spec = self.rollback_desktop_reload();
                    self.force_transport_restart = true;
                    return Ok(None);
                }
                if self.committed == 0 {
                    return Err(error.into());
                }
                self.degraded = true;
                self.public = Some(public);
                crate::session_println!(
                    "sophia_live_wm schema=4 status=degraded adapter=sophia_wm_v1 reason=restart_failed preserved_layout=true error={error:?}"
                );
                return Ok(None);
            }
        };
        let pid = self
            .supervisor
            .peer_id()
            .ok_or("restarted public WM has no supervised PID")?;
        transport.authorize_supervised_pid(pid)?;
        crate::diagnostics::capture_process_identity("wm", pid, next_epoch);
        if let Some(output_service) = public.output_service.as_ref() {
            output_service
                .command(
                    sophia_runtime::OutputTransportServiceCommand::ReplaceSupervisedPid { pid },
                )
                .map_err(|_| "output authority service is unavailable during WM restart")?;
        }
        let (state, _) = update_supervisor(self.supervisor_state.clone(), started, self.restart_policy);
        self.supervisor_state = state;
        public.reducer.connect(next_epoch)?;
        public.worker = Some(start_public_policy_worker(
            transport,
            next_epoch,
            public.profile_key,
            public.native_presentation_capable,
        )?);
        public.connection_epoch = next_epoch;
        public.presentation_capture.revoke();
        public.presentation_input.revoke();
        public.presentation_receipts.clear();
        public.presentation_withdrawals.clear();
        public.presentation_withdrawal_pending = true;
        public.configured = false;
        public.negotiated = false;
        public.cycle_submitted = false;
        public.transport_ready = false;
        public.in_flight_request = None;
        public.in_flight_source = None;
        public.staged = None;
        public.prepared = None;
        public.pending_operation = None;
        public.expected_operation_slot = None;
        public.deferred_command = None;
        public.transport_unavailable = false;
        public.actions.clear();
        public.queue.clear();
        public.pending_dirty_outputs.clear();
        // The restarted policy has answered nothing. Its predecessor's settled
        // answers do not bind it, and the epoch key alone would not release them
        // until the first commit of the new connection.
        layout.rearm_manage_settlements();
        let affected_outputs = public.all_outputs(output.id);
        public.queue.push_back(LivePublicPolicyCause {
            source: LiveWmProposalSource::Relayout,
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs,
        });
        self.public = Some(public);
        crate::session_println!(
            "sophia_live_wm schema=4 status=restarted adapter=sophia_wm_v1 epoch={next_epoch} restarts={} preserved_layout=true",
            self.restarts
        );
        Ok(None)
    }
}
