// Handing work in and taking it out: the producers a private instance exposes,
// and the routing that applies what they accepted.
//
// Split from construction by subject. Building an instance is a transaction
// over handles and slots; this is the surface that runs afterwards, and every
// act on it asks the same question -- is the owner that keeps this service's
// connections' evidence still there, and is it this service's?

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Submit work, and be told why if it is not accepted.
    ///
    /// Reservation happens before acceptance and is rolled back exactly when
    /// acceptance fails, so a refusal leaves no reservation behind and no
    /// other request's delivery is disturbed: only this envelope's own id is
    /// aborted, and only when this envelope was the one that failed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn submit(
        &self,
        service: &PrivateServiceLease<'_>,
        route: XAuthorityRoutedInput,
    ) -> Result<crate::ReadySequence, PrivateSendError> {
        self.ingress().submit(service, route)
    }

    /// The stamped ingress, as a private handle.
    ///
    /// Not the ordinary sender. Handing that out left `try_send` reachable
    /// from a private frontend, and it reports a policy denial as `Full`, so
    /// claiming every private producer error is typed would have been false
    /// while that escape existed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn ingress(&self) -> PrivateIngress {
        PrivateIngress {
            sender: self.broker.routed_input_sender(),
            admission: Arc::clone(&self.admission),
            role: None,
            requests: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            barrier: std::sync::OnceLock::new(),
        }
    }

    /// The stamped ingress for one admitted producer, reserving its requests
    /// against this instance's authority before they are published.
    ///
    /// This is the production shape. The capability is issued here, on the
    /// origin's side of the handover, and what the producer receives is the
    /// right to reserve and to observe its own outcomes -- never the authority
    /// and never the issuer.
    /// Against whichever admission is current. Production names one instead,
    /// so only controls reach this.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn ingress_for(
        &mut self,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateIngress, PrivateAdmissionRefusal> {
        self.ingress_for_admission(client, device, None)
    }

    pub(crate) fn ingress_for_admission(
        &mut self,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
        expected: Option<sophia_protocol::ClientAdmissionId>,
    ) -> Result<PrivateIngress, PrivateAdmissionRefusal> {
        if !self.admission.lifecycle_open() {
            return Err(PrivateAdmissionRefusal::Unreachable);
        }
        // Claimed when a reserving producer is exposed, not when the first
        // ordered turn happens to run. Between those two moments the older
        // route could drain reserved work and apply it without the execution
        // its reservation exists for, which is the case this refusal is for.
        self.ordered_runner = true;
        Ok(PrivateIngress {
            sender: self.broker.routed_input_sender(),
            admission: Arc::clone(&self.admission),
            role: Some(self.reservation_role_for(client, device, expected)?),
            requests: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            barrier: std::sync::OnceLock::new(),
        })
    }

    /// A producer handle for control, bound to this instance's admission.
    ///
    /// A second real producer class, so the shared order is something two
    /// producers actually contend for rather than one producer's queue with a
    /// new name.
    pub(crate) fn control_producer(&self) -> PrivateControlProducer {
        PrivateControlProducer {
            admission: Arc::clone(&self.admission),
            completion: self.completion.clone(),
            routing: self.broker.registry.clone(),
        }
    }

    /// Returns what it ran, in the order it took them.
    ///
    /// The order is a return value rather than a count, because a caller that
    /// can only see how many ran cannot tell an ordered consumer from one that
    /// grouped entries someone else had already numbered.
    /// THE LEASE IS ASKED FOR HERE TOO, and this is the path an unprepared
    /// instance still has. A frontend that was never prepared into a runner
    /// carries no borrow of its owner, so without this it could go on taking
    /// accepted work and applying it after its keeper was gone -- which is the
    /// same continuing service by another entry point.
    pub fn route_pending(
        &mut self,
        service: &PrivateServiceLease<'_>,
    ) -> Result<Vec<PrivateRun>, XServerFrontendRouteError> {
        if !self.broker.registry.leased_by(service) {
            return Err(XServerFrontendRouteError::ForeignServiceOwner);
        }
        // One permitted consumer per order. This route discards the
        // reservation an item was published with and applies the work without
        // the execution that reservation exists for, so letting it drain an
        // order the ordered consumer is draining would apply accepted work
        // behind that consumer's back and leave its request unanswerable.
        if self.ordered_runner {
            return Err(XServerFrontendRouteError::OrderedRunnerEngaged);
        }
        // Bounded by what the queue can hold, not by when producers stop.
        // Draining until empty lets a producer that keeps replenishing hold
        // this turn open and grow the report without limit, which is an
        // unbounded allocation added to production to satisfy a test.
        let budget = self.service_budget;
        let mut ran = Vec::with_capacity(budget);
        while ran.len() < budget {
            let next = self
                .admission
                .take_next()
                .map_err(|()| XServerFrontendRouteError::RegistryPoisoned)?;
            let Some((sequence, class, operation)) = next else {
                break;
            };
            let identity = PrivateIdentity::of(&operation);
            match self.run_one(operation) {
                // Routed or enqueued, which is not the same as answered. The
                // credit stays with the work until its real terminal outcome,
                // because route_control only hands a command to a client
                // writer and routed input can sit writer-pending or frozen.
                Ok(()) => self.outstanding.push(identity),
                Err(error) => {
                    // The operation is consumed by now, so its credit has
                    // nothing left to travel with. Recorded as outstanding
                    // rather than released, so a failure cannot look like a
                    // completion and free capacity for new work.
                    self.outstanding.push(identity);
                    return Err(error);
                }
            }
            // Pushed into capacity taken before the effect, so recording never
            // allocates after something has already happened.
            ran.push(PrivateRun {
                sequence,
                class,
                identity,
            });
        }
        Ok(ran)
    }

    /// Run one operation, whichever order it came from.
    fn run_one(&mut self, operation: PrivateOperation) -> Result<(), XServerFrontendRouteError> {
            match operation {
                PrivateOperation::LeaseRelease(release) => {
                    self.broker.registry.release_route_lease(release)?;
                }
                PrivateOperation::RoutedInput(route) => {
                    let admitted = self
                        .broker
                        .control_gate
                        .get()
                        .is_some_and(|gate| {
                            gate.admits(crate::ControlStamp {
                                control_epoch: route.control_epoch,
                                publication: route.publication,
                            })
                            .is_ok()
                        });
                    self.broker.registry.route_engine_input_admitted(
                        route.route,
                        crate::ControlStamp {
                            control_epoch: route.control_epoch,
                            publication: route.publication,
                        },
                        admitted,
                    )?;
                }
                PrivateOperation::Control(control, completion) => {
                    self.broker
                        .registry
                        .route_control_with_completion(control, completion)?;
                }
            }
        Ok(())
    }
}
