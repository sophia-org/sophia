/// A control's original execution and unresolved output, kept by its already
/// reserved completion record before the first native effect.
#[cfg(unix)]
struct PrivateControlExecution {
    token: ControlCompletionToken,
    source: Arc<PrivateControlClientSource>,
    surface: SurfaceId,
    window: XResourceId,
    records: Vec<Vec<u8>>,
    emission: PrivateControlEmission,
    peer_generation_begun: bool,
    pending_metadata: Option<sophia_protocol::ReducedMetadataCandidate>,
    // Original generated event payloads survive partial peer routing. A
    // numeric target records intent only; it never authorizes later delivery
    // or retirement against a replacement connection.
    generated_events: Vec<(Option<XServerFrontendClientId>, XClientEvent)>,
    focus_peers: Vec<PrivateControlFocusPeer>,
    dependent_records: Vec<Vec<u8>>,
    protocol_receipts: Vec<Arc<PrivateControlProtocolReceipt>>,
    protocol_cursor: usize,
}

#[cfg(unix)]
struct PrivateControlFocusPeer {
    recipient: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    window: XResourceId,
    time: u32,
    claim: Option<PrivateFocusClaim>,
    flushed: bool,
    superseded: bool,
}

#[cfg(unix)]
enum PrivateFocusPeerResolution {
    Flushed,
    Superseded,
}

