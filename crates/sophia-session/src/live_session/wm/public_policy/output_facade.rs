impl LiveWmSession {
    /// A physical owner is being retired before an admitted policy effect
    /// started. Its old head identities cannot survive into the replacement.
    /// Started effects must instead finish their own cancellation/rollback.
    fn abandon_unstarted_output_topology_for_rebuild(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(public) = self.public.as_mut() else { return Ok(()); };
        if public.output_effect_dispatched {
            return Err("native rebuild overlaps a dispatched output effect".into());
        }
        if let Some(transaction) = public.output_authority.as_ref().and_then(|authority| authority.active_transaction()) {
            public.reject_output_topology_effect(transaction, sophia_engine::OutputTopologyTransactionFailure::Stale)?;
        }
        Ok(())
    }

    fn poll_output_authority(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(public) = self.public.as_mut() {
            public.poll_output_authority()?;
        }
        Ok(())
    }

    fn output_topology_effect_pending(&self) -> bool {
        self.public
            .as_ref()
            .is_some_and(LivePublicPolicyState::output_topology_effect_pending)
    }

    fn startup_output_topology_pending(&self) -> bool {
        self.public
            .as_ref()
            .is_some_and(|public| public.startup_output_transaction.is_some())
    }

    fn output_peer_transaction_epoch(&self, transaction: TransactionId) -> Option<u64> {
        let public = self.public.as_ref()?;
        if public.startup_output_transaction == Some(transaction)
            || public.reload_output_transaction == Some(transaction)
            || public.output_service.is_none()
        {
            return None;
        }
        let authority = public.output_authority.as_ref()?;
        (authority.active_transaction() == Some(transaction)).then(|| authority.connection_epoch())
    }

    fn output_peer_supervisor_running(&self) -> bool {
        self.public.as_ref().and_then(|public| public.output_service.as_ref())
            .is_some_and(|service| matches!(service, LiveOutputService::Files { supervisor, .. } if supervisor.child_id().is_some()))
    }

    fn request_output_peer_proof_termination(
        &mut self,
        transaction: TransactionId,
        epoch: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.output_peer_transaction_epoch(transaction) != Some(epoch) {
            return Err("output proof lost the assigned transaction".into());
        }
        let Some(LiveOutputService::Files { supervisor, .. }) = self
            .public
            .as_mut()
            .and_then(|public| public.output_service.as_mut())
        else {
            return Err("output proof has no file-role supervisor".into());
        };
        let peer = supervisor
            .peer_id()
            .ok_or("output proof peer already departed")?;
        supervisor.request_termination()?;
        self.public
            .as_mut()
            .expect("proof checked public owner")
            .output_peer_loss_observation = Some(OutputPeerLossObservation {
            connection_epoch: epoch,
            transaction,
            peer,
            disconnected: false,
            terminated: false,
            failed: false,
        });
        tracing::warn!(
            "sophia_output_peer_loss_proof schema=1 status=termination_requested epoch={epoch} transaction={} peer={peer} boundary=all_cards_applied deadline_ms={}",
            transaction.raw(),
            OUTPUT_PEER_LOSS_DEPARTURE_TIMEOUT.as_millis(),
        );
        Ok(())
    }

    fn output_peer_loss_observation(&self) -> Option<OutputPeerLossObservation> {
        self.public.as_ref()?.output_peer_loss_observation
    }

    fn ordinary_policy_settlement_idle(&self) -> bool {
        self.public
            .as_ref()
            .is_none_or(LivePublicPolicyState::ordinary_policy_settlement_idle)
    }

    fn take_output_topology_effect(
        &mut self,
    ) -> Option<crate::live_output_authority::LiveOutputAuthorityEffect> {
        self.public.as_mut()?.take_output_topology_effect()
    }

    fn take_output_topology_reload_request(&mut self) -> bool {
        self.public
            .as_mut()
            .is_some_and(LivePublicPolicyState::take_output_topology_reload_request)
    }

    fn published_output_snapshot(&self) -> Option<sophia_protocol::OutputAuthoritySnapshot> {
        self.public.as_ref()?.published_output_snapshot()
    }

    fn admit_reloaded_output_topology(
        &mut self,
        candidate: sophia_protocol::OutputTopologyCandidate,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match self.public.as_mut() {
            Some(public) => public.admit_reloaded_output_topology(candidate),
            None => Ok(false),
        }
    }

    fn output_topology_cancellation_reason(&self, transaction: TransactionId) -> Option<String> {
        self.public
            .as_ref()?
            .output_candidate_cancellation_reason(transaction)
            .map(str::to_owned)
    }

    fn output_candidate_active(&self) -> bool {
        self.public
            .as_ref()
            .is_some_and(LivePublicPolicyState::output_candidate_active)
    }

    fn output_authority_topology_epoch(&self) -> Option<u64> {
        self.public
            .as_ref()
            .and_then(LivePublicPolicyState::output_authority_topology_epoch)
    }

    fn publish_output_authority_snapshot(
        &mut self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
        capabilities: Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(public) = self.public.as_mut() else {
            return Ok(false);
        };
        public.publish_output_authority_snapshot(snapshot, capabilities)
    }

    fn reject_output_topology_effect(
        &mut self,
        transaction: TransactionId,
        failure: sophia_engine::OutputTopologyTransactionFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output effect observation requires the public policy owner")?
            .reject_output_topology_effect(transaction, failure)
    }

    fn begin_output_topology_apply(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output apply requires the public policy owner")?
            .begin_output_topology_apply(transaction, heads)
    }

    fn observe_output_topology_applied(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output apply requires the public policy owner")?
            .observe_output_topology_applied(transaction, heads)
    }

    fn observe_output_topology_first_presented(
        &mut self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<Option<sophia_protocol::OutputAuthoritySnapshot>, Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output presentation requires the public policy owner")?
            .observe_output_topology_first_presented(transaction, outputs)
    }

    fn preview_output_topology_first_presented(
        &self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<sophia_protocol::OutputAuthoritySnapshot, Box<dyn std::error::Error>> {
        self.public
            .as_ref()
            .ok_or("output presentation preview requires the public policy owner")?
            .preview_output_topology_first_presented(transaction, outputs)
    }

    fn observe_output_topology_rolled_back(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output rollback requires the public policy owner")?
            .observe_output_topology_rolled_back(transaction, heads)
    }

    fn observe_output_topology_rollback_failed(
        &mut self,
        transaction: TransactionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output rollback requires the public policy owner")?
            .observe_output_topology_rollback_failed(transaction)
    }
}
