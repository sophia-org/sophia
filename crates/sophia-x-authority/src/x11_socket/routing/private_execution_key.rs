// Keyboard transactions borrow the runner's one history and the inventory's
// installed custody. Native selection, immutable emissions and cleanup proofs
// remain the source's; this adapter only moves their owned records.

#[cfg(unix)]
fn prepare_key_custody(
    registry: &XServerFrontendRouteRegistry,
    route: &XAuthorityRoutedInput,
    pending: &mut Option<PrivateDeliveryCustody>,
    next_order: &mut u64,
    notes: &mut PrivateTransactionNotes<'_>,
) -> Result<(), sophia_input_authority::RegistrationError> {
    use sophia_input_authority::RegistrationError::StaleRequest;
    if pending.is_some() {
        notes.custody_retained = true;
        return Err(StaleRequest);
    }
    let cell = if route.mode == XAuthorityRoutedInputMode::StateOnly {
        // Reserved before the effect, with no invented delivery identity or
        // writer cell. The source records its explicit suppression disposition.
        None
    } else {
        let Some(delivery) = route.delivery else {
            notes.completion_missing = true;
            return Err(StaleRequest);
        };
        match registry.input_recovery.completion_for(delivery) {
            Ok(Some(cell)) => Some(cell),
            Ok(None) => {
                notes.completion_missing = true;
                return Err(StaleRequest);
            }
            Err(PrivateCompletionUnreadable) => {
                notes.recovery_unavailable = true;
                return Err(StaleRequest);
            }
        }
    };
    let Some(next) = next_order.checked_add(1) else {
        notes.order_exhausted = true;
        return Err(StaleRequest);
    };
    *pending = Some(PrivateDeliveryCustody::new(*next_order, cell));
    *next_order = next;
    Ok(())
}

