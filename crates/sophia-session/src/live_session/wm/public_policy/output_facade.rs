impl LiveWmSession {
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
        self.public.as_ref().is_some_and(|public| public.startup_output_transaction.is_some())
    }

    fn is_startup_output_transaction(&self, transaction: TransactionId) -> bool {
        self.public
            .as_ref()
            .is_some_and(|public| public.startup_output_transaction == Some(transaction))
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

    fn output_topology_cancellation_reason(
        &self,
        transaction: TransactionId,
    ) -> Option<String> {
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
