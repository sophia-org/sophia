/// Authority-side endpoint for a portal executor. Broker-visible values stop
/// at the grant and payload; retained XIDs, atoms, properties, and event
/// routing remain private to this object.
#[cfg(unix)]
#[derive(Clone)]
pub struct XServerFrontendClipboardExecutor {
    state: X11CoreSocketServerState,
    routing: XServerFrontendRouteRegistry,
}

#[cfg(unix)]
impl XServerFrontendClipboardExecutor {
    /// Read owner facts without disclosing X resource identities to policy.
    pub fn source_generation(
        &self,
        request: &sophia_protocol::PortalRequest,
    ) -> Option<u64> {
        self.state.runtime.lock().ok()?.clipboard_source_generation(request)
    }

    pub fn request_source(
        &self,
        grant: &sophia_protocol::PortalGrant,
    ) -> Result<crate::ClipboardSelectionProxy, X11SetupSocketError> {
        let proxy = self
            .state
            .runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
            .begin_clipboard_source_request(grant)
            .map_err(|error| {
                X11SetupSocketError::new(format!("clipboard source request rejected: {error:?}"))
            })?;
        let target = self
            .state
            .client_for_resource(proxy.owner)?
            .ok_or_else(|| X11SetupSocketError::new("clipboard owner disconnected"))?;
        self.routing
            .route_protocol_contained(
                target,
                XClientEvent::SelectionRequest {
                    sequence: 0,
                    time: proxy.time,
                    owner: proxy.owner,
                    requestor: proxy.requestor,
                    selection: proxy.selection,
                    target: proxy.target,
                    property: proxy.property,
                },
            )
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to route clipboard source request: {error}"
                ))
            })?;
        Ok(proxy)
    }

    pub fn execute(
        &self,
        grant: &sophia_protocol::PortalGrant,
        payload: &[u8],
    ) -> Result<crate::ClipboardSelectionExecutionOutcome, X11SetupSocketError> {
        let outcome = self.execute_effect(grant, payload)?;
        self.route_outcome(&outcome)?;
        Ok(outcome)
    }

    /// Property custody has already changed before notification is routed.
    /// A disconnected notification recipient cannot turn that effect into a
    /// claim that no execution happened.
    pub(crate) fn execute_portal(
        &self,
        grant: &sophia_protocol::PortalGrant,
        payload: &[u8],
    ) -> Result<crate::ClipboardSelectionExecutionOutcome, X11SetupSocketError> {
        let outcome = self.execute_effect(grant, payload)?;
        if let Err(error) = self.route_outcome(&outcome) {
            tracing::warn!("portal recipient notification unavailable: {error}");
        }
        Ok(outcome)
    }

    fn execute_effect(
        &self,
        grant: &sophia_protocol::PortalGrant,
        payload: &[u8],
    ) -> Result<crate::ClipboardSelectionExecutionOutcome, X11SetupSocketError> {
        let mut runtime = self
            .state
            .runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
        let mut atoms = self
            .state
            .atoms
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 atom table lock poisoned"))?;
        let mut properties = self
            .state
            .properties
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 property table lock poisoned"))?;
        let outcome = runtime
            .execute_clipboard_payload(grant.transfer, grant, payload, &mut atoms, &mut properties)
            .map_err(|error| {
                X11SetupSocketError::new(format!("clipboard executor rejected payload: {error:?}"))
            })?;
        drop(properties);
        drop(atoms);
        drop(runtime);
        Ok(outcome)
    }

    pub fn fail(
        &self,
        transfer: sophia_protocol::PortalTransferId,
        error: crate::ClipboardSelectionExecutionError,
    ) -> Result<crate::ClipboardSelectionExecutionOutcome, X11SetupSocketError> {
        let mut runtime = self
            .state
            .runtime
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
        let proxies = runtime.discard_clipboard_proxies(transfer);
        let outcome = runtime
            .fail_clipboard_transfer(transfer, error)
            .map_err(|error| {
                X11SetupSocketError::new(format!("clipboard failure rejected: {error:?}"))
            })?;
        drop(runtime);
        if !proxies.is_empty() {
            let mut properties = self
                .state
                .properties
                .lock()
                .map_err(|_| X11SetupSocketError::new("X11 property table lock poisoned"))?;
            for (namespace, proxy) in proxies {
                properties.remove_window(namespace, proxy);
            }
        }
        self.route_outcome(&outcome)?;
        Ok(outcome)
    }

    fn route_outcome(
        &self,
        outcome: &crate::ClipboardSelectionExecutionOutcome,
    ) -> Result<(), X11SetupSocketError> {
        let notify = match &outcome {
            crate::ClipboardSelectionExecutionOutcome::Handoff(handoff) => handoff.notify,
            crate::ClipboardSelectionExecutionOutcome::Failed { notify, .. } => *notify,
        };
        let target = self
            .state
            .client_for_resource(notify.requestor)?
            .ok_or_else(|| X11SetupSocketError::new("clipboard requestor disconnected"))?;
        self.routing
            .route_protocol_contained(
                target,
                XClientEvent::SelectionNotify {
                    sequence: 0,
                    synthetic: false,
                    time: notify.time,
                    requestor: notify.requestor,
                    selection: notify.selection,
                    target: notify.target,
                    property: notify.property,
                },
            )
            .map_err(|error| {
                X11SetupSocketError::new(format!("failed to route clipboard notify: {error}"))
            })?;
        Ok(())
    }
}

/// Coordinates one retained X11 selection through the broker socket. The
/// grant is obtained before the source proxy is exposed, and only the bounded
/// captured bytes return over the portal connection.
#[cfg(unix)]
pub fn coordinate_x11_clipboard_transfer(
    path: impl AsRef<Path>,
    request: &sophia_protocol::PortalBrokerRequestPacket,
    executor: &XServerFrontendClipboardExecutor,
    routes: &XServerFrontendRouteBroker,
    timeout: Duration,
) -> Result<sophia_protocol::PortalBrokerResponsePacket, X11SetupSocketError> {
    let mut session =
        sophia_portal::begin_portal_clipboard_request(path, request).map_err(|error| {
            X11SetupSocketError::new(format!("portal broker request failed: {error}"))
        })?;
    let decision = session.response().decision.clone();
    match &decision {
        sophia_protocol::PortalBrokerResponseDecision::Denied => {
            executor.fail(
                session.response().transfer,
                crate::ClipboardSelectionExecutionError::Denied,
            )?;
        }
        sophia_protocol::PortalBrokerResponseDecision::Allowed(grant) => {
            executor.request_source(grant)?;
            let payload = routes
                .recv_clipboard_source_payload_timeout(timeout)
                .map_err(|error| {
                    let _ = executor.fail(
                        grant.transfer,
                        crate::ClipboardSelectionExecutionError::Expired,
                    );
                    X11SetupSocketError::new(format!(
                        "clipboard source payload unavailable: {error}"
                    ))
                })?;
            if payload.transfer != grant.transfer {
                executor.fail(
                    grant.transfer,
                    crate::ClipboardSelectionExecutionError::ExecutorFailure,
                )?;
                return Err(X11SetupSocketError::new(
                    "clipboard source payload correlation mismatch",
                ));
            }
            session.send_payload(&payload.bytes).map_err(|error| {
                let _ = executor.fail(
                    grant.transfer,
                    crate::ClipboardSelectionExecutionError::ExecutorFailure,
                );
                X11SetupSocketError::new(format!("portal payload send failed: {error}"))
            })?;
        }
    }
    Ok(session.into_response())
}
