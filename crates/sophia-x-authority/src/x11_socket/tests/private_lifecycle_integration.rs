// Integration-overlay tests: real private constructor, participant, route
// registration, recovery and native authority. No display or wire client.
fn lifecycle_integrated_route(
    f: &crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
) -> (
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
) {
    let registry = &f.broker.registry;
    let (registration, channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .unwrap();
    registry
        .attach_private_lifecycle(&registration, admitted(client))
        .unwrap();
    (registration, channels)
}
fn lifecycle_gate(registration: &XServerFrontendClientRouteRegistration) -> PrivateLifecycleGate {
    registration
        .lifecycle
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .gate()
}
fn lifecycle_drain(owner: &PrivateLifecycleOwner) {
    for _ in 0..16 {
        owner.drive(NonZeroUsize::new(1).unwrap()).unwrap();
    }
}

#[test]
fn lifecycle_integration_query_cleanup_and_issue_execute_gate() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let owner = f.terminal.lifecycle.clone();
    let client = XServerFrontendClientId(7601);
    let (registration, _channels) = lifecycle_integrated_route(&f, client);
    let gate = lifecycle_gate(&registration);
    let role = f.reservation_role(client, DeviceId::from_raw(1)).unwrap();
    let request = role
        .reserve(f.control_gate().stamp().unwrap(), 1)
        .unwrap()
        .accepted();
    owner
        .register_query_gate(&gate, admitted(client).namespace.id, client)
        .unwrap();
    f.broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_server(admitted(client).namespace.id, client.raw())
        .unwrap();
    gate.close();
    assert!(f.reservation_role(client, DeviceId::from_raw(2)).is_err());
    let called = std::cell::Cell::new(false);
    assert!(
        f.execute_ordered(&request, client, |permit, _| {
            called.set(true);
            permit.begin_external_effect()
        })
        .is_err()
    );
    assert!(!called.get());
    assert!(
        owner
            .register_query_gate(&gate, admitted(client).namespace.id, client)
            .is_err()
    );
    lifecycle_drain(&owner);
    let x = f.broker.registry.input_authority.lock().unwrap();
    assert!(!x.query_namespace_active(admitted(client).namespace.id));
    assert_eq!(x.server_owner(admitted(client).namespace.id), None);
}

#[test]
fn lifecycle_integration_registration_drop_under_common_only_requests() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let owner = f.terminal.lifecycle.clone();
    let client = XServerFrontendClientId(7602);
    let (registration, _channels) = lifecycle_integrated_route(&f, client);
    let gate = lifecycle_gate(&registration);
    owner
        .register_query_gate(&gate, admitted(client).namespace.id, client)
        .unwrap();
    let common = f.authority().common.lock().unwrap();
    drop(registration); // Actual registration Drop -> recovery disconnect.
    assert!(!gate.is_open());
    assert!(
        f.broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .query_namespace_active(admitted(client).namespace.id)
    );
    assert!(!f.admission_participant().bindings.lock().unwrap().bound[&client].closed);
    drop(common);
    lifecycle_drain(&owner);
    assert!(
        !f.broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .query_namespace_active(admitted(client).namespace.id)
    );
}

#[test]
fn lifecycle_integration_old_lease_cannot_close_new_incarnation() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let owner = f.terminal.lifecycle.clone();
    let client = XServerFrontendClientId(7603);
    f.admission_participant()
        .admit(client, admitted(client))
        .unwrap();
    let old = owner.register(client, admitted(client)).unwrap();
    old.close();
    assert!(
        f.admission_participant()
            .admit(client, admitted(client))
            .is_err()
    );
    lifecycle_drain(&owner);
    let mut replacement = admitted(client);
    replacement.client_id = sophia_protocol::ClientAdmissionId::from_raw(17603);
    f.admission_participant()
        .admit(client, replacement)
        .unwrap();
    let new = owner.register(client, replacement).unwrap();
    drop(old);
    assert!(new.gate().is_open());
    lifecycle_drain(&owner);
    assert_eq!(owner.inventory().unwrap().open, 1);
}

