// One accounted dequeue and its original service allowance.

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// A queued, newly accepted item alone is not existing cleanup debt.
    /// Everything already taken remains conservatively eligible until its
    /// owning record has explicitly retired it.
    fn cleanup_readiness(&self) -> sophia_input_authority::CleanupReadiness {
        if self.terminal.cleanup_is_empty()
            && self.outstanding.is_empty()
            && self.routing.is_none()
            && self.parked.is_none()
            && self.parked_barrier.is_none()
        {
            sophia_input_authority::CleanupReadiness::NoneEligible
        } else {
            sophia_input_authority::CleanupReadiness::Eligible
        }
    }
}

#[cfg(unix)]
impl PrivatePreparedRunner {
    /// Check the allowance before dequeue, and charge only the item actually
    /// taken. Both accounting guards are local; the work they describe is
    /// already in the frontend before either can fail or unwind.
    fn execute_accounted_step(
        &mut self,
    ) -> Result<PrivateAccountedStep, XServerFrontendRouteError> {
        use sophia_input_authority::ServiceWork;
        let Self {
            watch,
            frontend,
            keyboards,
            service_origin,
            service,
            ..
        } = self;
        let cleanup = frontend.as_ref().expect("live runner").cleanup_readiness();
        let admission =
            match service.prepare(service_origin.elapsed(), ServiceWork::NewWork, cleanup) {
                Ok(admission) => admission,
                Err(cause) => return Ok(PrivateAccountedStep::Yield { cause, taken: None }),
            };
        let mut admission = Some(admission);
        let mut running = None;
        let mut refused = None;
        let mut taken = None;
        #[cfg(all(test, unix))]
        let registry = frontend
            .as_ref()
            .expect("live runner")
            .broker
            .registry
            .clone();
        let result = frontend.as_mut().expect("live runner").step_once_accounted(
            keyboards,
            &mut |sequence, taken_at, cleanup| {
                taken = Some(sequence);
                let admission = admission
                    .take()
                    .ok_or(XServerFrontendRouteError::OrderedItemUnresolved)?;
                let Some(elapsed) = taken_at.checked_duration_since(*service_origin) else {
                    refused = Some(sophia_input_authority::ServiceStartRefusal::ClockRegressed);
                    return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                };
                // The source supplies its fresh observation immediately before
                // taking this item. The just-taken item is new work, not a
                // pre-existing cleanup obligation that revokes donation.
                #[cfg(all(test, unix))]
                routing_tests::m3_acceptance::dequeue_accounting(&registry, sequence, cleanup);
                match admission.dequeued(elapsed, cleanup) {
                    Ok(run) => {
                        running = Some(run);
                        Ok(())
                    }
                    Err(cause) => {
                        refused = Some(cause);
                        Err(XServerFrontendRouteError::OrderedItemUnresolved)
                    }
                }
            },
            watch.as_ref().expect("prepared supervisor"),
        );
        // Idle/Blocked never called the hook. Dropping that admission neither
        // consumes an interval nor marks an interrupted execution.
        // Every returned Result finishes accounting, including a refused
        // execution. An unwind instead drops the ServiceRun and permanently
        // closes this budget while the accepted item remains instance-owned.
        let charge = running
            .map(|run| run.finish(service_origin.elapsed()))
            .transpose()
            .map_err(|_| XServerFrontendRouteError::OrderedItemUnresolved)?;
        if let Some(cause) = refused {
            if frontend
                .as_ref()
                .expect("live runner")
                .terminal
                .current_is_frozen
            {
                taken = None;
            }
            return Ok(PrivateAccountedStep::Yield { cause, taken });
        }
        Ok(PrivateAccountedStep::Step {
            step: result?,
            charge,
        })
    }
}
