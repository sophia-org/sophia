// Publication means channel admission, not observation by Session. Abandoned
// counts notifications whose consumer disappeared, never cancelled effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RouteLeaseOutboxSnapshot {
    capacity: usize,
    pending: usize,
    reserved: usize,
    disconnected: bool,
    accepted: u64,
    published: u64,
    abandoned: u64,
}

impl RouteLeaseOutbox {
    fn snapshot(&self) -> Result<RouteLeaseOutboxSnapshot, XServerFrontendRouteError> {
        let state = self.state.lock().map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        Ok(RouteLeaseOutboxSnapshot {
            capacity: state.capacity,
            pending: state.pending.len(),
            reserved: state.reserved,
            disconnected: state.disconnected,
            accepted: state.accepted,
            published: state.published,
            abandoned: state.abandoned,
        })
    }
}

fn route_lease_update_fixture(sequence: u64) -> XAuthorityRouteLeaseUpdate {
    XAuthorityRouteLeaseUpdate {
        identity: sophia_protocol::ApplicationRouteLeaseIdentity {
            id: sophia_protocol::ApplicationRouteLeaseId::from_raw(sequence),
            seat: SeatId::from_raw(1),
            frontend_sequence: sequence,
            control_epoch: 2,
        },
        target_surface: SurfaceId::INVALID,
        admission: sophia_protocol::ClientAdmissionContext::new(
            sophia_protocol::ClientAdmissionId::from_raw(8),
            sophia_protocol::NamespaceContext::new(
                NamespaceId::from_raw(21),
                sophia_protocol::NamespaceProfile::Confined,
                sophia_protocol::NamespaceCapabilities::NONE,
            )
            .unwrap(),
            sophia_protocol::ClientAuthProvenance::new(
                sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
                5,
            )
            .unwrap(),
        )
        .unwrap(),
        kind: XAuthorityRouteLeaseUpdateKind::Released,
    }
}

// Keep the receiver outside the worker so a regression to blocking send can
// be interrupted. The timeout is only a hang guard, not a performance budget.
fn route_lease_bounded_call<T: Send + 'static>(
    receiver: std::sync::mpsc::Receiver<XAuthorityRouteLeaseUpdate>,
    run: impl FnOnce() -> T + Send + 'static,
) -> (T, std::sync::mpsc::Receiver<XAuthorityRouteLeaseUpdate>) {
    let (done, result) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let value = run();
        let _ = done.send(value);
    });
    match result.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(value) => {
            worker.join().unwrap();
            (value, receiver)
        }
        Err(error) => {
            drop(receiver);
            let _ = worker.join();
            panic!("route lease notification failed to return without a receiver drain: {error}");
        }
    }
}

fn route_lease_assert_conservation(outbox: &RouteLeaseOutbox) -> RouteLeaseOutboxSnapshot {
    let snapshot = outbox.snapshot().unwrap();
    assert!(snapshot.pending + snapshot.reserved <= snapshot.capacity);
    assert_eq!(
        snapshot.accepted,
        snapshot.published + snapshot.abandoned + snapshot.pending as u64
    );
    snapshot
}