#[test]
fn lifecycle_integration_capacity_refusal_does_not_publish_binding() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    for n in 0..16 {
        let client = XServerFrontendClientId(7700 + n);
        f.admission_participant()
            .admit(client, admitted(client))
            .unwrap();
    }
    for n in 0..32 {
        let client = XServerFrontendClientId(7800 + n);
        assert_eq!(
            f.admission_participant().admit(client, admitted(client)),
            Err(PrivateAdmissionRefusal::ClientRecordsExhausted)
        );
    }
    assert_eq!(
        f.admission_participant()
            .bindings
            .lock()
            .unwrap()
            .bound
            .len(),
        16
    );
    assert_eq!(f.terminal.lifecycle.inventory().unwrap().open, 16);
}

#[test]
fn lifecycle_integration_zero_hold_debt_survives_handle_and_durable_handover() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let durable = f.durable.clone();
    let owner = f.terminal.lifecycle.clone();
    for n in 0..3 {
        let client = XServerFrontendClientId(7900 + n);
        f.admission_participant()
            .admit(client, admitted(client))
            .unwrap();
    }
    assert!(f.terminal.holds.is_empty());
    let handle = f.shutdown();
    assert_eq!(handle.terminal_outstanding(), Some(2));
    assert!(!handle.is_settled());
    drop(handle);
    assert_eq!(durable.terminal_inventories(), Some(1));
    for _ in 0..16 {
        assert!(durable.drive().readable);
    }
    assert_eq!(
        owner.inventory().unwrap(),
        PrivateLifecycleInventory::default()
    );
    assert_eq!(durable.terminal_inventories(), Some(0));
}

#[test]
fn lifecycle_integration_poison_is_unavailable_not_settled() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let durable = f.durable.clone();
    let owner = f.terminal.lifecycle.clone();
    let client = XServerFrontendClientId(7606);
    f.admission_participant()
        .admit(client, admitted(client))
        .unwrap();
    let poison = owner.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poison.inner.records.lock().unwrap();
            panic!("owned records poison fixture");
        })
        .join()
        .is_err()
    );
    let handle = f.shutdown();
    assert_eq!(handle.terminal_outstanding(), None);
    assert!(!handle.is_settled());
    drop(handle);
    assert!(!durable.drive().readable);
    assert_eq!(durable.terminal_inventories(), Some(1));
}

#[test]
fn lifecycle_integration_recovery_closure_runs_without_delivery_receipt() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let client = XServerFrontendClientId(7607);
    let (registration, _channels) = lifecycle_integrated_route(&f, client);
    let owner = f.terminal.lifecycle.clone();
    let gate = lifecycle_gate(&registration);
    owner
        .register_query_gate(&gate, admitted(client).namespace.id, client)
        .unwrap();
    // No ticket exists: cleanup must not be inferred from the expired list.
    f.broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .unwrap();
    assert!(!gate.is_open());
    assert!(
        f.broker
            .registry
            .input_recovery
            .recover(Instant::now(), true)
            .unwrap()
            .is_empty()
    );
    assert!(
        !f.broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .query_namespace_active(admitted(client).namespace.id)
    );
}

#[test]
fn lifecycle_integration_query_owner_finish_and_drop_do_not_repeat_cleanup() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let state = X11CoreSocketServerState::new();
    state
        .runtime
        .lock()
        .unwrap()
        .set_input_authority(f.broker.registry.input_authority.clone());
    let client = XServerFrontendClientId(7608);
    let (registration, _channels) = lifecycle_integrated_route(&f, client);
    let owner = f.terminal.lifecycle.clone();
    let gate = lifecycle_gate(&registration);
    let mut query = X11QueryOwner::register(
        &state.runtime,
        admitted(client).namespace.id,
        client,
        Some((owner.clone(), gate)),
    )
    .unwrap();
    query.finish().unwrap();
    assert!(
        !state
            .runtime
            .lock()
            .unwrap()
            .input_authority_mut()
            .query_namespace_active(admitted(client).namespace.id)
    );
    // A later registration is deliberate fixture state: neither repeated
    // finish nor Drop may run old cleanup against it.
    state
        .runtime
        .lock()
        .unwrap()
        .input_authority_mut()
        .register_query_client(admitted(client).namespace.id, client.raw());
    query.finish().unwrap();
    drop(query);
    drop(registration);
    lifecycle_drain(&owner);
    assert!(
        state
            .runtime
            .lock()
            .unwrap()
            .input_authority_mut()
            .query_namespace_active(admitted(client).namespace.id)
    );
}

