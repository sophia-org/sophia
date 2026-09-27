use super::*;
impl Fixture {
    pub(super) fn service(&mut self) -> Result<(), ShellTransportError> {
        if self.state == 0 || self.state == 3 {
            return Ok(());
        }
        if self.state == 1 {
            if self
                .transport
                .poll_negotiation(&mut self.registry, 65536)?
                .is_some()
            {
                self.state = 2;
            }
            return Ok(());
        }
        self.transport.poll_io(&mut self.registry)?;
        if self.bar {
            return self.indicator_service();
        }
        if self.native && self.transport.native_launcher_state().is_none() {
            return Ok(());
        }
        let catalog = self.catalog();
        let allocations = self.transport.content_allocation_snapshots(&self.registry);
        let context = ContentCandidateContext {
            output: output(),
            facts_generation: 3,
            interaction_generation: 4,
            allocations: &allocations,
        };
        let state = self.transport.native_launcher_state().map_or(1, |s| s.1);
        let native_context = NativeLauncherCandidateContext {
            opening: self.opening(),
            state_revision: state,
            catalog: &catalog,
        };
        if !self.hold_candidates {
            if self.native {
                self.transport.service_native_launcher_content(
                    &mut self.registry,
                    context,
                    native_context,
                    10,
                )?;
            } else {
                self.transport
                    .service_content_allocation_requests(&mut self.registry, &[], 10)?;
                self.transport
                    .service_content_resources(&mut self.registry, 10)?;
                self.transport.service_content_demands(
                    &mut self.registry,
                    &[output()],
                    &allocations,
                )?;
                self.transport.service_catalog_candidates(
                    &mut self.registry,
                    &[context],
                    &catalog,
                    10,
                )?;
            }
        }
        while let Some((_, request)) = self
            .transport
            .next_content_allocation_request(&self.registry)
        {
            let allocation = self.allocation();
            self.transport.grant_content_allocation(
                &mut self.registry,
                request.allocation_request_id,
                allocation,
                &[],
            )?;
        }
        while let Some((transaction, demand)) = self.transport.next_content_demand(&self.registry) {
            self.permit += 1;
            self.transport.grant_content_demand(
                &mut self.registry,
                transaction,
                demand.output,
                self.permit,
                10,
            )?;
        }
        if self.prepared.is_none()
            && let Some((_, generation)) = self.transport.next_content_submission(&self.registry)
        {
            let render = if self.native {
                self.transport.begin_native_launcher_submission(
                    &mut self.registry,
                    generation,
                    context,
                    native_context,
                    10,
                )?
            } else {
                self.transport
                    .connection(&mut self.registry)
                    .begin_catalog_submission(generation, context, &catalog, 10)?
            };
            assert_eq!(
                render
                    .resource(ContentResourceId {
                        id: 5,
                        generation: 1
                    })
                    .unwrap()
                    .bytes()
                    .len(),
                8
            );
            self.transport.content_prepared(
                &mut self.registry,
                self.grant,
                output(),
                generation,
                7,
                8,
                10,
            )?;
            drop(render);
            self.prepared = Some(generation);
        }
        if self.native {
            if let Some((_, _, _)) = self
                .transport
                .poll_native_launcher_input_ack(&mut self.registry)?
            {
                self.input_acks += 1;
            }
            if !self.hold_activation
                && let Some((transaction, activation)) = self
                    .transport
                    .poll_native_launcher_activation(&mut self.registry)?
            {
                let decision = match self.transport.native_launcher_activation_eligibility(
                    transaction,
                    &activation,
                    &catalog,
                    20_000,
                )? {
                    NativeLauncherActivationEligibility::Rejected(d) => d,
                    NativeLauncherActivationEligibility::Keyboard
                    | NativeLauncherActivationEligibility::Pointer => {
                        self.admissions += 1;
                        NativeLauncherActivationDecision::Admitted
                    }
                };
                self.transport.finish_native_launcher_activation(
                    &self.registry,
                    transaction,
                    &activation,
                    decision,
                )?;
            }
        } else if !self.hold_activation
            && let Some((transaction, activation)) = self
                .transport
                .connection(&mut self.registry)
                .poll_catalog_activation()?
        {
            // Scripted Session decision, not a duplicate production owner.
            let valid = activation.catalog_generation == self.catalog_generation
                && self.last_action.as_ref() == Some(&activation.action);
            let status = if valid {
                self.admissions += 1;
                self.last_action = None;
                1
            } else {
                2
            };
            self.transport
                .connection(&mut self.registry)
                .finish_catalog_activation(transaction, &activation, status)?;
            self.transport.retain_content_action_reservations(|_| false);
        }
        Ok(())
    }
    fn indicator_service(&mut self) -> Result<(), ShellTransportError> {
        if self.hold_activation {
            return Ok(());
        }
        if let Some((transaction, activation)) = self
            .transport
            .poll_indicator_activation(&mut self.registry)?
        {
            let status = if activation.connection_epoch != self.grant.connection_epoch
                || activation.snapshot_generation != self.indicator_generation
                || activation.event_id <= self.indicator_watermark
            {
                ShellIndicatorActivationStatus::Stale
            } else if activation.output != OutputId::from_raw(2)
                || activation.indicator != 1
                || activation.action != 2
            {
                ShellIndicatorActivationStatus::Unknown
            } else {
                self.indicator_watermark = activation.event_id;
                self.admissions += 1;
                ShellIndicatorActivationStatus::Accepted
            };
            self.transport.finish_indicator_activation(
                &mut self.registry,
                transaction,
                &activation,
                status,
                0,
            )?;
        }
        Ok(())
    }
}