#[cfg(unix)]
#[expect(
    clippy::too_many_arguments,
    reason = "borrow the executor's owned slots through the common transaction"
)]
fn resolve_and_apply_key(
    permit: &mut sophia_input_authority::ExecutionPermit<'_>,
    bindings: &PrivateAdmissionBindings,
    registry: &XServerFrontendRouteRegistry,
    holds: &mut Vec<PrivateHoldRecord>,
    settling: &mut Vec<PrivateSettlingRelease>,
    route: &XAuthorityRoutedInput,
    grant: sophia_input_authority::GrantId,
    capability: sophia_input_authority::DeviceCapability,
    native: &private_native::Owner,
    native_pending: &mut PrivateNativePending,
    pending_custody: &mut Option<PrivateDeliveryCustody>,
    next_event_order: &mut u64,
    keyboards: &mut PrivateKeyboards,
    notes: &mut PrivateTransactionNotes<'_>,
) -> Result<(), sophia_input_authority::RegistrationError> {
    use sophia_input_authority::{
        CapacityError, Input, RegistrationError as Error, ReleaseOutcome,
    };
    let InputEventKind::Key { keycode, pressed } = route.request.kind else {
        return Err(Error::StaleExecution);
    };
    let key = PrivateKeyboards::x_keycode(keycode).ok_or(Error::StaleExecution)?;
    let input = Input::key(key).map_err(|_| Error::StaleExecution)?;
    // Freeze contributors are resolved from these held admission/client rows;
    // the same native guard is retained from eligibility through the effect.
    let clients = registry.clients.lock().map_err(|_| Error::RoutingUnavailable)?;
    let index = holds.iter().position(|record| {
        record
            .native
            .as_ref()
            .is_some_and(|hold| hold.input() == input)
    });

    if !pressed {
        // Reserve the destination before a release can end the aggregate.
        if settling.len() >= PRIVATE_HOLD_RECORDS {
            notes.records_exhausted = true;
            return Err(Error::Capacity(CapacityError::NoCompletionCell));
        }
        let Some(index) = index else {
            let guards = native.lock_base().map_err(|cause| {
                notes.native_refusal = Some(cause);
                Error::RoutingUnavailable
            })?;
            if notes.defer_freeze(guards.freeze(bindings, &clients, notes.freeze_witness(), true))? {
                return Ok(());
            }
            notes
                .watched
                .applying()
                .map_err(|_| Error::StaleExecution)?;
            notes.may_have_applied.set(true);
            let outcome = permit.release(input)?;
            notes
                .watched
                .committed()
                .map_err(|_| Error::StaleExecution)?;
            if matches!(outcome, ReleaseOutcome::DeliverTo(_)) {
                notes.plan_missing = true;
                return Err(Error::StaleRequest);
            }
            notes.decided = Some(PrivateOrderedDecision {
                owes_event: false,
                reached: None,
                first_press: false,
                keyboard_applied: false,
                release: Some(outcome),
                event: None,
            });
            return Ok(());
        };
        let connection = holds[index]
            .native
            .as_ref()
            .expect("selected native hold")
            .connection();
        let mut guards = native.lock_for_release(&connection).map_err(|cause| {
            notes.native_refusal = Some(cause);
            Error::RoutingUnavailable
        })?;
        if notes.defer_freeze(guards.freeze(bindings, &clients, notes.freeze_witness(), true))? {
            return Ok(());
        }
        prepare_key_custody(registry, route, pending_custody, next_event_order, notes)?;
        notes
            .watched
            .applying()
            .map_err(|_| Error::StaleExecution)?;
        let hold = holds[index]
            .native
            .as_mut()
            .and_then(PrivateNativeHold::key_mut)
            .expect("a key obligation");
        let (outcome, built) = guards
            .release_key(permit, hold, route, keyboards, notes.may_have_applied)
            .map_err(|cause| {
                notes.native_refusal = Some(cause);
                Error::RoutingUnavailable
            })?;
        let keyboard_applied = hold.release_xkb_applied();
        let disposition = hold.release_disposition();
        notes
            .watched
            .committed()
            .map_err(|_| Error::StaleExecution)?;
        drop(guards);
        let ReleaseOutcome::DeliverTo(incarnation) = outcome else {
            // A survivor is still physically down. Dispose only the unused
            // delivery custody; the original history and native hold remain.
            *pending_custody = None;
            notes.decided = Some(PrivateOrderedDecision {
                owes_event: false,
                reached: None,
                first_press: false,
                keyboard_applied,
                release: Some(outcome),
                event: None,
            });
            return Ok(());
        };
        if holds[index].incarnation != incarnation {
            notes.plan_missing = true;
            return Err(Error::StaleRequest);
        }
        let reached = holds[index].reached;
        let (event, unbuilt) = match built {
            Ok(event) => (event.map(XAuthorityInputEvent::Key), None),
            Err(cause) => (None, Some(cause)),
        };
        let binding = if disposition
            == private_native::KeyReleaseDisposition::RecipientTerminationRequired
        {
            PrivateReleaseBinding::RecipientTerminationRequired
        } else {
            match registry.input_recovery.bind(route.delivery, reached.client) {
                Ok(true) => PrivateReleaseBinding::Reached,
                Ok(false) => PrivateReleaseBinding::Ended,
                Err(_) => {
                    notes.recovery_unavailable = true;
                    PrivateReleaseBinding::Unknown
                }
            }
        };
        // No fallible call remains between removing the hold and installing
        // its native context and both event obligations in reserved storage.
        let removed = holds.remove(index);
        settling.push(PrivateSettlingRelease {
            incarnation,
            reached,
            custody: pending_custody.take().expect("installed before release"),
            press_custody: Some(removed.custody),
            native: removed.native,
            unbuilt,
            native_recorded: false,
            native_failure: None,
            native_attempts: 0,
            outcome,
            event,
            binding,
            delivery: route.delivery,
        });
        notes.decided = Some(PrivateOrderedDecision {
            owes_event: binding == PrivateReleaseBinding::Reached,
            reached: Some(reached),
            first_press: false,
            keyboard_applied,
            release: Some(outcome),
            event: (binding == PrivateReleaseBinding::Reached)
                .then_some(event)
                .flatten(),
        });
        return Ok(());
    }

    if let Some(index) = index {
        let record = &holds[index];
        let hold = record
            .native
            .as_ref()
            .and_then(PrivateNativeHold::key)
            .expect("a key obligation");
        let connection = hold.connection();
        let mut guards = native.lock_for_release(&connection).map_err(|cause| {
            notes.native_refusal = Some(cause);
            Error::RoutingUnavailable
        })?;
        if notes.defer_freeze(guards.freeze(bindings, &clients, notes.freeze_witness(), true))? {
            return Ok(());
        }
        match registry.input_recovery.bind(route.delivery, hold.client()) {
            Ok(true) => {}
            Ok(false) => {
                notes.delivery_ended = true;
                return Err(Error::StaleRequest);
            }
            Err(_) => {
                notes.recovery_unavailable = true;
                return Err(Error::StaleRequest);
            }
        }
        notes
            .watched
            .applying()
            .map_err(|_| Error::StaleExecution)?;
        guards
            .join_key(permit, hold, keyboards, notes.may_have_applied)
            .map_err(|cause| {
                notes.native_refusal = Some(cause);
                Error::RoutingUnavailable
            })?;
        notes
            .watched
            .committed()
            .map_err(|_| Error::StaleExecution)?;
        notes.decided = Some(PrivateOrderedDecision {
            owes_event: false,
            reached: Some(record.reached),
            first_press: false,
            keyboard_applied: false,
            release: None,
            event: None,
        });
        return Ok(());
    }

    if holds.len() >= PRIVATE_HOLD_RECORDS {
        notes.records_exhausted = true;
        return Err(Error::Capacity(CapacityError::NoGrantSlot));
    }
    // Keep the rank common -> bindings -> clients -> surfaces -> native
    // base -> exact selections -> publication. The source selects the window
    // once and captures its surface from this held map before applying.
    let surfaces = registry
        .surfaces
        .lock()
        .map_err(|_| Error::RoutingUnavailable)?;
    let mut guards = native.lock_base().map_err(|cause| {
        notes.native_refusal = Some(cause);
        Error::RoutingUnavailable
    })?;
    if notes.defer_freeze(guards.freeze(bindings, &clients, notes.freeze_witness(), true))? {
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
    let result = guards.press_key(
        permit,
        capability,
        route,
        &surfaces,
        keyboards,
        native_pending.key_slot().map_err(|cause| {
            notes.native_refusal = Some(cause);
            Error::RoutingUnavailable
        })?,
        notes.may_have_applied,
        |recipient| {
            let binding = bindings
                .bound
                .get(&recipient)
                .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
            registry.applied_client(&clients, recipient, binding)
        },
    );
    let (applied, event) = result.map_err(|cause| {
        // Only a normal, proved pre-effect refusal disposes the unused
        // custody. An interrupted/source-installed context stays owned.
        if !notes.may_have_applied.get() && native_pending.is_none() {
            *pending_custody = None;
        }
        notes.native_refusal = Some(cause);
        Error::StaleExecution
    })?;
    notes
        .watched
        .committed()
        .map_err(|_| Error::StaleExecution)?;
    if !applied.first_press() {
        // The source retained its unexpected obligation in pending. Do not
        // discard it or pretend the new press is an ordinary join.
        notes.plan_missing = true;
        return Err(Error::StaleRequest);
    }
    let held = native_pending
        .key()
        .expect("the source installed its key context");
    let reached = PrivateReachedResources {
        client: held.client(),
        window: held.delivered_window(),
        surface: held.reached_surface(),
        namespace: held.namespace(),
        seat: route.request.seat,
        grant,
    };
    holds.push(PrivateHoldRecord {
        incarnation: applied.incarnation(),
        reached,
        custody: pending_custody.take().expect("installed before press"),
        native: None,
    });
    holds
        .last_mut()
        .expect("reserved destination installed")
        .native = native_pending.take();
    notes.decided = Some(PrivateOrderedDecision {
        owes_event: true,
        reached: Some(reached),
        first_press: true,
        keyboard_applied: true,
        release: None,
        event: event.map(XAuthorityInputEvent::Key),
    });
    Ok(())
}
