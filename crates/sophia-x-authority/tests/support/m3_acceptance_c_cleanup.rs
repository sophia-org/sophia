// Control cleanup and the indeterminate send. Included from
// m3_acceptance.rs beside m3_acceptance_c.rs (t026).

/// The control source this connection's own registration published.
fn control_source_of(custody: &Arc<PrivateEvidenceCustody>) -> Arc<PrivateControlClientSource> {
    // WAITED FOR ON THIS EXACT CUSTODY. A newly minted custody is not a
    // finished attachment: the registration and the control source it
    // publishes are applied afterwards, so reading them the instant the
    // connection is accepted races the setup that produces them. Nothing here
    // substitutes a source or infers one; it waits for this connection's own.
    waited_for_value(|| {
        custody
            .cleanup_record()
            .connection_state
            .get()?
            .control_source
            .get()?
            .upgrade()
    })
    .expect("this connection's registration published its own control source")
}

/// The command this row submits for one kind, with the exact values its first
/// effect is afterwards read back by.
fn control_command_for(
    kind: XAuthorityControlKind,
    surface: SurfaceId,
) -> XAuthorityControlCommand {
    use XAuthorityControlCommand as Command;
    use XAuthorityControlKind as Kind;
    let transaction = TransactionId::from_raw(99882);
    let geometry = Rect {
        x: 2,
        y: 3,
        width: 80,
        height: 60,
    };
    let state = sophia_protocol::PolicyPresentationState {
        fullscreen: true,
        maximized: false,
        minimized: false,
    };
    match kind {
        Kind::PublishMetadataRule => Command::PublishMetadataRule {
            transaction,
            surface,
            rule: sophia_protocol::MetadataDisclosureRule {
                surface,
                disclosure: sophia_protocol::MetadataDisclosure::None,
                trust_level: sophia_protocol::TrustLevel::Unknown,
                icon: None,
                generation: 37,
            },
        },
        Kind::AdmitSurface => Command::AdmitSurface {
            transaction,
            surface,
            geometry,
        },
        Kind::ConfigureSurface => Command::ConfigureSurface {
            transaction,
            surface,
            geometry,
        },
        Kind::SetPresentationState => Command::SetPresentationState {
            transaction,
            surface,
            state,
        },
        Kind::RestorePresentationState => Command::RestorePresentationState {
            transaction,
            surface,
            state,
        },
        Kind::FocusSurface => Command::FocusSurface {
            transaction,
            surface,
        },
        Kind::ClearFocus => Command::ClearFocus {
            transaction,
            surface,
        },
        Kind::WithdrawSurface => Command::WithdrawSurface {
            transaction,
            surface,
        },
        Kind::CloseSurface => Command::CloseSurface {
            transaction,
            surface,
        },
    }
}

/// Wait out an allowance that names a delay.
///
/// Returns the refusal that cannot be waited out when one is met, so a caller
/// fails on it rather than spinning against it.
fn wait_out_allowance(
    refusal: Option<sophia_input_authority::ServiceStartRefusal>,
) -> Option<String> {
    use sophia_input_authority::ServiceStartRefusal as Refusal;
    match refusal {
        None => None,
        Some(
            Refusal::StartsExhausted { retry_after }
            | Refusal::TimeExhausted { retry_after }
            | Refusal::CleanupStartsReserved { retry_after }
            | Refusal::CleanupTimeReserved { retry_after },
        ) => {
            std::thread::sleep(retry_after.min(Duration::from_millis(50)));
            None
        }
        Some(other @ (Refusal::ClockRegressed | Refusal::Interrupted)) => {
            Some(format!("{other:?}"))
        }
    }
}

/// One window's presentation properties, by the atoms that carry them.
///
/// A COUNT IS NOT A WITNESS. "Some properties exist" is true of a window that
/// was never touched; what a presentation control does is create `WM_STATE` and
/// `_NET_WM_STATE` and put the state it was given into the latter, so that is
/// what is read, before and after.
struct PresentationState {
    wm_state: Option<(Vec<u8>, u8, crate::XAtom)>,
    net_wm_state: Option<(Vec<u8>, u8, crate::XAtom)>,
}

fn presentation_state_of(
    source: &PrivateControlClientSource,
    resource: XResourceId,
) -> PresentationState {
    let named = |name: &str| {
        source
            .state
            .atoms
            .lock()
            .expect("a readable atom table")
            .intern(name, true)
            .ok()
            .flatten()
    };
    let wm_state = named(crate::X_ATOM_NAME_WM_STATE);
    let net_wm_state = named(crate::X_ATOM_NAME_NET_WM_STATE);
    let properties = source
        .state
        .properties
        .lock()
        .expect("a readable property table");
    let read = |atom: Option<crate::XAtom>| {
        atom.and_then(|atom| properties.get(source.endpoint.namespace, resource, atom))
            .map(|held| (held.bytes.clone(), held.format, held.property_type))
    };
    PresentationState {
        wm_state: read(wm_state),
        net_wm_state: read(net_wm_state),
    }
}

