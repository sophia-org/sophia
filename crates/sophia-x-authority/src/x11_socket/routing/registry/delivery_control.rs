// Control routing: the control writers, completion registries, execution
// claims and stale acknowledgements of the route registry. Included from
// registry.rs beside delivery.rs, which held them until it passed the
// source ceiling with the passive grab (t220).

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn route_control(
        &self,
        route: XAuthorityClientControlCommand,
    ) -> Result<(), XServerFrontendRouteError> {
        self.route_control_with_completion(route, None)
    }

    /// Install the completion registry a private instance owns.
    ///
    /// Once only. A second install would leave client writers registered
    /// before it reporting outcomes to a registry nobody reads.
    fn install_control_completion(&self, completion: ControlCompletionRegistry) -> bool {
        self.control_completion.set(completion).is_ok()
    }

    /// The completion registry a client writer should report outcomes to.
    fn control_completion(&self) -> Option<ControlCompletionRegistry> {
        self.control_completion.get().cloned()
    }

    /// Record that this client's control writer has stopped.
    ///
    /// However it stopped: a stop flag, a disconnected queue, a failure
    /// partway, a full acknowledgement channel, or an unwind. A registration
    /// can outlive its writer -- returning on a full channel is exactly that
    /// -- so this is a fact about the writer, not about the registration.
    fn mark_control_writer_gone(&self, client: XServerFrontendClientId) {
        if let Ok(clients) = self.clients.lock()
            && let Some(senders) = clients.get(&client)
        {
            senders.control_writer_gone.store(true, Ordering::Release);
        }
    }

    /// Whether this client has a control writer that could still execute work.
    ///
    /// Positive: the client is registered here and its writer has not stopped.
    /// An unreadable registry answers no, because accepting work on a
    /// question nobody could answer is how work is accepted for a writer that
    /// has gone.
    fn control_writer_present(&self, client: XServerFrontendClientId) -> bool {
        self.clients
            .lock()
            .ok()
            .and_then(|clients| {
                clients
                    .get(&client)
                    .map(|senders| !senders.control_writer_gone.load(Ordering::Acquire))
            })
            .unwrap_or(false)
    }

    /// Enter a routing call as an executor for this client.
    ///
    /// A control with no registration is ungoverned and routes as it always
    /// did. One with a registration needs something already executing for its
    /// client, or there is nothing to route it to.
    fn enter_routing_execution(
        &self,
        client: XServerFrontendClientId,
        completion: Option<ControlCompletionToken>,
    ) -> Option<ControlExecutorLease> {
        match (self.control_completion.get(), completion) {
            (Some(registry), Some(_)) => registry.enter_routing(client),
            (_, None) => Some(ControlExecutorLease::ungoverned()),
            (None, Some(_)) => None,
        }
    }

    /// Claim execution of a control before anything authoritative happens.
    ///
    /// A command with no registration is ungoverned and routes as it always
    /// did. A registration with no registry to answer to is refused: nothing
    /// here could establish who owns the outcome.
    fn claim_control_execution(
        &self,
        completion: Option<ControlCompletionToken>,
    ) -> ControlExecutionClaim {
        match (self.control_completion.get(), completion) {
            (Some(registry), Some(token)) => registry.claim_execution(token),
            (_, None) => ControlExecutionClaim::Ungoverned,
            (None, Some(_)) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::Unavailable)
            }
        }
    }

    /// Route a control, carrying a completion registration when the private
    /// path made one.
    ///
    /// Both producer routes take the token. `route_focus_control` returns
    /// early below for focus commands, so attaching it only at the
    /// construction further down would cover ordinary control and silently
    /// miss focus.
    fn route_control_with_completion(
        &self,
        route: XAuthorityClientControlCommand,
        completion: Option<ControlCompletionToken>,
    ) -> Result<(), XServerFrontendRouteError> {
        if !self.input_recovery.active(None, route.client) {
            return Err(XServerFrontendRouteError::UnknownClient { client: route.client });
        }
        // Taken before the first authoritative effect and held across all of
        // them, because routing is one of them: focus routing sends FocusOut
        // and moves the focused surface before any writer runs. Holding it is
        // what stops a writer's exit abandoning an operation this call is
        // still inside.
        //
        // Taking it is also the liveness check, so there is no gap between
        // deciding this client has an executor and being one. A separate
        // precheck could be true and then false before the claim.
        let Some(_executing) = self.enter_routing_execution(route.client, completion) else {
            return Err(XServerFrontendRouteError::UnknownClient { client: route.client });
        };
        // Before the first authoritative effect, which is not the writer.
        // Focus routing sends FocusOut to the previously focused client and
        // moves the focused surface before any writer runs, so a claim taken
        // at the writer would leave those effects behind a record still
        // reporting the operation unexecuted.
        if !self.claim_control_execution(completion).permits_effects() {
            return Err(XServerFrontendRouteError::ControlNotClaimable {
                client: route.client,
            });
        }
        self.retain_control_route_source(route, completion)?;
        if let Some(result) = self.route_focus_control(route, completion) {
            return result;
        }
        let senders = self.client_senders(route.client)?;
        let incarnation = senders.connection_state.clone();
        self.route_control_to_client(
            route.client,
            &incarnation,
            senders.control,
            X11RoutedControl::Authority {
                command: route.command,
                focus: None,
                claim: None,
                completion,
            },
        )
    }

    fn acknowledge_stale_control(
        &self,
        route: XAuthorityClientControlCommand,
    ) -> Result<(), XServerFrontendRouteError> {
        match self
            .acknowledgement_sender
            .try_send(XAuthorityClientControlAck {
                client: route.client,
                acknowledgement: XAuthorityControlAck {
                    kind: route.command.kind(),
                    transaction: route.command.transaction(),
                    surface: route.command.surface(),
                    outcome: XAuthorityControlOutcome::ClientGone,
                },
            }) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => Ok(()),
            Err(TrySendError::Full(_)) => {
                Err(XServerFrontendRouteError::ClientQueueFull {
                    client: route.client,
                })
            }
        }
    }
}