#[test]
fn lifecycle_integration_native_hold_debt_is_not_query_cleanup() {
    let client = XServerFrontendClientId(7609);
    let surface = SurfaceId::new(7609, 1);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { keeper, runner, ingress, channels, registration, .. } = &mut fixture;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let owner = private.terminal.lifecycle.clone();
    let gate = lifecycle_gate(registration);
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        ingress,
        &keeper.lease(),
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        7609,
    );
    gate.close();
    lifecycle_drain(&owner);
    let mut cursor = 0;
    assert!(
        private
            .authority()
            .common
            .lock()
            .unwrap()
            .next_debt(&mut cursor)
            .is_some()
    );
    assert_eq!(private.terminal.holds.len(), 1);
}

struct LifecycleSetupPolicy(sophia_protocol::ClientAdmissionContext);
impl XServerFrontendAdmissionPolicy for LifecycleSetupPolicy {
    fn admit(
        &self,
        _request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        Ok(self.0)
    }
    fn revoke(
        &self,
        _context: ClientAdmissionContext,
    ) -> Result<(), XServerFrontendAdmissionError> {
        Ok(())
    }
}

#[test]
fn lifecycle_integration_actual_setup_both_orders_closes_exact_query_owner() {
    for big in [false, true] {
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let f = private_for_roles(&service_keeper);
        let registry = f.broker.registry.clone();
        let owner = f.terminal.lifecycle.clone();
        let state = Arc::new(X11CoreSocketServerState::new());
        state
            .runtime
            .lock()
            .unwrap()
            .set_input_authority(registry.input_authority.clone());
        let context = admitted(XServerFrontendClientId(7610));
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let server_state = state.clone();
        let worker = std::thread::spawn(move || {
            serve_x11_core_socket_client_with_trace_observer_and_input(
                &mut server,
                context.namespace.id,
                &server_state,
                X11ClientConnectionInputs {
                    input_receiver: None,
                    control_channels: None,
                    client_routing: Some(registry),
                },
                X11ClientAdmissionContext {
                    authorization: &XServerFrontendSetupAuthorization::default(),
                    admission_policy: Some(Arc::new(LifecycleSetupPolicy(context))),
                    worker_admission: None,
                },
                |_| Ok(None),
            )
        });
        let mut setup = [0u8; 12];
        setup[0] = if big { b'B' } else { b'l' };
        setup[if big { 3 } else { 2 }] = 11;
        std::io::Write::write_all(&mut client, &setup).unwrap();
        let mut prefix = [0u8; 8];
        std::io::Read::read_exact(&mut client, &mut prefix).unwrap();
        assert_eq!(prefix[0], 1);
        let units = if big {
            u16::from_be_bytes([prefix[6], prefix[7]])
        } else {
            u16::from_le_bytes([prefix[6], prefix[7]])
        };
        let mut body = vec![0; usize::from(units) * 4];
        std::io::Read::read_exact(&mut client, &mut body).unwrap();
        // Setup reply precedes route admission. A synchronous GetInputFocus
        // reply proves setup and writer construction finished on this socket.
        let mut request = [43u8, 0, 0, 0];
        request[if big { 3 } else { 2 }] = 1;
        std::io::Write::write_all(&mut client, &request).unwrap();
        let mut reply = [0u8; 32];
        std::io::Read::read_exact(&mut client, &mut reply).unwrap();
        assert_eq!(reply[0], 1);
        assert_eq!(owner.inventory().unwrap().open, 1);
        assert!(
            f.admission_participant()
                .bindings
                .lock()
                .unwrap()
                .bound
                .values()
                .all(|bound| bound.grants.is_empty()),
            "ordinary setup must not issue synthetic grants"
        );
        assert!(
            state
                .runtime
                .lock()
                .unwrap()
                .input_authority_mut()
                .query_namespace_active(context.namespace.id)
        );
        client.shutdown(std::net::Shutdown::Both).unwrap();
        worker.join().unwrap().unwrap();
        lifecycle_drain(&owner);
        assert_eq!(
            owner.inventory().unwrap(),
            PrivateLifecycleInventory::default()
        );
        assert!(
            !state
                .runtime
                .lock()
                .unwrap()
                .input_authority_mut()
                .query_namespace_active(context.namespace.id)
        );
    }
}