#[test]
fn route_lease_outbox_preserves_fifo_at_full_capacity() {
    let (sender, receiver) = sync_channel(1);
    let outbox = RouteLeaseOutbox::new(Some(sender.into()), 2);
    let work = outbox.clone();
    let ((), receiver) = route_lease_bounded_call(receiver, move || {
        for sequence in 1..=3 {
            work.reserve()
                .unwrap()
                .unwrap()
                .commit(route_lease_update_fixture(sequence))
                .unwrap();
        }
        assert!(matches!(
            work.reserve(),
            Err(XServerFrontendRouteError::RouteLeaseQueueFull)
        ));
    });
    let snapshot = route_lease_assert_conservation(&outbox);
    assert_eq!(
        (snapshot.accepted, snapshot.published, snapshot.pending),
        (3, 1, 2)
    );
    assert_eq!(receiver.try_recv().unwrap(), route_lease_update_fixture(1));
    let work = outbox.clone();
    let (flushed, receiver) = route_lease_bounded_call(receiver, move || work.flush());
    assert_eq!(flushed, Ok(1));
    assert_eq!(receiver.try_recv().unwrap(), route_lease_update_fixture(2));
    assert_eq!(outbox.flush(), Ok(1));
    assert_eq!(receiver.try_recv().unwrap(), route_lease_update_fixture(3));
    assert_eq!(outbox.flush(), Ok(0));
    assert_eq!(route_lease_assert_conservation(&outbox).published, 3);
    assert!(matches!(
        receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn route_lease_outbox_aborted_reservation_returns_exact_credit() {
    let (sender, _receiver) = sync_channel(1);
    let outbox = RouteLeaseOutbox::new(Some(sender.into()), 1);
    let reservation = outbox.reserve().unwrap().unwrap();
    assert_eq!(route_lease_assert_conservation(&outbox).reserved, 1);
    assert!(matches!(
        outbox.reserve(),
        Err(XServerFrontendRouteError::RouteLeaseQueueFull)
    ));
    drop(reservation);
    let snapshot = route_lease_assert_conservation(&outbox);
    assert_eq!((snapshot.reserved, snapshot.accepted), (0, 0));
    outbox
        .reserve()
        .unwrap()
        .unwrap()
        .commit(route_lease_update_fixture(1))
        .unwrap();
    assert_eq!(route_lease_assert_conservation(&outbox).published, 1);
}

#[test]
fn route_lease_outbox_disconnect_accounts_pending_and_held_reservation() {
    let (sender, receiver) = sync_channel(1);
    let outbox = RouteLeaseOutbox::new(Some(sender.into()), 3);
    let held = outbox.reserve().unwrap().unwrap();
    let aborted = outbox.reserve().unwrap().unwrap();
    let work = outbox.clone();
    let ((), receiver) = route_lease_bounded_call(receiver, move || {
        work.reserve()
            .unwrap()
            .unwrap()
            .commit(route_lease_update_fixture(1))
            .unwrap();
        work.reserve()
            .unwrap()
            .unwrap()
            .commit(route_lease_update_fixture(2))
            .unwrap();
    });
    let snapshot = route_lease_assert_conservation(&outbox);
    assert_eq!((snapshot.pending, snapshot.reserved), (1, 2));
    drop(receiver);
    assert_eq!(
        outbox.flush(),
        Err(XServerFrontendRouteError::RouteLeaseQueueDisconnected)
    );
    assert_eq!(
        held.commit(route_lease_update_fixture(3)),
        Err(XServerFrontendRouteError::RouteLeaseQueueDisconnected)
    );
    drop(aborted);
    let snapshot = route_lease_assert_conservation(&outbox);
    assert!(snapshot.disconnected);
    assert_eq!(
        (
            snapshot.accepted,
            snapshot.published,
            snapshot.abandoned,
            snapshot.reserved
        ),
        (3, 1, 2, 0)
    );
    assert!(matches!(
        outbox.reserve(),
        Err(XServerFrontendRouteError::RouteLeaseQueueDisconnected)
    ));
}

#[test]
fn route_lease_outbox_without_owner_has_no_notification_obligation() {
    let outbox = RouteLeaseOutbox::new(None, 1);
    assert!(outbox.reserve().unwrap().is_none());
    assert_eq!(outbox.flush(), Ok(0));
    assert_eq!(route_lease_assert_conservation(&outbox).accepted, 0);
}

#[test]
fn route_lease_broker_release_retains_full_channel_update_until_retry() {
    let (ack_sender, _ack_receiver) = sync_channel(1);
    let (delivery_sender, _delivery_receiver) = std::sync::mpsc::channel();
    let (lease_sender, lease_receiver) = sync_channel(1);
    lease_sender
        .try_send(route_lease_update_fixture(10))
        .unwrap();
    let broker = XServerFrontendRouteBroker::with_route_capacities_xkb_and_lease_updates(
        XServerFrontendRouteCapacities::uniform(NonZeroUsize::new(1).unwrap()),
        ack_sender,
        delivery_sender,
        lease_sender,
        crate::XkbRmlvoConfig::default(),
    )
    .unwrap();
    let (mut broker, lease_receiver) = route_lease_bounded_call(lease_receiver, move || {
        let update = route_lease_update_fixture(11);
        broker
            .registry
            .release_route_lease(XAuthorityRouteLeaseRelease {
                identity: update.identity,
                admission: update.admission,
            })
            .unwrap();
        assert_eq!(
            route_lease_assert_conservation(&broker.registry.route_lease_updates).pending,
            1
        );
        let refused = route_lease_update_fixture(12);
        assert_eq!(
            broker
                .registry
                .release_route_lease(XAuthorityRouteLeaseRelease {
                    identity: refused.identity,
                    admission: refused.admission,
                }),
            Err(XServerFrontendRouteError::RouteLeaseQueueFull)
        );
        assert_eq!(
            route_lease_assert_conservation(&broker.registry.route_lease_updates).accepted,
            1
        );
        broker
    });
    assert_eq!(
        lease_receiver.try_recv().unwrap(),
        route_lease_update_fixture(10)
    );
    // The ordinary broker turn must retry even without a new routed event.
    broker.route_pending().unwrap();
    assert_eq!(
        lease_receiver.try_recv().unwrap(),
        route_lease_update_fixture(11)
    );
    assert_eq!(
        route_lease_assert_conservation(&broker.registry.route_lease_updates).pending,
        0
    );
    assert!(matches!(
        lease_receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn route_lease_broker_press_retains_confirmation_and_refuses_before_effect_at_bound() {
    for exhausted in [false, true] {
        let (ack_sender, _ack_receiver) = sync_channel(1);
        let (delivery_sender, _delivery_receiver) = std::sync::mpsc::channel();
        let (lease_sender, lease_receiver) = sync_channel(1);
        lease_sender
            .try_send(route_lease_update_fixture(10))
            .unwrap();
        let mut broker = XServerFrontendRouteBroker::with_route_capacities_xkb_and_lease_updates(
            XServerFrontendRouteCapacities::uniform(NonZeroUsize::new(1).unwrap()),
            ack_sender,
            delivery_sender,
            lease_sender,
            crate::XkbRmlvoConfig::default(),
        )
        .unwrap();
        let mut expected = route_lease_update_fixture(11);
        expected.target_surface = SurfaceId::new(31, 2);
        expected.kind = XAuthorityRouteLeaseUpdateKind::Confirmed;
        let (mut broker, lease_receiver) = route_lease_bounded_call(lease_receiver, move || {
            let client = XServerFrontendClientId(17);
            let namespace = expected.admission.namespace.id;
            let (_registration, channels) = broker
                .registry
                .register_client_with_admission(client, Some(expected.admission))
                .unwrap();
            broker
                .registry
                .register_surface(
                    client,
                    namespace,
                    expected.target_surface,
                    XResourceId::new(0x200001, 1),
                )
                .unwrap();
            let reserved = exhausted.then(|| {
                broker
                    .registry
                    .route_lease_updates
                    .reserve()
                    .unwrap()
                    .unwrap()
            });
            broker
                .routed_input_sender()
                .send(XAuthorityRoutedInput {
                    request: RoutedInputRequest {
                        serial: 1,
                        seat: expected.identity.seat,
                        device: DeviceId::from_raw(2),
                        time_msec: 1,
                        target_surface: expected.target_surface,
                        global_position: Point::default(),
                        local_position: Point::default(),
                        kind: InputEventKind::PointerButton {
                            button: 0x110,
                            pressed: true,
                        },
                    },
                    route_lease: Some(expected.identity),
                    delivery: None,
                    mode: XAuthorityRoutedInputMode::Deliver,
                    origin: XAuthorityRoutedInputOrigin::Physical,
                })
                .unwrap();
            if exhausted {
                assert_eq!(
                    broker.route_pending(),
                    Err(XServerFrontendRouteError::RouteLeaseQueueFull)
                );
                assert!(
                    channels.input.try_recv().is_err(),
                    "refused press reached client input"
                );
                assert!(
                    broker
                        .registry
                        .input_authority
                        .lock()
                        .unwrap()
                        .pointer_grab(namespace)
                        .is_none(),
                    "refused press created a grab"
                );
                assert_eq!(
                    route_lease_assert_conservation(&broker.registry.route_lease_updates).accepted,
                    0
                );
            } else {
                assert_eq!(broker.route_pending(), Ok(1));
                assert!(channels.input.try_recv().is_ok());
                assert!(
                    broker
                        .registry
                        .input_authority
                        .lock()
                        .unwrap()
                        .pointer_grab(namespace)
                        .is_some()
                );
                assert_eq!(
                    route_lease_assert_conservation(&broker.registry.route_lease_updates).pending,
                    1
                );
            }
            drop(reserved);
            broker
        });
        assert_eq!(
            lease_receiver.try_recv().unwrap(),
            route_lease_update_fixture(10)
        );
        broker.route_pending().unwrap();
        if exhausted {
            assert!(matches!(
                lease_receiver.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
        } else {
            assert_eq!(lease_receiver.try_recv().unwrap(), expected);
        }
    }
}
