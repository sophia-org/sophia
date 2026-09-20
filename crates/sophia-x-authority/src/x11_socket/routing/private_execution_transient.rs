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
    // Motion over the bare root names no registered surface, so its recipient
    // is found rather than looked up. Everything else names one and must.
    let over_root = route.request.target_surface == crate::ROOT_POINTER_SURFACE;
    let surface = if over_root {
        None
    } else {
        Some(
            surfaces
                .get(&route.request.target_surface)
                .copied()
                .ok_or(Error::RoutingUnavailable)?,
        )
    };
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
    let surface = match surface {
        Some(surface) => surface,
        None => match root_motion_recipient(&guards, bindings, &clients, route) {
            Some(surface) => surface,
            None => {
                // Nobody selected it. The pointer still moved, and the
                // query state says where it is; that is the whole effect,
                // and the delivery custody reserved for an event nobody is
                // owed is disposed of unused.
                guards.observe_root_motion(permit, route).map_err(|cause| {
                    notes.native_refusal = Some(cause);
                    Error::RoutingUnavailable
                })?;
                notes
                    .watched
                    .committed()
                    .map_err(|_| Error::StaleExecution)?;
                *pending_custody = None;
                notes.decided = Some(PrivateOrderedDecision {
                    owes_event: false,
                    reached: None,
                    first_press: false,
                    keyboard_applied: false,
                    release: None,
                    event: None,
                });
                return Ok(());
            }
        },
    };
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

/// Who receives motion over the bare root, resolved the way the applied view
/// will resolve it again under the same guards.
///
/// The reference delivers root motion to the deepest mapped window under the
/// point whose ancestry selects it, and to a pointer grab's owner before any
/// of that. A registered surface carries its client; the root carries none,
/// so the client is found by asking each admitted connection's own selection
/// projection the question the view asks. The first that answers is the
/// recipient, and the window handed on is the root: the view walks down from
/// it to the same child, so the answer here is a client, not a resolution.
#[cfg(unix)]
fn root_motion_recipient(
    guards: &private_native::BaseGuards<'_>,
    bindings: &PrivateAdmissionBindings,
    clients: &BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>,
    route: &XAuthorityRoutedInput,
) -> Option<XServerFrontendSurfaceRoute> {
    let namespace = guards.namespace();
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let (event_x, event_y) = (
        clamp_input_coordinate(route.request.local_position.x),
        clamp_input_coordinate(route.request.local_position.y),
    );
    let mut candidates = bindings.bound.keys().copied().filter_map(|client| {
        let senders = clients.get(&client)?;
        let state = senders.connection_state.get()?;
        (state.namespace == namespace).then_some((client, senders.admission, state))
    });
    let selected = if let Some(grab) = guards.pointer_grab() {
        // A grab decides the recipient outright; the view will route to the
        // grab window itself. Only the client's admission is needed here.
        let owner = XServerFrontendClientId::from_raw(grab.owner);
        candidates.find(|(client, _, _)| *client == owner)
    } else {
        candidates.find(|(_, _, state)| {
            let Ok(selections) = state.selections.lock() else {
                return false;
            };
            let budget = PrivateTraversalBudget::new();
            let Ok(target) =
                selections.ordered_pointer_target_budget(root, event_x, event_y, &budget)
            else {
                return false;
            };
            let Ok(ancestry) = selections.ordered_ancestry_budget(target, &budget) else {
                return false;
            };
            selections
                .ordered_pointer_selection(&ancestry, root, 1 << 6)
                .is_some()
        })
    };
    selected.map(|(client, admission, _)| XServerFrontendSurfaceRoute {
        client,
        namespace,
        admission,
        window: root,
    })
}