#[test]
fn lifecycle_integration_namespace_revocation_retains_exact_native_cleanup() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let owner = f.terminal.lifecycle.clone();
    let first = XServerFrontendClientId(7611);
    let other = XServerFrontendClientId(7612);
    let (a, _ca) = lifecycle_integrated_route(&f, first);
    let (b, _cb) = lifecycle_integrated_route(&f, other);
    let ga = lifecycle_gate(&a);
    let gb = lifecycle_gate(&b);
    owner
        .register_query_gate(&ga, admitted(first).namespace.id, first)
        .unwrap();
    owner
        .register_query_gate(&gb, admitted(other).namespace.id, other)
        .unwrap();
    assert_eq!(
        f.admission_participant()
            .revoke_namespace(admitted(first).namespace.id)
            .unwrap()
            .closed,
        1
    );
    assert!(!ga.is_open());
    assert!(gb.is_open());
    assert_eq!(owner.inventory().unwrap().closed, 1);
    lifecycle_drain(&owner);
    let native = f.broker.registry.input_authority.lock().unwrap();
    assert!(!native.query_namespace_active(admitted(first).namespace.id));
    assert!(native.query_namespace_active(admitted(other).namespace.id));
}

#[test]
fn lifecycle_integration_live_origin_cannot_resolve_a_closing_recipient() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let registry = &f.broker.registry;
    let namespace = NamespaceId::from_raw(7613);
    let sender = XServerFrontendClientId(7613);
    let recipient = XServerFrontendClientId(7614);
    let mut registrations = Vec::new();
    for client in [sender, recipient] {
        let admission = namespaced(client, namespace);
        let (registration, channels) = registry
            .register_client_with_admission(client, Some(admission))
            .unwrap();
        registry
            .attach_private_lifecycle(&registration, admission)
            .unwrap();
        registry
            .attach_connection_state(
                &registration,
                namespace,
                Arc::new(Mutex::new(XCoreEventSelectionState::default())),
                Arc::new(AtomicU64::new(u64::from(X_SETUP_DEFAULT_ROOT))),
            )
            .unwrap();
        registrations.push((registration, channels));
    }
    registry
        .install_private_applied(&f.participant, namespace)
        .unwrap();
    f.participant
        .under_boundary(|_, _, bindings| {
            let clients = registry.clients.lock().unwrap();
            assert!(
                registry
                    .applied_client(&clients, sender, &bindings.bound[&sender])
                    .is_ok()
            );
            assert!(
                registry
                    .applied_client(&clients, recipient, &bindings.bound[&recipient])
                    .is_ok()
            );
        })
        .unwrap();
    lifecycle_gate(&registrations[1].0).close();
    f.participant
        .under_boundary(|_, _, bindings| {
            let clients = registry.clients.lock().unwrap();
            assert!(
                !bindings.bound[&recipient].closed,
                "closure request has not pretended to finish revocation"
            );
            assert!(
                registry
                    .applied_client(&clients, sender, &bindings.bound[&sender])
                    .is_ok()
            );
            assert!(matches!(
                registry.applied_client(&clients, recipient, &bindings.bound[&recipient]),
                Err(PrivateAppliedRegistryRefusal::AdmissionClosed)
            ));
        })
        .unwrap();
}

