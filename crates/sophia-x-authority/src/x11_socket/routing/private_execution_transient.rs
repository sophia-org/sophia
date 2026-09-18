// Motion and scroll own request effects, not common holds. The source installs
// an immutable emission before changing its mapper or query projection.

#[cfg(unix)]
fn resolve_and_apply_transient(
    permit: &mut sophia_input_authority::ExecutionPermit<'_>,
    bindings: &PrivateAdmissionBindings,
    registry: &XServerFrontendRouteRegistry,
    route: &XAuthorityRoutedInput,
    grant: sophia_input_authority::GrantId,
    native: &private_native::Owner,
    transients: &mut PrivateTransientInventory,
    pending_custody: &mut Option<PrivateDeliveryCustody>,
    next_event_order: &mut u64,
    notes: &mut PrivateTransactionNotes<'_>,
) -> Result<(), sophia_input_authority::RegistrationError> {
    use sophia_input_authority::{CapacityError, RegistrationError as Error};
    if transients.records.len() == transients.records.capacity() {
        notes.records_exhausted = true;
        return Err(Error::Capacity(CapacityError::NoCompletionCell));
    }
    let clients = registry
        .clients
        .lock()
        .map_err(|_| Error::RoutingUnavailable)?;
    let surfaces = registry
        .surfaces
        .lock()
        .map_err(|_| Error::RoutingUnavailable)?;
    let surface = surfaces
        .get(&route.request.target_surface)
        .copied()
        .ok_or(Error::RoutingUnavailable)?;
    let mut guards = native.lock_base().map_err(|cause| {
        notes.native_refusal = Some(cause);
        Error::RoutingUnavailable
    })?;
    if notes.defer_freeze(guards.freeze(bindings, &clients, notes.freeze_witness(), false))? {
        return Ok(());
    }
    prepare_key_custody(
        registry,
        route,
        pending_custody,
        next_event_order,
        notes,
    )?;
    notes
        .watched
        .applying()
        .map_err(|_| Error::StaleExecution)?;
    guards
        .transient(
            permit,
            route,
            surface,
            &mut transients.pending,
            notes.may_have_applied,
            |recipient| {
                let binding = bindings
                    .bound
                    .get(&recipient)
                    .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
                registry.applied_client(&clients, recipient, binding)
            },
        )
        .map_err(|cause| {
            notes.native_refusal = Some(cause);
            // This unbound custody has no effect or output to retain. Source
            // custody, once installed, is never discarded on refusal.
            if transients.pending.is_none() && !notes.may_have_applied.get() {
                *pending_custody = None;
            }
            Error::RoutingUnavailable
        })?;
    notes
        .watched
        .committed()
        .map_err(|_| Error::StaleExecution)?;
    let (client, window, event) = transients
        .pending
        .as_ref()
        .expect("source installed before application")
        .reached();
    let reached = PrivateReachedResources {
        client,
        window,
        surface: surfaces.iter().find_map(|(id, candidate)| {
            (candidate.namespace == surface.namespace
                && candidate.client == client
                && candidate.window == window)
                .then_some(*id)
        }),
        namespace: surface.namespace,
        seat: route.request.seat,
        grant,
    };
    // Both destination slots were reserved before producer exposure. No
    // fallible operation separates taking the source from installing it.
    transients.records.push(PrivateTransientRecord {
        source: transients.pending.take().expect("source committed"),
        custody: pending_custody
            .take()
            .expect("custody installed before effect"),
    });
    notes.decided = Some(PrivateOrderedDecision {
        owes_event: true,
        reached: Some(reached),
        first_press: false,
        keyboard_applied: false,
        release: None,
        event: Some(XAuthorityInputEvent::Pointer(event)),
    });
    Ok(())
}