/// These bytes, as the atoms they are, in the order this fixture's recipient
/// negotiated.
///
/// LITTLE-ENDIAN, BECAUSE THAT IS WHAT THE HANDSHAKE AGREED. Accepting either
/// order would accept a value encoded the wrong way round, which is precisely
/// what a recipient could not read.
fn atoms_of(bytes: &[u8]) -> Vec<crate::XAtom> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")))
        .collect()
}

/// What this kind actually changed at the source before the writer was
/// interrupted, read back from the source's own tables and state.
///
/// ONE WITNESS PER KIND, AND THE KIND'S OWN. "A record is owed" is the same
/// sentence for all nine; what tells them apart is the thing each one did, and
/// a row that did not read that has not established the kind it names.
fn first_effect_of(
    kind: XAuthorityControlKind,
    source: &PrivateControlClientSource,
    surface: SurfaceId,
    resource: XResourceId,
) -> Value {
    use XAuthorityControlKind as Kind;
    match kind {
        Kind::PublishMetadataRule => {
            let generation = source
                .tables
                .rules
                .lock()
                .expect("a readable rule table")
                .get(&surface)
                .expect("the rule this control inserted")
                .generation;
            assert_eq!(generation, 37, "the rule inserted is the one submitted");
            assert!(
                !source
                    .tables
                    .generations
                    .lock()
                    .expect("a readable generation table")
                    .contains_key(&surface),
                "and the interruption stopped before the generation was recorded"
            );
            json!({"rule_generation_inserted": generation, "surface_generation_recorded": false})
        }
        Kind::AdmitSurface | Kind::ConfigureSurface => {
            let runtime = source
                .state
                .runtime
                .lock()
                .expect("readable runtime state")
                .window_geometry(source.endpoint.namespace, resource)
                .expect("the geometry this control applied")
                .width;
            let selected = source
                .endpoint
                .registration
                .get()
                .expect("this connection's registration")
                .selections
                .lock()
                .expect("readable selections")
                .geometry(resource)
                .expect("the geometry its own selection still holds")
                .width;
            assert_eq!(runtime, 80, "the runtime geometry is the one submitted");
            assert_eq!(
                selected, 8,
                "and the recipient's own selection still holds what it had"
            );
            json!({"runtime_width": runtime, "selection_width": selected})
        }
        Kind::SetPresentationState | Kind::RestorePresentationState => {
            let held = presentation_state_of(source, resource);
            let (wm_bytes, wm_format, wm_type) = held
                .wm_state
                .expect("the first presentation state created this window's WM_STATE");
            let (net_bytes, net_format, net_type) =
                held.net_wm_state.expect("and its _NET_WM_STATE");
            let named = |name: &str| {
                source
                    .state
                    .atoms
                    .lock()
                    .expect("a readable atom table")
                    .intern(name, true)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| panic!("the atom {name} this control interned"))
            };
            assert_eq!(wm_format, 32, "WM_STATE is a pair of 32-bit values");
            assert_eq!(
                wm_type,
                named(crate::X_ATOM_NAME_WM_STATE),
                "typed as WM_STATE itself"
            );
            assert_eq!(
                atoms_of(&wm_bytes),
                vec![1, 0],
                "holding the normal state and no icon window: {wm_bytes:?}"
            );
            assert_eq!(net_format, 32, "and _NET_WM_STATE is a list of atoms");
            assert_eq!(net_type, crate::X_ATOM_ATOM, "typed as a list of atoms");
            assert_eq!(
                atoms_of(&net_bytes),
                vec![named(crate::X_ATOM_NAME_NET_WM_STATE_FULLSCREEN)],
                "holding exactly the one state submitted: {net_bytes:?}"
            );
            json!({
                "wm_state_bytes": wm_bytes,
                "wm_state_format": wm_format,
                "net_wm_state_bytes": net_bytes,
                "net_wm_state_format": net_format,
                "net_wm_state_atoms": atoms_of(&net_bytes),
            })
        }
        Kind::FocusSurface => {
            let focus = source
                .state
                .runtime
                .lock()
                .expect("readable runtime state")
                .input_focus(source.endpoint.namespace)
                .0;
            assert_eq!(focus, resource, "focus actually moved to this window");
            json!({"input_focus": format!("{focus:?}")})
        }
        Kind::ClearFocus => {
            let focus = source
                .state
                .runtime
                .lock()
                .expect("readable runtime state")
                .input_focus(source.endpoint.namespace)
                .0
                .local
                .raw();
            assert_eq!(
                focus,
                u64::from(X_SETUP_DEFAULT_ROOT),
                "focus actually returned to the root"
            );
            json!({"input_focus_local": focus})
        }
        Kind::WithdrawSurface => {
            let mapped = source
                .state
                .runtime
                .lock()
                .expect("readable runtime state")
                .window_map_state(source.endpoint.namespace, resource)
                .expect("this window's map state");
            assert_eq!(
                mapped,
                crate::XMapState::Unmapped,
                "the window was actually unmapped"
            );
            json!({"map_state": format!("{mapped:?}")})
        }
        Kind::CloseSurface => {
            assert!(
                waited_for(|| source.teardown.lock().unwrap().finished),
                "the owned shutdown actually reached this source's teardown"
            );
            json!({"owned_shutdown_reached_teardown": true})
        }
    }
}

