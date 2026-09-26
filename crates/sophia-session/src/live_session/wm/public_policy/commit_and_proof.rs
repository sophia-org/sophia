impl LiveWmSession {
    fn prepare_public_layout_commit(
        &mut self,
        layout: &PersistentLiveLayout,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(identity) = layout
            .pending
            .as_ref()
            .and_then(|pending| pending.policy_settlement)
        else {
            return Ok(true);
        };
        if identity.session_operation {
            return Ok(true);
        }
        let public = self.public.as_mut().ok_or("public settlement lost its session")?;
        let staged = public
            .staged
            .as_ref()
            .ok_or("ready public layout lost its staged reducer successor")?;
        let outcome = public.reducer.revalidate_staged(staged);
        if public.in_flight_request.as_ref().is_some_and(|request| !public.presented_cause_is_current(request.cause)) {
            return Ok(false);
        }
        if staged.presentation_publication().is_some_and(|(_, presentation)| {
            sophia_protocol::validate_policy_presentation_actions(presentation, &public.actions).is_err()
        }) {
            return Ok(false);
        }
        if outcome == sophia_protocol::PolicyProjectionOutcome::RejectedStale {
            return Ok(false);
        }
        if outcome != sophia_protocol::PolicyProjectionOutcome::Committed {
            return Err(format!(
                "ready public layout failed canonical revalidation: {outcome:?}"
            )
            .into());
        }
        if public.prepared == Some(identity) {
            return Ok(true);
        }
        public.prepared = Some(identity);
        crate::session_println!(
            "sophia_live_wm_chrome schema=2 status=acknowledged transaction={} request_id={} scene_generation={}",
            identity.transaction.raw(),
            identity.request_id,
            identity.scene_generation,
        );
        Ok(true)
    }

    fn trigger_public_proof_fault(&mut self, point: PublicPolicyFaultPoint) -> bool {
        let trigger = self.public.as_mut().is_some_and(|public| {
            if public.proof_fault_triggered || public.proof_fault_after != Some(point) {
                return false;
            }
            public.proof_fault_triggered = true;
            true
        });
        if trigger {
            self.request_transport_restart("public_policy_proof_fault", Some(point.name()));
            crate::session_println!(
                "sophia_live_wm schema=4 status=proof_fault_triggered adapter={} phase={} preserved_layout=true",
                self.policy_wire_name(), point.name(),
            );
        }
        trigger
    }

    fn poll_public_proof_restart(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(public) = self.public.as_mut() else {
            return Ok(());
        };
        let Some(before) = public.proof_restart_checkpoint_before else {
            return Ok(());
        };
        let current = policy_checkpoint_identity(&public.checkpoint_path)?;
        if !policy_checkpoint_replaced(before, current) {
            return Ok(());
        }
        let action = public
            .proof_restart_after_action
            .expect("an armed checkpoint restart has an action");
        public.proof_restart_checkpoint_before = None;
        public.proof_restart_triggered = true;
        self.request_transport_restart("public_policy_checkpoint_proof", None);
        crate::session_println!(
            "sophia_live_wm schema=4 status=proof_restart_triggered adapter={} phase=checkpoint_saved action={} preserved_layout=true",
            self.policy_wire_name(), action.raw(),
        );
        Ok(())
    }

    fn public_settlement_abort_required(&self) -> bool {
        self.public
            .as_ref()
            .is_some_and(|public| public.transport_unavailable)
    }
}
