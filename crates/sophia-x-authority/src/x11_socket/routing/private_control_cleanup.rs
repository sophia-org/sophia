/// A control's original execution and unresolved output, kept by its already
/// reserved completion record before the first native effect.
#[cfg(unix)]
struct PrivateControlExecution {
    source: Arc<PrivateControlClientSource>,
    window: XResourceId,
    records: Vec<Vec<u8>>,
    emission: PrivateControlEmission,
    peer_generation_begun: bool,
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
impl ControlCompletionRegistry {
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
        if !matches!(record.phase, ControlPhase::Applying(command) if command.client == source.endpoint.client)
            || record.source.is_some()
        {
            return Err(Refusal::ForeignSource);
        }
        let execution = Arc::new(Mutex::new(PrivateControlExecution {
            source: source.clone(),
            window,
            records: Vec::new(),
            emission: PrivateControlEmission::NotStarted,
            peer_generation_begun: false,
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
        let (token, execution) = {
            let held = self.inner.lock().map_err(|_| Refusal::Unavailable)?;
            if held.records.is_empty() {
                return Ok(false);
            }
            let index = *cursor % held.records.len();
            *cursor = (index + 1) % held.records.len();
            let record = &held.records[index];
            let ControlPhase::Abandoned(command) = record.phase else {
                return Ok(false);
            };
            if record.dependents != 0 {
                return Ok(false);
            }
            // First supported reconciliation: a Configure interrupted after
            // runtime application, before projection or peer/output generation.
            if command.command.kind() != XAuthorityControlKind::ConfigureSurface
                || record.steps.runtime != ControlStepState::Completed
                || record.steps.projection != ControlStepState::NotStarted
            {
                return Err(Refusal::UnsupportedProgress.into());
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
                || operation.peer_generation_begun
            {
                return Err(Refusal::ForeignSource.into());
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
            let _terminated = PrivateRecipientTermination::from_place(
                service,
                origin,
                collected,
                custody,
                &source.endpoint,
            )?;
            // This is the original connection's projection, not a lookup by a
            // reissued client number or a later namespace's apparent absence.
            connection
                .selections
                .lock()
                .map_err(|_| Refusal::Unavailable)?
                .remove(operation.window);
        }
        let mut held = self.inner.lock().map_err(|_| Refusal::Unavailable)?;
        let Some(index) = held.records.iter().position(|record| {
            record.token == token
                && matches!(record.phase, ControlPhase::Abandoned(_))
                && record.dependents == 0
                && record
                    .source
                    .as_ref()
                    .is_some_and(|source| Arc::ptr_eq(source, &execution))
        }) else {
            return Ok(false);
        };
        let removed = held.records.remove(index);
        drop(held);
        drop(removed);
        Ok(true)
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
                    &mut cursor.custody,
                )
                .map(|retired| PrivateTerminalVisit::Control { retired })
        }
    }
}

#[cfg(unix)]
fn write_private_control_records(
    execution: &Arc<Mutex<PrivateControlExecution>>,
    stream: &Arc<Mutex<UnixStream>>,
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