/// One charged terminal visit, with what the store held either side of it.
#[derive(Clone, Debug)]
struct ControlVisit {
    at: usize,
    visit: String,
    retired_here: bool,
    state_before: ControlRecordState,
    state_after: ControlRecordState,
    credit_before: Option<usize>,
    credit_after: Option<usize>,
}

/// One control kind, from its actual first source effect through the separate
/// visits that retire its record and return its credit.
///
/// A CLEANUP IS NOT ONE EVENT. A charged control visit removes the record once
/// the source's own removal is available; a LATER control visit returns the one
/// credit that record carried. Asserting only the end state would let a single
/// visit that did both pass for the sequence production actually performs, and
/// would not notice a credit returned for a record still outstanding.
fn control_cleanup_for_kind(
    label: &'static str,
    kind: XAuthorityControlKind,
    namespace: u64,
    window: u32,
) -> (Value, Vec<String>) {
    let store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let mut service =
        LifecycleService::launch_over_store(label, namespace, None, false, 1, store.clone());
    service.start();
    let (mut peer, custody) = service.connect();
    let source = control_source_of(&custody);
    if kind == XAuthorityControlKind::AdmitSurface {
        source
            .state
            .set_policy_map_deferred(true)
            .expect("the source accepts a deferred map policy");
    }
    let (surface, _sequence) = selecting_window(&mut peer, &service.transactions, window, 0);
    let resource = XResourceId::new(u64::from(window), 1);
    let lease = service.owner.lease();
    let control = service
        .access
        .control_producer(&lease)
        .expect("the service's own control producer");
    if kind == XAuthorityControlKind::ClearFocus {
        // A REAL NON-ROOT FOCUS FIRST, so the clear below actually changes
        // something. Clearing a focus that was already root would establish
        // nothing about this kind.
        control
            .submit(
                &lease,
                XAuthorityClientControlCommand {
                    client: source.endpoint.client,
                    command: XAuthorityControlCommand::FocusSurface {
                        transaction: TransactionId::from_raw(99880),
                        surface,
                    },
                },
            )
            .expect("the order accepts the preparing focus");
        assert_eq!(
            ack_for(&service.acks, 99880)
                .expect("the writer published the preparing focus outcome")
                .acknowledgement
                .outcome,
            XAuthorityControlOutcome::Delivered
        );
        assert_eq!(
            source
                .state
                .runtime
                .lock()
                .expect("readable runtime state")
                .input_focus(source.endpoint.namespace)
                .0,
            resource,
            "the focus this clear is going to move actually moved here first"
        );
    }
    // WHAT THIS WINDOW HAD BEFORE, so the change below is a change. A
    // presentation control creates both of these; a window nobody has set a
    // state on has neither.
    let before = presentation_state_of(&source, resource);
    let (wm_state_before, net_wm_state_before) =
        (before.wm_state.is_some(), before.net_wm_state.is_some());
    if matches!(
        kind,
        XAuthorityControlKind::SetPresentationState
            | XAuthorityControlKind::RestorePresentationState
    ) {
        assert!(
            !wm_state_before && !net_wm_state_before,
            "{label}: this window carried no presentation state before the control"
        );
    }
    // INTERRUPTED AFTER ITS FIRST SOURCE EFFECT, which is the state the row is
    // about: something at the source changed and nothing answered for it.
    source.fail_after_effect.store(true, Ordering::Release);
    // THE WHOLE COMMAND IS KEPT, so the record below is compared against what
    // was actually submitted rather than against its kind alone.
    let submitted = XAuthorityClientControlCommand {
        client: source.endpoint.client,
        command: control_command_for(kind, surface),
    };
    control
        .submit(&lease, submitted)
        .expect("the order accepts this kind");
    let completion = service
        .registry
        .control_completion()
        .expect("this origin's own control completion registry");
    let cleanup = waited_for_value(|| {
        completion
            .cleanups_owed()
            .ok()
            .and_then(|owed| owed.into_iter().next())
    })
    .expect("the actual writer was interrupted after its first source effect");
    assert_eq!(
        cleanup.command, submitted,
        "the record owed names the exact command submitted: its client, its transaction, its surface and its payload"
    );
    assert_eq!(
        cleanup.command.command.kind(),
        kind,
        "which is this kind's own"
    );
    let execution = completion
        .execution_of(cleanup.token)
        .expect("the execution that record belongs to");
    {
        let held = execution.lock().expect("a readable execution");
        assert_eq!(
            held.token, cleanup.token,
            "the execution is the one this record was issued for"
        );
        assert!(
            Arc::ptr_eq(&held.source, &source),
            "against this connection's own control source"
        );
        assert!(
            held.source.endpoint.matches(&source.endpoint),
            "naming this connection's own endpoint"
        );
        assert_eq!(held.surface, surface, "and this surface");
        assert_eq!(held.window, resource, "and this window");
        assert!(
            !held.peer_generation_begun,
            "with no peer generation begun for it"
        );
        assert!(
            held.pending_metadata.is_none(),
            "and nothing left pending at the first-effect boundary"
        );
    }
    assert!(
        service.acks.try_recv().is_err(),
        "and the interruption published no acknowledgement"
    );
    let first_effect = first_effect_of(kind, &source, surface, resource);

    drop(peer);
    assert!(
        waited_for(|| source.teardown.lock().unwrap().finished),
        "the recipient going away reached this source's own teardown"
    );
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();

    // THE SOURCE'S OWN REMOVAL IS WITHHELD. Cleanup may not invent one: while
    // the removal this teardown produced is not available, the record stays
    // outstanding and its credit stays taken, however many charged visits run.
    let removal = source
        .teardown
        .lock()
        .expect("readable teardown")
        .removed
        .take()
        .expect("the source's own removal receipt");
    let destroyed = removal.resources.destroyed_windows.clone();
    assert!(
        destroyed.contains(&resource),
        "which names this window as destroyed: {destroyed:?}"
    );
    let credit_with_record = store.reserved();
    assert_eq!(
        credit_with_record,
        Some(1),
        "{label}: the record this row is about carries exactly one credit"
    );
    let mut withheld_visits = BoundedTrace::default();
    let mut withheld_refusals = 0usize;
    let withheld_deadline = std::time::Instant::now() + Duration::from_secs(15);
    // A CHARGED CONTROL VISIT THAT REFUSED FOR THIS RECORD'S WITHHELD REMOVAL,
    // not merely some charged step. Any charged Output or Terminal visit would
    // pass for one while never reaching this record at all.
    while std::time::Instant::now() < withheld_deadline && withheld_refusals < 3 {
        let visit = service.step();
        if visit.charged
            && visit.terminal_refusal
                == Some(PrivateTerminalDriveRefusal::Control(
                    PrivateControlCleanupRefusal::RemovalWithheld,
                ))
        {
            assert!(
                visit.supervision_ok,
                "{label}: that refusal came from a supervised visit: {visit:?}"
            );
            withheld_refusals += 1;
        }
        if let Some(fatal) = wait_out_allowance(visit.allowance_refusal) {
            panic!("{label}: a visit refused for something waiting cannot fix: {fatal}");
        }
        withheld_visits.push(|| format!("{visit:?}"));
    }
    assert!(
        withheld_refusals >= 1,
        "{label}: a charged control visit refused this record for its withheld removal: {withheld_visits:?}"
    );
    assert_eq!(
        completion.state_of(cleanup.token),
        ControlRecordState::Outstanding,
        "{label}: the record is still owed: {withheld_visits:?}"
    );
    // THE SAME EXECUTION, AND THE STORE'S ONE CREDIT IS ITS ONE CREDIT. A
    // count alone would agree with a record replaced by another; this is the
    // execution this row began, still the only one outstanding, against the
    // single credit the store is holding.
    let execution_after = completion
        .execution_of(cleanup.token)
        .unwrap_or_else(|| panic!("{label}: the same execution is still held"));
    assert!(
        Arc::ptr_eq(&execution_after, &execution),
        "{label}: and it is the very execution this row began"
    );
    assert_eq!(
        completion.outstanding(),
        Some(1),
        "{label}: this record is the only one outstanding"
    );
    assert_eq!(
        store.reserved(),
        Some(1),
        "{label}: so the one credit the store holds is its own"
    );

    // AND WITH IT BACK, TWO SEPARATE VISITS. One retires the record; a later
    // one returns the one credit that record carried.
    source.teardown.lock().expect("readable teardown").removed = Some(removal);
    let mut control_visits: Vec<ControlVisit> = Vec::new();
    let mut retired_at: Option<ControlVisit> = None;
    let mut reclaimed_at: Option<ControlVisit> = None;
    let cleanup_deadline = std::time::Instant::now() + Duration::from_secs(20);
    for at in 0..20_000usize {
        if std::time::Instant::now() >= cleanup_deadline {
            break;
        }
        let state_before = completion.state_of(cleanup.token);
        let credit_before = store.reserved();
        let visit = service.step();
        let state_after = completion.state_of(cleanup.token);
        let credit_after = store.reserved();
        let is_control = matches!(
            visit.terminal_visit,
            Some(PrivateTerminalVisit::Control { .. })
        );
        if is_control && visit.charged {
            let seen = ControlVisit {
                at,
                visit: format!("{:?}", visit.terminal_visit),
                retired_here: state_before == ControlRecordState::Outstanding
                    && state_after == ControlRecordState::Retired,
                state_before,
                state_after,
                credit_before,
                credit_after,
            };
            if seen.retired_here && retired_at.is_none() {
                retired_at = Some(seen.clone());
            }
            if retired_at.is_some()
                && reclaimed_at.is_none()
                && credit_before > credit_after
                && credit_after == Some(0)
            {
                reclaimed_at = Some(seen.clone());
            }
            if control_visits.len() < TRACE_BOUND {
                control_visits.push(seen);
            }
        }
        if let Some(fatal) = wait_out_allowance(visit.allowance_refusal) {
            panic!("{label}: a cleanup visit refused for something waiting cannot fix: {fatal}");
        }
        if !visit.supervision_ok && visit.charged {
            panic!("{label}: a charged cleanup visit lost its supervisor: {visit:?}");
        }
        if retired_at.is_some() && reclaimed_at.is_some() {
            break;
        }
    }
    let retired = retired_at.unwrap_or_else(|| {
        panic!("{label}: a charged control visit retired this record: {control_visits:?}")
    });
    let reclaimed = reclaimed_at.unwrap_or_else(|| {
        panic!("{label}: and a later charged control visit returned its credit: {control_visits:?}")
    });
    assert!(
        reclaimed.at > retired.at,
        "{label}: the credit came back on a separate, later visit: retired {retired:?} reclaimed {reclaimed:?}"
    );
    assert_eq!(
        retired.credit_before, retired.credit_after,
        "{label}: the visit that retired the record returned nothing by itself: {retired:?}"
    );
    assert_eq!(
        retired.state_after,
        ControlRecordState::Retired,
        "{label}: and left it retired: {retired:?}"
    );
    assert_eq!(
        reclaimed.state_before,
        ControlRecordState::Retired,
        "{label}: and the visit that returned the credit found the record already retired: {reclaimed:?}"
    );
    assert_eq!(
        reclaimed.state_after,
        ControlRecordState::Retired,
        "{label}: and left it so: {reclaimed:?}"
    );
    assert_eq!(
        completion.state_of(cleanup.token),
        ControlRecordState::Retired,
        "{label}: the record is retired"
    );
    assert_eq!(
        store.reserved(),
        Some(0),
        "{label}: and exactly the credit it carried came back"
    );
    // AND THE SOURCE'S OWN TABLES ARE CLEAR. What cleanup undid is read from
    // the tables it changed, not from the record going away.
    assert!(
        !source
            .tables
            .windows
            .lock()
            .expect("readable window table")
            .contains_key(&surface),
        "{label}: this surface is gone from the source's window table"
    );
    assert!(
        !source
            .tables
            .rules
            .lock()
            .expect("readable rule table")
            .contains_key(&surface),
        "{label}: and from its rules"
    );
    assert!(
        !source
            .tables
            .generations
            .lock()
            .expect("readable generation table")
            .contains_key(&surface),
        "{label}: and from its generations"
    );
    assert!(
        service.acks.try_recv().is_err(),
        "{label}: and cleanup never fabricated an outcome for it"
    );
    let seen = json!({
        "kind": format!("{kind:?}"),
        "window": window,
        "first_effect_at_the_source": first_effect,
        "record_owed_for_this_kind": format!("{:?}", cleanup.command.command.kind()),
        "execution_is_this_connections_source": true,
        "peer_generation_begun": false,
        "acknowledgement_from_the_interruption": Option::<String>::None,
        "removal_receipt_destroyed_windows": destroyed
            .iter()
            .map(|window| format!("{window:?}"))
            .collect::<Vec<_>>(),
        "submitted_command": format!("{submitted:?}"),
        "record_names_the_submitted_command": true,
        "charged_control_refusals_for_this_record_while_withheld": withheld_refusals,
        "same_execution_after_withheld_visits": true,
        "outstanding_records_while_withheld": 1,
        "state_while_removal_withheld": "Outstanding",
        "presentation_state_before_the_control": json!({
            "wm_state": wm_state_before,
            "net_wm_state": net_wm_state_before,
        }),
        "credit_while_removal_withheld": credit_with_record,
        "visit_that_retired_the_record": format!("{retired:?}"),
        "what_the_retiring_visit_was": retired.visit.clone(),
        "visit_that_returned_the_credit": format!("{reclaimed:?}"),
        "what_the_reclaiming_visit_was": reclaimed.visit.clone(),
        "visits_between_them": reclaimed.at - retired.at,
        "control_visits_seen": control_visits
            .iter()
            .map(|seen| format!("{seen:?}"))
            .collect::<Vec<_>>(),
        "credit_after_cleanup": store.reserved(),
        "closed_error": closed.error.clone(),
        "what_this_establishes": "this kind actually changed the source, its writer was interrupted before answering, the record it owes is that kind's own against this connection's own source, no outcome was fabricated, the record and its credit stay held while the source's own removal is withheld, and with the removal back one charged control visit retires the record and a separate later one returns exactly the one credit it carried.",
    });
    let collected = finish_labelled(label, service, &[custody]);
    (seen, collected)
}