#[test]
fn lifecycle_integration_actual_setup_binds_before_its_first_fallible_attachment() {
    // THE ORDER AT THE REAL CALL SITE, over a real socket. Connection setup
    // publishes the row, binds, and only then attaches lifecycle, connection
    // state and recovery. A binding placed after those left every one of their
    // refusals dropping a queue that was already reachable.
    //
    // The refusal here is the fixture's own: this instance has room for one
    // lifecycle lease and the first connection holds it, so the second is
    // refused at the first attachment after binding. Nothing is hooked or
    // patched to arrange it.
    let durable = PrivateSettlementOwner::with_capacities(4, 4);
    let service_keeper = service_owner(&durable, 1);
    let private = private_over(&service_keeper, 1);
    let registry = private.broker.registry.clone();

    // The one lease, taken by a connection that keeps it.
    let first = XServerFrontendClientId(8481);
    let (held, _held_channels) = registry
        .register_client_with_admission(first, Some(admitted(first)))
        .expect("a place and a row");
    registry
        .attach_private_lifecycle(&held, admitted(first))
        .expect("the only lifecycle lease");

    let state = Arc::new(X11CoreSocketServerState::new());
    state
        .runtime
        .lock()
        .unwrap()
        .set_input_authority(registry.input_authority.clone());
    let context = admitted(XServerFrontendClientId(8482));
    let (mut client, mut server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let worker = std::thread::spawn(move || {
        serve_x11_core_socket_client_with_trace_observer_and_input(
            &mut server,
            context.namespace.id,
            &state,
            X11ClientConnectionInputs {
                input_receiver: None,
                control_channels: None,
                client_routing: Some(registry),
            },
            X11ClientAdmissionContext {
                authorization: &XServerFrontendSetupAuthorization::default(),
                admission_policy: Some(Arc::new(LifecycleSetupPolicy(context))),
                worker_admission: None,
            },
            |_| Ok(None),
        )
    });
    std::io::Write::write_all(&mut client, &[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    let mut prefix = [0u8; 8];
    std::io::Read::read_exact(&mut client, &mut prefix).unwrap();
    assert_eq!(prefix[0], 1, "the setup reply precedes route admission");
    let units = u16::from_le_bytes([prefix[6], prefix[7]]);
    let mut body = vec![0; usize::from(units) * 4];
    std::io::Read::read_exact(&mut client, &mut body).unwrap();

    // Joined before anything is asserted, so what is read below is what the
    // real setup left behind.
    let result = worker.join().expect("the serving thread");
    let error = result.expect_err("the second connection has no lifecycle lease");
    // Either half of that attachment refusing is the same edge for this
    // control: both are inside the first fallible thing setup does after it
    // publishes the row and binds.
    let refusal = error.to_string();
    assert!(
        refusal.contains("private admission failed")
            || refusal.contains("private lifecycle failed"),
        "refused inside the first attachment after binding, got {refusal}"
    );

    // The refused connection's place holds its BOUND output. If the binding
    // came after that attachment there would be nothing here: the receiver
    // would have gone with the setup frame.
    let bound = durable
        .with_ordered_continuation(1, |continuation| {
            matches!(
                continuation,
                PrivateOrderedContinuation::Setup {
                    accepted: PrivateOrderedSetupCustody::Transport(_),
                    refusal: X11OrderedServingRefusal::Unserved,
                    ..
                }
            )
        })
        .expect("the refused connection's place");
    assert!(
        bound,
        "the real setup bound this connection's queue before the attachment \
         that refused it"
    );
    drop(held);
}

#[test]
fn lifecycle_integration_close_wakes_a_parked_connection() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let client = XServerFrontendClientId(7631);
    let (registration, _channels) = lifecycle_integrated_route(&f, client);
    let gate = lifecycle_gate(&registration);

    let notifier = crate::ConnectionNotifier::new().unwrap();
    assert!(gate.wake_on_close(&notifier), "a fresh gate takes a waiter");
    // One connection, one wake. A second ask means something tried to park
    // this incarnation twice, which is a defect rather than a second waiter.
    assert!(!gate.wake_on_close(&notifier));
    assert!(gate.is_open(), "registering must not close the gate");

    gate.close();

    // Revocation does not wait on the runner: the parked connection is roused
    // by the close itself, and finds the gate already shut rather than
    // parking again on one that still looks open.
    assert!(!gate.is_open());
    let (ours, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut wait = crate::ConnectionWait::new(std::os::fd::AsFd::as_fd(&ours), &notifier);
    assert_eq!(
        wait.wait_until(Some(
            std::time::Instant::now() + std::time::Duration::from_secs(5)
        ))
        .unwrap(),
        crate::ConnectionWake::Notified,
    );
}

#[test]
fn lifecycle_integration_a_new_incarnation_does_not_inherit_a_departed_wake() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let f = private_for_roles(&service_keeper);
    let client = XServerFrontendClientId(7632);
    let (registration, _channels) = lifecycle_integrated_route(&f, client);
    let gate = lifecycle_gate(&registration);
    let departed = crate::ConnectionNotifier::new().unwrap();
    assert!(gate.wake_on_close(&departed));
    gate.close();
    drop(registration);
    lifecycle_drain(&f.terminal.lifecycle);

    // The slot comes back for someone else. Its waiter is replaced rather
    // than cleared, so the next connection can park at all -- inheriting the
    // old one would leave the new client unable to register and silently
    // unwakeable.
    let (next, _channels) = lifecycle_integrated_route(&f, XServerFrontendClientId(7633));
    let next_gate = lifecycle_gate(&next);
    let fresh = crate::ConnectionNotifier::new().unwrap();
    assert!(
        next_gate.wake_on_close(&fresh),
        "a reused slot must offer its new connection an unclaimed waiter"
    );
}
