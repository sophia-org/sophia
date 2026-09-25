impl LivePublicPolicyState {
    /// Hands a snapshot to the transport that answers future connections.
    ///
    /// The service keeps its own copy and sends it to whoever connects, so a
    /// snapshot never pushed here is invisible to anyone arriving later --
    /// including a policy the supervisor restarts, which then reasons about a
    /// desk that no longer exists.
    fn publish_snapshot_to_transport(
        &mut self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
        degraded_reason: &str,
    ) -> Result<(TransactionId, bool), Box<dyn std::error::Error>> {
        let transaction = TransactionId::from_raw(self.next_output_snapshot_transaction);
        self.next_output_snapshot_transaction = self
            .next_output_snapshot_transaction
            .checked_add(1)
            .ok_or("output snapshot transaction exhausted")?;
        let published = self.output_service.as_ref().is_some_and(|service| {
            service
                .command(sophia_runtime::OutputTransportServiceCommand::PublishSnapshot {
                    transaction,
                    snapshot,
                })
                .is_ok()
        });
        if !published {
            self.output_service.take();
            tracing::warn!(
                "sophia_live_output_authority schema=2 status=degraded reason={degraded_reason} preserved_topology=true"
            );
        }
        Ok((transaction, published))
    }

    fn publish_output_authority_snapshot(
        &mut self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
        capabilities: Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(authority) = self.output_authority.as_ref() else {
            return Ok(false);
        };
        if self.output_candidate_active() {
            return Err("hardware output publication raced an active policy candidate".into());
        }
        let mut replacement = authority.clone();
        replacement.replace_published_snapshot(snapshot.clone())?;
        let (transaction, transport_published) =
            self.publish_snapshot_to_transport(snapshot, "hardware_snapshot_transport")?;
        self.output_authority = Some(replacement);
        self.output_capabilities = capabilities;
        crate::session_println!(
            "sophia_live_output_authority schema=2 status=hardware_snapshot_published transaction={} topology_epoch={} heads={} groups={} first_presented=true transport_published={transport_published}",
            transaction.raw(),
            self.output_authority
                .as_ref()
                .expect("replacement authority installed above")
                .published()
                .topology_epoch,
            self.output_capabilities.len(),
            self.output_authority
                .as_ref()
                .expect("replacement authority installed above")
                .published()
                .groups
                .len(),
        );
        Ok(true)
    }

    fn take_output_topology_effect(
        &mut self,
    ) -> Option<crate::live_output_authority::LiveOutputAuthorityEffect> {
        if self.output_effect_dispatched {
            return None;
        }
        let effect = self.output_authority.as_ref()?.active_effect()?;
        self.output_effect_dispatched = true;
        Some(effect)
    }

    fn published_output_snapshot(&self) -> Option<sophia_protocol::OutputAuthoritySnapshot> {
        self.output_authority
            .as_ref()
            .map(|authority| authority.published().clone())
    }

    fn take_output_topology_reload_request(&mut self) -> bool {
        if self.output_candidate_active() {
            return false;
        }
        std::mem::take(&mut self.output_topology_reload_pending)
    }

    /// Admits a topology a reloaded profile asked for, as an ordinary
    /// candidate.
    ///
    /// This is the same admission the startup effect uses, and it carries no
    /// privilege of its own: the effect it leaves behind is drained, quiesced,
    /// prepared and rolled back by exactly the machinery that already runs at
    /// session start. A reload is not a second way to set a mode, only a second
    /// occasion to use the first one.
    ///
    /// Whether the topology actually differs is decided before this is called,
    /// by comparing the reloaded profile's output values against the running
    /// ones. A reload that changed a keybinding never reaches here, which is
    /// what keeps it from blinking a display.
    ///
    /// Returns whether an effect is now waiting to be drained.
    fn admit_reloaded_output_topology(
        &mut self,
        candidate: sophia_protocol::OutputTopologyCandidate,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let connection_epoch = self.connection_epoch;
        let capabilities = self.output_capabilities.clone();
        let Some(authority) = self.output_authority.as_mut() else {
            crate::session_eprintln!(
                "sophia_live_output_authority schema=3 status=reload_declined reason=no_authority"
            );
            return Ok(false);
        };
        if authority.active_transaction().is_some() {
            // Admission cannot interrupt an owned topology transaction.
            crate::session_eprintln!(
                "sophia_live_output_authority schema=3 status=reload_declined reason=candidate_active"
            );
            return Ok(false);
        }
        let transaction = TransactionId::from_raw(self.next_output_snapshot_transaction);
        self.next_output_snapshot_transaction =
            self.next_output_snapshot_transaction.saturating_add(1);
        let admission = match authority.admit(
            transaction,
            &sophia_protocol::OutputV1Proposal {
                connection_epoch,
                candidate,
            },
            &capabilities,
        ) {
            Ok(admission) => admission,
            Err(error) => {
                // A mode the hardware will not take is the operator's typo far
                // more often than it is our defect, so it costs them a log line
                // and not their desktop.
                crate::session_eprintln!(
                    "sophia_live_output_authority schema=3 status=reload_declined reason=not_admitted detail={error}"
                );
                return Ok(false);
            }
        };
        if !matches!(
            admission,
            crate::live_output_authority::LiveOutputAuthorityAdmission::Prepared
        ) {
            crate::session_eprintln!(
                "sophia_live_output_authority schema=3 status=reload_declined reason=not_prepared"
            );
            return Ok(false);
        }
        // The latch is what the startup effect left set. Clearing it is what
        // makes this candidate reachable by the same drain.
        self.output_effect_dispatched = false;
        crate::session_println!(
            "sophia_live_output_authority schema=3 status=reload_effect_pending transaction={}",
            transaction.raw(),
        );
        Ok(true)
    }


    fn output_topology_effect_pending(&self) -> bool {
        !self.output_effect_dispatched
            && self
                .output_authority
                .as_ref()
                .is_some_and(|authority| authority.active_effect().is_some())
    }

    fn ordinary_policy_settlement_idle(&self) -> bool {
        !self.cycle_submitted
            && self.in_flight_request.is_none()
            && self.staged.is_none()
            && self.prepared.is_none()
            && self.pending_operation.is_none()
            && self.deferred_command.is_none()
    }

    fn reject_output_topology_effect(
        &mut self,
        transaction: TransactionId,
        failure: sophia_engine::OutputTopologyTransactionFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output effect observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction) {
            return Err("output effect observation targets a stale transaction".into());
        }
        let transition = authority.fail(failure)?;
        if matches!(
            transition,
            sophia_engine::OutputTopologyTransactionTransition::OutOfOrder
                | sophia_engine::OutputTopologyTransactionTransition::UnknownHead
                | sophia_engine::OutputTopologyTransactionTransition::UnknownOutput
                | sophia_engine::OutputTopologyTransactionTransition::Terminal
        ) {
            return Err(format!(
                "output effect observation violated transaction order: {transition:?}"
            )
            .into());
        }
        if matches!(
            authority.active_phase(),
            Some(
                sophia_engine::OutputTopologyTransactionPhase::Committed
                    | sophia_engine::OutputTopologyTransactionPhase::RolledBack
                    | sophia_engine::OutputTopologyTransactionPhase::Failed
            )
        ) {
            let settlement = authority.settle_terminal()?;
            self.output_effect_dispatched = false;
            self.finish_output_settlement(settlement)?;
        }
        Ok(())
    }

    fn begin_output_topology_apply(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output apply observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.mark_prepared_batch(heads)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
            || authority.begin_apply()?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output apply preparation violated transaction order".into());
        }
        Ok(())
    }

    fn observe_output_topology_applied(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output apply observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction) {
            return Err("output apply observation targets a stale transaction".into());
        }
        let transition = authority.mark_applied_batch(heads)?;
        if !matches!(
            transition,
            sophia_engine::OutputTopologyTransactionTransition::Accepted
                | sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        ) {
            return Err("output apply observation violated transaction order".into());
        }
        Ok(())
    }

    fn observe_output_topology_first_presented(
        &mut self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<Option<sophia_protocol::OutputAuthoritySnapshot>, Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output presentation observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.mark_first_presented_batch(outputs)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output first-presentation observation violated transaction order".into());
        }
        let settlement = authority.settle_terminal()?;
        let published = settlement.published_snapshot.clone();
        self.output_effect_dispatched = false;
        self.finish_output_settlement(settlement)?;
        Ok(published)
    }

    fn preview_output_topology_first_presented(
        &self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<sophia_protocol::OutputAuthoritySnapshot, Box<dyn std::error::Error>> {
        let mut authority = self
            .output_authority
            .as_ref()
            .ok_or("output presentation preview has no authority owner")?
            .clone();
        if authority.active_transaction() != Some(transaction)
            || authority.mark_first_presented_batch(outputs)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output first-presentation preview violated transaction order".into());
        }
        authority
            .settle_terminal()?
            .published_snapshot
            .ok_or_else(|| "output authority preview did not commit a snapshot".into())
    }

    fn observe_output_topology_rolled_back(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output rollback observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.mark_rolled_back_batch(heads)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output rollback observation violated transaction order".into());
        }
        let settlement = authority.settle_terminal()?;
        self.output_effect_dispatched = false;
        self.finish_output_settlement(settlement)
    }

    fn observe_output_topology_rollback_failed(
        &mut self,
        transaction: TransactionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output rollback failure has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.rollback_failed()?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output rollback failure violated transaction order".into());
        }
        let settlement = authority.settle_terminal()?;
        self.output_effect_dispatched = false;
        self.finish_output_settlement(settlement)
    }

    fn abandon_output_candidate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(authority) = self.output_authority.as_mut() else {
            return Ok(());
        };
        let active = authority.active_transaction();
        if active.is_some() && active == self.startup_output_transaction {
            return Ok(());
        }
        if active.is_some() {
            authority.fail(sophia_engine::OutputTopologyTransactionFailure::Stale)?;
            let _ = authority.settle_terminal()?;
            self.output_effect_dispatched = false;
        }
        Ok(())
    }
}