#[cfg(unix)]
impl PrivateControlExecution {
    fn peer_debt_pending(&self) -> bool {
        self.peer_generation_begun
            || self
                .focus_peers
                .iter()
                .any(|peer| !peer.flushed && !peer.superseded)
            || self
                .protocol_receipts
                .iter()
                .any(|receipt| !receipt.settled())
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrivateControlEmission {
    NotStarted,
    Pending,
    Indeterminate,
    Flushed,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateControlCleanupRefusal {
    Unavailable,
    MissingSource,
    ForeignSource,
    UnsupportedProgress,
    RemovalWithheld,
    PublicationOutstanding,
}

#[cfg(unix)]
impl ControlRecord {
    fn source_debt_settled(&self) -> bool {
        self.source.as_ref().is_none_or(|source| {
            source.lock().is_ok_and(|source| {
                !source.peer_debt_pending() && source.pending_metadata.is_none()
            })
        })
    }
}

#[cfg(unix)]
impl ControlCompletionRegistry {
    fn execution_of(
        &self,
        token: ControlCompletionToken,
    ) -> Option<Arc<Mutex<PrivateControlExecution>>> {
        if token.origin != self.origin {
            return None;
        }
        self.inner
            .lock()
            .ok()?
            .records
            .iter()
            .find(|record| record.token == token)?
            .source
            .clone()
    }
    fn retain_execution_source(
        &self,
        token: ControlCompletionToken,
        source: &Arc<PrivateControlClientSource>,
        window: XResourceId,
    ) -> Result<Arc<Mutex<PrivateControlExecution>>, PrivateControlCleanupRefusal> {
        use PrivateControlCleanupRefusal as Refusal;
        if token.origin != self.origin
            || !std::sync::Weak::ptr_eq(&source.completion, &Arc::downgrade(&self.inner))
        {
            return Err(Refusal::ForeignSource);
        }
        let mut held = self.inner.lock().map_err(|_| Refusal::Unavailable)?;
        let record = held
            .records
            .iter_mut()
            .find(|record| record.token == token)
            .ok_or(Refusal::MissingSource)?;
        let ControlPhase::Applying(command) = record.phase else {
            return Err(Refusal::ForeignSource);
        };
        if command.client != source.endpoint.client {
            return Err(Refusal::ForeignSource);
        }
        if let Some(execution) = &record.source {
            // The router took custody before any focus effect. The writer
            // resumes that exact custody instead of replacing its history.
            let same = execution.lock().map_err(|_| Refusal::Unavailable)?;
            if !Arc::ptr_eq(&same.source, source) || same.window != window {
                return Err(Refusal::ForeignSource);
            }
            return Ok(execution.clone());
        }
        let execution = Arc::new(Mutex::new(PrivateControlExecution {
            token,
            source: source.clone(),
            surface: command.command.surface(),
            window,
            records: Vec::new(),
            emission: PrivateControlEmission::NotStarted,
            peer_generation_begun: false,
            pending_metadata: None,
            generated_events: Vec::new(),
            focus_peers: Vec::new(),
            dependent_records: Vec::new(),
            protocol_receipts: Vec::new(),
            protocol_cursor: 0,
        }));
        record.source = Some(execution.clone());
        Ok(execution)
    }

    fn cleanup_one(
        &self,
        origin: &XServerFrontendRouteRegistry,
        service: &PrivateServiceLease<'_>,
        collected: Option<&PrivateConnectionsCollected>,
        cursor: &mut usize,
        custody: &mut usize,
    ) -> Result<bool, PrivateTerminalDriveRefusal> {
        use PrivateControlCleanupRefusal as Refusal;
        // Visit the product of control records and physical custody slots.
        // This cursor is independent of native-recipient maintenance, which
        // must not change which pair this phase visits next.
        let places = service
            .owner
            .inventory
            .kept
            .lock()
            .map_err(|_| Refusal::Unavailable)?
            .places
            .len()
            .max(1);
        let place = *custody % places;
        *custody = (place + 1) % places;
        let (token, execution) = {
            let held = self.inner.lock().map_err(|_| Refusal::Unavailable)?;
            if held.records.is_empty() {
                return Ok(false);
            }
            let index = *cursor % held.records.len();
            if *custody == 0 {
                *cursor = (index + 1) % held.records.len();
            }
            let record = &held.records[index];
            if !matches!(
                record.phase,
                ControlPhase::Abandoned(_) | ControlPhase::Settled(_)
            ) {
                return Ok(false);
            }
            if record.dependents != 0 {
                return Ok(false);
            }
            (
                record.token,
                record
                    .source
                    .as_ref()
                    .cloned()
                    .ok_or(Refusal::MissingSource)?,
            )
        };
        if visit_control_protocol_receipt(
            &execution,
            origin,
            service,
            collected,
            place,
            *custody == 0,
        )? {
            return Ok(false);
        }
        {
            let operation = execution.lock().map_err(|_| Refusal::Unavailable)?;
            let source = &operation.source;
            let connection = source
                .endpoint
                .registration
                .get()
                .ok_or(Refusal::MissingSource)?;
            if !std::sync::Weak::ptr_eq(&connection.registry, &Arc::downgrade(&origin.clients))
                || !std::sync::Weak::ptr_eq(&source.completion, &Arc::downgrade(&self.inner))
            {
                return Err(Refusal::ForeignSource.into());
            }
            if operation.peer_debt_pending() || operation.pending_metadata.is_some() {
                return Err(Refusal::UnsupportedProgress.into());
            }
            {
                let teardown = source.teardown.lock().map_err(|_| Refusal::Unavailable)?;
                let removed = teardown.removed.as_ref().ok_or(Refusal::RemovalWithheld)?;
                if !removed.endpoint.matches(&source.endpoint) {
                    return Err(Refusal::ForeignSource.into());
                }
                if !removed
                    .resources
                    .destroyed_windows
                    .contains(&operation.window)
                {
                    return Err(Refusal::RemovalWithheld.into());
                }
                if !teardown.properties_removed
                    || !teardown.finished
                    || teardown.pending_publication.is_some()
                {
                    return Err(Refusal::PublicationOutstanding.into());
                }
            }
            let mut examined_place = place;
            let _terminated = PrivateRecipientTermination::from_place(
                service,
                origin,
                collected,
                &mut examined_place,
                &source.endpoint,
            )?;
            // This is the original connection's projection, not a lookup by a
            // reissued client number or a later namespace's apparent absence.
            connection
                .selections
                .lock()
                .map_err(|_| Refusal::Unavailable)?
                .remove(operation.window);
            source
                .tables
                .windows
                .lock()
                .map_err(|_| Refusal::Unavailable)?
                .remove(&operation.surface);
            source
                .tables
                .rules
                .lock()
                .map_err(|_| Refusal::Unavailable)?
                .remove(&operation.surface);
            source
                .tables
                .generations
                .lock()
                .map_err(|_| Refusal::Unavailable)?
                .remove(&operation.surface);
        }
        let mut held = self.inner.lock().map_err(|_| Refusal::Unavailable)?;
        let Some(index) = held.records.iter().position(|record| {
            record.token == token
                && matches!(
                    record.phase,
                    ControlPhase::Abandoned(_) | ControlPhase::Settled(_)
                )
                && record.dependents == 0
                && record
                    .source
                    .as_ref()
                    .is_some_and(|source| Arc::ptr_eq(source, &execution))
        }) else {
            return Ok(false);
        };
        let removed = held.records.remove(index);
        *cursor = index;
        *custody = 0;
        drop(held);
        drop(removed);
        Ok(true)
    }
}

#[cfg(unix)]
fn retain_private_control_events(
    execution: Option<&Arc<Mutex<PrivateControlExecution>>>,
    events: impl IntoIterator<Item = (Option<XServerFrontendClientId>, XClientEvent)>,
) -> Result<(), X11SetupSocketError> {
    if let Some(execution) = execution {
        let mut operation = execution
            .lock()
            .map_err(|_| X11SetupSocketError::new("control event custody unavailable"))?;
        for (target, event) in events {
            operation.generated_events.push((target, event));
        }
    }
    Ok(())
}

/// Called by the original dependent writer after actual projection and flush,
/// or the source's exact claim comparison established supersession. A missing
/// or unreadable claim never produces either disposition.
#[cfg(unix)]
fn record_private_focus_peer_resolution(
    dependent: Option<&ControlDependent>,
    claim: Option<&PrivateFocusClaim>,
    window: XResourceId,
    time: u32,
    resolution: PrivateFocusPeerResolution,
) -> Result<(), X11SetupSocketError> {
    let Some(held) = dependent.and_then(|dependent| dependent.held.as_ref()) else {
        return Ok(());
    };
    let Some(execution) = held.registry.execution_of(held.origin) else {
        return Ok(());
    };
    let mut operation = execution
        .lock()
        .map_err(|_| X11SetupSocketError::new("dependent control custody unavailable"))?;
    let Some(claim) = claim else {
        return Err(X11SetupSocketError::new(
            "dependent control has no original focus claim",
        ));
    };
    let peer = operation
        .focus_peers
        .iter_mut()
        .find(|peer| {
            peer.window == window
                && peer.time == time
                && Arc::ptr_eq(&peer.recipient, &claim.connection)
                && peer.claim.as_ref().is_some_and(|original| {
                    original.issued.generation == claim.issued.generation
                        && original.issued.window == claim.issued.window
                        && original.authority == claim.authority
                        && original.admission == claim.admission
                        && original.connection_generation == claim.connection_generation
                })
        })
        .ok_or_else(|| X11SetupSocketError::new("dependent flush names another original focus"))?;
    match resolution {
        PrivateFocusPeerResolution::Flushed if !peer.superseded => peer.flushed = true,
        PrivateFocusPeerResolution::Superseded if !peer.flushed => peer.superseded = true,
        _ => {
            return Err(X11SetupSocketError::new(
                "dependent focus source disposition conflicts",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn retain_control_route_source(
        &self,
        command: XAuthorityClientControlCommand,
        token: Option<ControlCompletionToken>,
    ) -> Result<(), XServerFrontendRouteError> {
        let Some(token) = token else {
            return Ok(());
        };
        let senders = self.client_senders(command.client)?;
        let Some(source) = senders
            .connection_state
            .get()
            .and_then(|state| state.control_source.get())
            .and_then(std::sync::Weak::upgrade)
        else {
            return Ok(());
        };
        let window = source
            .tables
            .windows
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .get(&command.command.surface())
            .copied()
            .ok_or(XServerFrontendRouteError::UnknownSurface {
                surface: command.command.surface(),
            })?;
        self.control_completion()
            .ok_or(XServerFrontendRouteError::RegistryPoisoned)?
            .retain_execution_source(token, &source, window)
            .map_err(|_| XServerFrontendRouteError::ControlNotClaimable {
                client: command.client,
            })?;
        Ok(())
    }

    fn retain_control_peer_debt(
        &self,
        token: Option<ControlCompletionToken>,
        recipient: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
        window: XResourceId,
        time: u32,
        claim: Option<&PrivateFocusClaim>,
    ) -> Result<(), XServerFrontendRouteError> {
        if let Some(execution) =
            token.and_then(|token| self.control_completion()?.execution_of(token))
        {
            let mut operation = execution
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            operation.focus_peers.push(PrivateControlFocusPeer {
                recipient: recipient.clone(),
                window,
                time,
                claim: claim.cloned(),
                flushed: false,
                superseded: false,
            });
        }
        Ok(())
    }
}

#[cfg(unix)]
impl From<PrivateControlCleanupRefusal> for PrivateTerminalDriveRefusal {
    fn from(refusal: PrivateControlCleanupRefusal) -> Self {
        Self::Control(refusal)
    }
}

#[cfg(unix)]
impl PrivateRetainedExecutionResources {
    fn visit_control_cleanup(
        origin: &XServerFrontendRouteRegistry,
        service: &PrivateServiceLease<'_>,
        collected: Option<&PrivateConnectionsCollected>,
        cursor: &mut PrivateTerminalDriveCursor,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        let completion = origin
            .control_completion()
            .ok_or(PrivateControlCleanupRefusal::MissingSource)?;
        cursor.control_reclaim = !cursor.control_reclaim;
        if cursor.control_reclaim {
            // Same control-credit rule as durable.drive, one original row at
            // a time. No input request credit is affected by this phase.
            let mut held = service
                .store()
                .inner
                .lock()
                .map_err(|_| PrivateTerminalDriveRefusal::StoreUnreadable)?;
            if held.outstanding.is_empty() {
                return Ok(PrivateTerminalVisit::Control { retired: false });
            }
            let index = cursor.control_credit % held.outstanding.len();
            cursor.control_credit = (index + 1) % held.outstanding.len();
            let (other, identity) = &held.outstanding[index];
            let retired = Arc::ptr_eq(&other.clients, &origin.clients)
                && matches!(identity, PrivateIdentity::Control { completion: Some(token), .. }
                    if completion.state_of(*token) == ControlRecordState::Retired);
            if retired {
                held.obligations_changed();
                held.outstanding.swap_remove(index);
                held.reserved = held.reserved.saturating_sub(1);
            }
            drop(held);
            Ok(PrivateTerminalVisit::Control { retired })
        } else {
            completion
                .cleanup_one(
                    origin,
                    service,
                    collected,
                    &mut cursor.controls,
                    &mut cursor.control_custody,
                )
                .map(|retired| PrivateTerminalVisit::Control { retired })
        }
    }
}

#[cfg(unix)]
fn write_private_control_records(
    execution: &Arc<Mutex<PrivateControlExecution>>,
    stream: &Arc<Mutex<X11ClientOutput>>,
    wire: &X11WirePermission,
    byte_order: XByteOrder,
    sequence: &AtomicU16,
    records: Vec<Vec<u8>>,
) -> Result<(), X11SetupSocketError> {
    let mut operation = execution
        .lock()
        .map_err(|_| X11SetupSocketError::new("control output custody unavailable"))?;
    operation.records = records;
    operation.emission = PrivateControlEmission::Pending;
    #[cfg(all(test, unix))]
    if operation.source.fail_before_write.load(Ordering::Acquire) {
        return Err(X11SetupSocketError::new(
            "staged interruption after actual control record generation",
        ));
    }
    let mut stream = enter_x11_wire(stream, wire)?;
    let event_sequence = sequence.load(Ordering::Acquire);
    operation.emission = PrivateControlEmission::Indeterminate;
    for record in &mut operation.records {
        write_xi_u16(byte_order, &mut record[2..4], event_sequence);
        stream
            .write_all(record)
            .map_err(|error| x11_peer_write_error("failed to write private control", error))?;
    }
    stream
        .flush()
        .map_err(|error| x11_peer_write_error("failed to flush private control", error))?;
    operation.emission = PrivateControlEmission::Flushed;
    Ok(())
}