/// Actual cleanup of every control kind, on the production service.
///
/// EACH KIND ON ITS OWN INVOCATION, from the thing it actually changed at
/// the source to the separate visits that retire its record and return its
/// credit. A close ends the recipient's connection, so the kinds that must
/// run cannot share one with it.
#[test]
fn c_control_cleanup() {
    let mut actors = Vec::new();
    let mut per_kind: Vec<(&'static str, Value)> = Vec::new();
    // ONE INVOCATION PER KIND. `CloseSurface` ends the recipient's connection,
    // so the kinds that must run cannot share one with it.
    //
    // WHAT THIS ESTABLISHES AND WHAT IT DOES NOT. Each kind here is
    // interrupted after its first source effect, before any peer generation
    // has begun, so nothing below is evidence about healthy-peer reuse, a
    // failed peer, or a replacement recipient. Those are separate questions
    // and separate controls answer them.
    for (index, (label, subcase, kind)) in [
        (
            "c-cleanup-metadata-rule",
            "PublishMetadataRule",
            XAuthorityControlKind::PublishMetadataRule,
        ),
        (
            "c-cleanup-admit",
            "AdmitSurface",
            XAuthorityControlKind::AdmitSurface,
        ),
        (
            "c-cleanup-configure",
            "ConfigureSurface",
            XAuthorityControlKind::ConfigureSurface,
        ),
        (
            "c-cleanup-set-presentation",
            "SetPresentationState",
            XAuthorityControlKind::SetPresentationState,
        ),
        (
            "c-cleanup-restore-presentation",
            "RestorePresentationState",
            XAuthorityControlKind::RestorePresentationState,
        ),
        (
            "c-cleanup-focus",
            "FocusSurface",
            XAuthorityControlKind::FocusSurface,
        ),
        (
            "c-cleanup-clear-focus",
            "ClearFocus",
            XAuthorityControlKind::ClearFocus,
        ),
        (
            "c-cleanup-withdraw",
            "WithdrawSurface",
            XAuthorityControlKind::WithdrawSurface,
        ),
        (
            "c-cleanup-close",
            "CloseSurface",
            XAuthorityControlKind::CloseSurface,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (seen, collected) = control_cleanup_for_kind(
            label,
            kind,
            12_070 + index as u64,
            0x330101 + index as u32 * 0x100,
        );
        per_kind.push((subcase, seen));
        actors.extend(collected);
    }
    assert_eq!(
        per_kind.len(),
        9,
        "every named control kind was driven through its own cleanup"
    );
    emit_case("C.control_cleanup", &per_kind, &actors);
}

/// Transmission whose outcome is uncertain is never sent again, on the
/// production service.
///
/// FOUR STATES, EACH MADE RATHER THAN DESCRIBED. A capsule stopped between
/// its own two frames; a handover admitted and never recorded; work sitting
/// on its recipient's queue; and a decided request whose refusal cannot be
/// published. An earlier version of this control claimed that interrupting
/// the handover leaves the invocation's connection worker unjoined. That was
/// wrong: it was a fixture-ordering error in this file, a third service
/// finished without being stopped first, and the claim is withdrawn.
#[test]
fn c_indeterminate_send() {
    let mut actors = Vec::new();

    // A REFUSED PUBLICATION STAYS OWNED, through two deterministic holds
    // of the original runner: one while its admission is taken away, and
    // one while it is given back, so neither observation races the
    // service's own legitimate retry.
    let (refused, refused_actors) = refused_publication(12050, 0x320701);
    actors.extend(refused_actors);

    // ENQUEUED WORK IS OBSERVED, NOT RESENT, on an invocation of its own.
    let (enqueued, enqueued_actors) = enqueued_observation(12053, 0x320c01);
    actors.extend(enqueued_actors);

    // A HANDOVER BEGUN AND NEVER REPORTED. The capsule left, and the record
    // of what the handover returned never happened, so nothing can say
    // whether the recipient has it. It is not offered again.
    let unknown_store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let mut unknown = LifecycleService::launch_over_store(
        "c-unknown-send",
        12051,
        None,
        false,
        1,
        unknown_store.clone(),
    );
    unknown.start();
    let (mut unknown_peer, unknown_custody) = unknown.connect();
    let (unknown_surface, unknown_sequence, unknown_ingress) =
        focus_window(&unknown, &mut unknown_peer, 0x320a01, 12051);
    let unknown_client = unknown_custody.cleanup_record().client;
    let press = XAuthorityInputDeliveryId::from_raw(12080);
    let release = XAuthorityInputDeliveryId::from_raw(12081);
    unknown_ingress
        .submit(
            &unknown.owner.lease(),
            button_to(unknown_surface, press, 272, true),
        )
        .expect("an actual press");
    assert_eq!(
        read_event(&mut unknown_peer, 3),
        Some(expected_button_event(true, unknown_sequence, 0x320a01, 1))
    );
    assert_eq!(
        receipt_for(&unknown.deliveries, 12080),
        XAuthorityInputDeliveryOutcome::Flushed
    );
    // THE RUNNER IS HELD BEFORE THE RELEASE IS SUBMITTED, so its admission
    // completion can be taken while nothing is able to consume it. Reading
    // it after an unheld submit raced the very handover this case is
    // about.
    let (pause, held_runner) = Pause::pair();
    arm_runner(&unknown.registry, Box::new(move |_, _| pause.wait()));
    let held_on = held_runner.entered();
    unknown_ingress
        .submit(
            &unknown.owner.lease(),
            button_to(unknown_surface, release, 272, false),
        )
        .expect("an actual release, whose handover this case interrupts");
    let original_release_cell = waited_for_value(|| delivery_cell(&unknown.registry, 12081))
        .expect("the release's own completion, minted by its own admission");
    arm_handover(
        &unknown.registry,
        release,
        Box::new(|| panic!("labelled acceptance interruption between handover and its record")),
    );
    held_runner.release();

    let unknown_closed = unknown.closed();
    assert!(
        unknown_closed.unwound,
        "the interruption ended the invocation it happened in"
    );
    // WHAT THE RECIPIENT GOT IS READ, NOT ASSUMED. The capsule had been
    // handed to the writer, but the invocation unwound underneath it, so
    // the bytes may have reached the recipient or the connection may have
    // ended first. Both are honest outcomes of an interruption and
    // neither authorises rebuilding the event; what this case establishes
    // is about the record, not about which of the two happened.
    let release_wire = read_event(&mut unknown_peer, 3);
    let release_cell = delivery_cell(&unknown.registry, 12081).and_then(|cell| cell.answer());
    let phases = retained_dispatch(&unknown);
    // THE RECORD SAYS WHAT HAPPENED TO IT, which is that nobody knows. The
    // handover was begun and its result never written down, and that is the
    // one state this subcase is about.
    assert!(
        phases
            .iter()
            .any(|seen| seen.dispatch == PrivateDispatchPhase::Indeterminate),
        "the retained release says its handover was begun and never reported: {phases:?}"
    );
    // AND IT KEEPS NOTHING TO SEND AGAIN. The capsule left; no replayable
    // copy stayed behind, so nothing could re-offer it even if something
    // decided to.
    assert!(
        phases
            .iter()
            .all(|seen| seen.dispatch != PrivateDispatchPhase::Indeterminate
                || !seen.pending_capsule),
        "and keeps no replayable copy of what it handed over: {phases:?}"
    );
    // THE EXACT RELEASE, BY IDENTITY. Not one release in an indeterminate
    // phase: this connection's own, carrying the completion its own
    // admission minted, with no copy of the capsule left to send again.
    let retained_release = phases
        .iter()
        .find(|seen| seen.delivery == Some(XAuthorityInputDeliveryId::from_raw(12081)))
        .unwrap_or_else(|| panic!("the release this case submitted is retained: {phases:?}"));
    assert_eq!(
        retained_release.dispatch,
        PrivateDispatchPhase::Indeterminate,
        "its handover was begun and never reported: {retained_release:?}"
    );
    assert!(
        retained_release.carries(&original_release_cell),
        "and it still carries the very completion its own admission minted: {retained_release:?}"
    );
    assert!(
        !retained_release.pending_capsule,
        "with nothing kept to send again: {retained_release:?}"
    );
    assert_eq!(
        (
            retained_release.reached_client,
            retained_release.reached_window.local.raw()
        ),
        (unknown_client, u64::from(0x320a01u32)),
        "reaching this connection's own window: {retained_release:?}"
    );
    // AND IT IS THE RELEASE THAT WAS HANDED OVER. The witness is taken at
    // the seam, before the interruption, so this compares the retained
    // record against what the release actually was at that moment rather
    // than against a second reading of the same aftermath.
    let witness = handover_witness()
        .expect("the handover recorded what the release was before it was interrupted");
    assert_eq!(
        retained_release.delivery, witness.delivery,
        "the retained record is the delivery that was handed over: {retained_release:?}"
    );
    assert_eq!(
        retained_release.incarnation, witness.incarnation,
        "with the incarnation it was handed over under"
    );
    assert_eq!(
        retained_release.attempt, witness.attempt,
        "and the attempt token it was dispatched with"
    );
    assert!(
        retained_release.attempt.is_some(),
        "which it actually had: {retained_release:?}"
    );
    assert!(
        witness
            .completion
            .as_ref()
            .is_some_and(|cell| retained_release.carries(cell)),
        "carrying the completion the handover saw it carry"
    );
    assert!(
        witness
            .completion
            .as_ref()
            .is_some_and(|cell| Arc::ptr_eq(cell, &original_release_cell)),
        "which is the one this case took from its own admission"
    );
    assert_eq!(
        (witness.reached_client, witness.reached_window),
        (
            retained_release.reached_client,
            retained_release.reached_window
        ),
        "and reaching the recipient it named then"
    );
    // THE CAPSULE'S RECIPIENT AND THE RELEASE'S ARE THE SAME ONE, and both
    // are this connection's own registration. The witness side is taken
    // from the capsule that was handed over and the retained side from the
    // source obligation the release still answers for; reading both from
    // the release would compare a reading with itself and could not catch
    // a capsule owed to a different endpoint.
    let handed_endpoint = witness
        .endpoint
        .as_ref()
        .expect("the handover recorded the recipient its capsule named");
    assert!(
        retained_release.names_endpoint(handed_endpoint),
        "the retained release answers for the recipient the capsule was minted for: {retained_release:?}"
    );
    assert!(
        retained_release.serves_registration(&unknown_custody.cleanup_record().connection_state),
        "which is this connection's own registration: {retained_release:?}"
    );
    assert!(
        handed_endpoint.is_registration(&unknown_custody.cleanup_record().connection_state),
        "and so is the capsule's"
    );
    // AND THE HANDOVER SUCCEEDED. The seam is after either answer, so the
    // arrangement has to say which one it caught: the capsule was admitted
    // and only the record of that was lost. A full or disconnected queue
    // is a different state, and one the record can describe.
    assert!(
        witness.handed_over,
        "the capsule was admitted by its recipient's queue before the interruption"
    );

    // The debt is the retained release itself, not an accepted-item credit:
    // that credit is returned when the item is disposed of and its event
    // moves into separately reserved storage, which had already happened.
    // What the interruption must not do is settle the release or discard it.
    assert!(
        !phases.is_empty(),
        "the release is retained by the store that outlived the invocation"
    );
    // A REAL VISIT IS DRIVEN, AND WHAT IT REACHED IS NOT OVERSTATED. The
    // interruption closed this invocation's own cleanup budget, and that
    // budget stays closed: nothing here resets or rebuilds it, and no
    // recovery policy is invented to reach further. So this visit yields
    // before the retained source decision and the durable drive does not
    // visit terminal dispatch. Both are recorded as they came.
    let visit = unknown.step();
    let unknown_drive = unknown_store.drive();
    // TYPED, NOT FORMATTED. The interruption closed this invocation's own
    // budget permanently, so its retained visit refuses before authorizing
    // custody or entering the home. That refusal is the guarantee this
    // half rests on, and it is asserted as the value it is.
    assert_eq!(
        visit.phase,
        PrivateMaintenancePhase::Output,
        "the visit after the interruption is the retained output visit: {visit:?}"
    );
    assert_eq!(
        visit.status,
        PrivateMaintenanceStatus::Yielded,
        "which yields rather than running: {visit:?}"
    );
    assert_eq!(
        visit.allowance_refusal,
        Some(sophia_input_authority::ServiceStartRefusal::Interrupted),
        "because the original budget was interrupted and never reopens: {visit:?}"
    );
    assert!(!visit.charged, "so nothing was charged for it: {visit:?}");
    let unknown_after = retained_dispatch(&unknown);
    assert!(
        unknown_after.len() == phases.len()
            && unknown_after
                .iter()
                .zip(phases.iter())
                .all(|(after, before)| after.same_as(before)),
        "the retained record is the same release across the visit, by its own completion: {visit:?}"
    );
    // AT MOST ONE COPY, whichever way the interruption fell. If the bytes
    // went, they went once; if they did not, nothing produced them
    // afterwards. Neither outcome is treated as licence to rebuild.
    let replayed = read_event(&mut unknown_peer, 1);
    assert_eq!(
        replayed, None,
        "no further copy of the release was produced after the interruption"
    );
    let unknown_fact = json!({
        "press_delivery": 12080,
        "release_delivery": 12081,
        "seam": "labelled test-only, keyed by this origin and this delivery, one shot, immediately after the handover returned and before its result was recorded",
        "seam_mode": "one shot, an actual interruption: the result of the handover is never recorded, which is the state the subcase is about",
        "unwound": unknown_closed.unwound,
        "release_bytes_on_wire": release_wire.map(|bytes| bytes.to_vec()),
    "release_wire_note": "null here means the connection ended before the bytes reached the recipient; a value means they did. Both are honest outcomes of interrupting the invocation, and this case asserts neither.",
        "release_answer": release_cell.map(|answer| format!("{answer:?}")),
        "retained_phases": format!("{phases:?}"),
        "retained_release": format!("{retained_release:?}"),
    "original_release_completion": Arc::as_ptr(&original_release_cell) as usize,
    "runner_held_before_submit_on": format!("{held_on:?}"),
    "handover_returned_success": witness.handed_over,
    "capsule_recipient_matches_retained_release": true,
    "capsule_recipient_is_this_registration": true,
    "handover_witness": format!(
        "delivery={:?} incarnation={:?} attempt={:?} client={:?} window={:?}",
        witness.delivery,
        witness.incarnation,
        witness.attempt,
        witness.reached_client,
        witness.reached_window
    ),
    "retained_phases_after_actual_maintenance_visit": format!("{unknown_after:?}"),
        "maintenance_visit": format!("{visit:?}"),
        "durable_drive": format!("{unknown_drive:?}"),
        "what_the_visit_reached": "not the retained source decision. The unwind interrupted this invocation's cleanup budget and it stays closed, so the visit yields before that decision and the durable drive does not visit terminal dispatch. This is therefore NOT evidence that a retry path looked at the handover and declined to resend it. What is established here is the actual interruption, the record that says the handover was begun and never reported, and that no replayable payload was kept.",
        "second_copy_on_wire": replayed.map(|bytes| bytes.to_vec()),
        "writer_receipt_is_the_writers_own": "the recipient half may be answered by the writer that flushed; the executor still cannot join it, and does not resend",
        "charged": unknown_store.reserved(),
    });
    actors.extend(finish_labelled(
        "unknown-handover invocation",
        unknown,
        &[unknown_custody],
    ));

    // A RECIPIENT THAT STOPS TAKING ITS BYTES. The arrangement is the one
    // already proved to stall the writer between the two frames of one
    // capsule: the focus notification is left unread, so whole frame
    // writes are already outstanding when the axis stream starts. It is
    // driven once and asserted exactly; there is no matrix and no weaker
    // reading to fall back to.
    let (partial, partial_actors) = blocked_recipient_attempt(
        "focus-notification-left-unread",
        true,
        2048,
        12052,
        0x320b01,
    );
    actors.extend(partial_actors);

    emit_case(
        "C.indeterminate_send",
        &[
            ("partial_send_not_replayed", partial),
            ("unknown_send_not_replayed", unknown_fact),
            ("enqueued_observation_only", enqueued),
            ("refused_publication_retained", refused),
        ],
        &actors,
    );
}
