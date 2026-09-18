// Controls for the private routed service driving its own prepared runner:
// real sockets and admission, the existing producers obtained through the
// service's port after readiness, ordered input and control submitted through
// them and served by the runner's bounded turns to the registered workers'
// wires. Nothing here supplies a re-addressed capsule; what reaches a client
// is what the service's own order decided.
//
// NATIVE AND RENDERING FACTS ARE SUPPLIED by the device-hidden
// `private_authority()` fixture the frontend parts are built from, as every
// private control in this crate supplies them; nothing here is hardware
// evidence. Wire expectations are hand-encoded from the X protocol, the
// submitted request and the requests this control itself sent, never read
// back from the crate's encoder.

/// One producing launch: the service on its own thread over an owner and
/// store the control shares with it, its real socket, and the caller's side
/// of the producer port.
struct ProducingLaunch {
    handle: std::thread::JoinHandle<ProducedOutcome>,
    finished: Receiver<()>,
    registry: XServerFrontendRouteRegistry,
    controller: PrivateAuthorityController,
    raster: XServerFrontendRasterRouter,
    commands: SyncSender<XServerFrontendServiceCommand>,
    transactions: Receiver<XAuthorityObservedTransactionBatch>,
    acks: Receiver<XAuthorityClientControlAck>,
    deliveries: Receiver<XAuthorityClientInputDelivery>,
    access: PrivateProducerAccess,
    owner: Arc<PrivateServiceOwner>,
}

/// What the invocation returned, whole, plus the owner's state after.
struct ProducedOutcome {
    execution: Option<PrivateExecutionReading>,
    execution_abandoned: Vec<PrivateExecutionReading>,
    execution_inventory_matches: bool,
    execution_collected: bool,
    unwound: bool,
    ok: Option<bool>,
    error: Option<String>,
    workers: Vec<PrivateWorkerCollection>,
    uncollected: Vec<usize>,
    maintenance: Vec<PrivateDeferredCleanupOutcome>,
    /// The order tally, from whichever return carried it.
    order: Option<PrivateOrderTally>,
    after: AfterService,
    /// The native holds the returned settlement's terminal inventory still
    /// carried: (client, window, press delivery still pending in custody).
    retained_holds: Vec<(XServerFrontendClientId, u64, bool)>,
    /// The same, from the terminal inventories the store retained (an
    /// unwound invocation hands its inventory there).
    store_holds: Vec<(XServerFrontendClientId, u64, bool)>,
    /// The returned settlement's terminal counts (holds, settling,
    /// delivering, undelivered, pending custody), if a settlement returned.
    terminal: Option<(usize, usize, usize, usize, bool)>,
    /// Exact source proof and writer completion observations, read while the
    /// returned inventory still owns each release (no proof is fabricated).
    key_releases: Vec<KeyServiceReleaseObservation>,
}

/// A hold record's exact identity: whose window it reached, and whether its
/// press capsule is still pending in its custody.
/// What a terminal inventory still carries, by kind: holds, settling
/// releases, items being delivered, items undelivered, and whether a pending
/// custody stands.
fn terminal_counts(terminal: Option<&PrivateTerminalInventory>) -> Option<(usize, usize, usize, usize, bool)> {
    terminal.map(|terminal| {
        (
            terminal.holds.len(),
            terminal.settling.len(),
            terminal.delivering.len(),
            terminal.undelivered.len(),
            terminal.pending_custody.is_some(),
        )
    })
}

fn holds_of(terminal: Option<&PrivateTerminalInventory>) -> Vec<(XServerFrontendClientId, u64, bool)> {
    terminal
        .map(|terminal| {
            terminal
                .holds
                .iter()
                .map(|hold| {
                    (
                        hold.reached.client,
                        hold.reached.window.local.raw(),
                        hold.custody.pending.is_some(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn producing_parts(
    clients: usize,
) -> (
    crate::PrivateFrontendParts,
    Receiver<XAuthorityClientControlAck>,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (control_acknowledgements, acks) = sync_channel(16);
    let (input_deliveries, deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    (
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(clients).expect("a bound"),
            input_capacity: NonZeroUsize::new(4).expect("a bound"),
            control_acknowledgements,
            input_deliveries,
            authority,
            issuer,
            submit,
        },
        acks,
        deliveries,
    )
}

fn launch_producing(tag: &str, namespace: u64, clients: usize) -> (ProducingLaunch, std::path::PathBuf) {
    launch_producing_with(tag, namespace, clients, false, Arc::new(|_| {}))
}

/// The same, choosing whether every connection gets its own admission id
/// (needed for more than one worker per service) and the backpressure
/// observer the service reports to.
fn launch_producing_with(
    tag: &str,
    namespace: u64,
    clients: usize,
    distinct_admissions: bool,
    observer: Arc<XAuthorityBackpressureObserver>,
) -> (ProducingLaunch, std::path::PathBuf) {
    launch_producing_observed(
        tag,
        namespace,
        clients,
        distinct_admissions,
        observer,
        64,
        Arc::new(Mutex::new(None)),
    )
}

/// The same, with the transaction transport's capacity and the slot the
/// service thread records its id in (for an observer that unwinds only
/// there).
fn launch_producing_observed(
    tag: &str,
    namespace: u64,
    clients: usize,
    distinct_admissions: bool,
    observer: Arc<XAuthorityBackpressureObserver>,
    transport_capacity: usize,
    service_thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
) -> (ProducingLaunch, std::path::PathBuf) {
    let socket_path = private_service_socket(tag);
    let (transaction_sender, transactions) = sync_channel(transport_capacity);
    let (commands, service_commands) = sync_channel(4);
    let (registry_out, registry_in) = channel();
    let (port, access) = PrivateProducerAccess::for_service();
    let (parts, acks, deliveries) = producing_parts(clients);
    let durable = PrivateSettlementOwner::default();
    let owner = Arc::new(service_owner(&durable, clients));
    let config = if distinct_admissions {
        distinct_config(&socket_path, NamespaceId::from_raw(namespace), clients)
    } else {
        private_service_config(&socket_path, NamespaceId::from_raw(namespace), clients)
    };
    let (done, finished) = channel();
    let service_owner = Arc::clone(&owner);
    let service_durable = durable.clone();
    let handle = std::thread::spawn(move || {
        *service_thread.lock().expect("a writable slot") = Some(std::thread::current().id());
        let private = crate::PrivateXServerFrontend::new(parts, &service_owner)
            .unwrap_or_else(|(refusal, _)| panic!("a frontend over this owner: {refusal:?}"));
        let _ = registry_out.send((private.broker.registry.clone(), private.broker.raster_router(), private.controller.clone()));
        let lease = service_owner.lease();
        let mut execution = PrivateServiceExecutionKeeper::new();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            serve_private_frontend_until_stopped(
                private,
                &lease,
                &mut execution,
                config,
                transaction_sender,
                service_commands,
                port,
                observer,
            )
        }));
        let unwound = outcome.is_err();
        let mut retained_holds = Vec::new();
        let mut terminal = None;
        let mut key_releases = Vec::new();
        let (ok, error, workers, uncollected, maintenance, order) = match outcome.ok() {
            Some(Ok(ret)) => {
                retained_holds = holds_of(ret.settlement.terminal.as_ref());
                terminal = terminal_counts(ret.settlement.terminal.as_ref());
                key_releases = observe_key_service_releases(ret.settlement.terminal.as_ref());
                (
                    Some(true),
                    None,
                    ret.workers,
                    Vec::new(),
                    ret.maintenance,
                    Some(ret.order),
                )
            }
            Some(Err(PrivateServiceFailure::Failed {
                error,
                workers,
                maintenance,
                order,
                settlement,
                ..
            })) => {
                let order = *order;
                retained_holds = holds_of(settlement.terminal.as_ref());
                terminal = terminal_counts(settlement.terminal.as_ref());
                key_releases = observe_key_service_releases(settlement.terminal.as_ref());
                (
                    Some(false),
                    Some(error.to_string()),
                    workers,
                    Vec::new(),
                    maintenance,
                    Some(order),
                )
            }
            Some(Err(PrivateServiceFailure::Uncollected {
                error,
                workers,
                uncollected,
                frontend,
                maintenance,
                order,
                ..
            })) => {
                let order = *order;
                drop(frontend);
                (
                    Some(false),
                    Some(format!("uncollected: {error:?}")),
                    workers,
                    uncollected,
                    maintenance,
                    Some(order),
                )
            }
            Some(Err(failure)) => (
                Some(false),
                Some(format!("{failure:?}")),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
            ),
            None => (None, None, Vec::new(), Vec::new(), Vec::new(), None),
        };
        let after = inspect_after(&service_owner, &service_durable);
        // What the store retained from instances that could not finish: an
        // unwound invocation's terminal inventory lands here.
        let store_holds = service_durable
            .inner
            .lock()
            .map(|held| {
                held.terminal
                    .iter()
                    .flat_map(|terminal| holds_of(Some(terminal)))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let execution_inventory_matches = service_durable.inner.lock().map(|held| {
            held.terminal.iter().all(|inventory| execution.resources_for_inventory(inventory).is_ok())
        }).unwrap_or(false);
        let execution_reading = execution.execution();
        let execution_collected = execution.resources.as_ref().is_some_and(|resources| resources.collected.is_some());
        drop(execution);
        let execution_abandoned = service_durable.retained_executions().expect("readable durable owner");
        let holds_after_execution_loss = service_durable.inner.lock().expect("readable inventory")
            .terminal.iter().flat_map(|terminal| holds_of(Some(terminal))).collect::<Vec<_>>();
        assert_eq!(store_holds, holds_after_execution_loss, "execution loss preserves unresolved debt");
        let _ = done.send(());
        ProducedOutcome {
            execution: execution_reading,
            execution_abandoned,
            execution_inventory_matches,
            execution_collected,
            unwound,
            ok,
            error,
            workers,
            uncollected,
            maintenance,
            order,
            after,
            retained_holds,
            store_holds,
            terminal,
            key_releases,
        }
    });
    let (registry, raster, controller) = registry_in
        .recv_timeout(Duration::from_secs(15))
        .expect("the launch built its frontend");
    (
        ProducingLaunch {
            handle,
            finished,
            registry,
            controller,
            raster,
            commands,
            transactions,
            acks,
            deliveries,
            access,
            owner,
        },
        socket_path,
    )
}

/// The setup handshake, keeping what the reply grants this connection: its
/// resource-id base, so a second connection's windows are its own.
fn handshake_ids(client: &mut UnixStream) -> u32 {
    use std::io::{Read, Write};
    client
        .write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .expect("the setup request is sent");
    let mut prefix = [0u8; 8];
    client.read_exact(&mut prefix).expect("a setup reply prefix");
    assert_eq!(prefix[0], 1, "setup succeeded");
    let mut body = vec![0u8; usize::from(u16::from_le_bytes([prefix[6], prefix[7]])) * 4];
    client.read_exact(&mut body).expect("the setup reply body");
    u32::from_le_bytes([body[4], body[5], body[6], body[7]])
}

/// CreateWindow selecting `event_mask`: value-mask CWEventMask alone, one
/// value. Parent is the setup's root, depth copied, 8x8 at the origin.
fn create_selecting_window(client: &mut UnixStream, window: u32, event_mask: u32) {
    use std::io::Write;
    let mut out: Vec<u8> = vec![1, 24];
    out.extend_from_slice(&9u16.to_le_bytes());
    out.extend_from_slice(&window.to_le_bytes());
    out.extend_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    out.extend_from_slice(&0i16.to_le_bytes());
    out.extend_from_slice(&0i16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(1u32 << 11).to_le_bytes());
    out.extend_from_slice(&event_mask.to_le_bytes());
    client.write_all(&out).expect("a CreateWindow request is sent");
}

fn map_window(client: &mut UnixStream, window: u32) {
    use std::io::Write;
    let mut out: Vec<u8> = vec![8, 0];
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&window.to_le_bytes());
    client.write_all(&out).expect("a MapWindow request is sent");
}

/// The surface the authority reported for the one draw this control made,
/// read from the observed batch that names it.
fn learn_drawn_surface(transactions: &Receiver<XAuthorityObservedTransactionBatch>) -> SurfaceId {
    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match transactions.recv_timeout(Duration::from_millis(50)) {
            Ok(batch) => {
                if batch.cpu_buffer_updates.len() == 1 && batch.transactions.len() == 1 {
                    return batch.transactions[0].surface;
                }
                seen.push((batch.cpu_buffer_updates.len(), batch.transactions.len()));
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    panic!("the draw is observed as one CPU buffer update; batches seen (cpu updates, transactions): {seen:?}")
}

/// A mapped 8x8 window selecting `event_mask`, drawn once so its surface is
/// known. Returns the window and the number of requests this sent after
/// setup, which is the sequence number the next event will carry.
fn selecting_window(
    client: &mut UnixStream,
    transactions: &Receiver<XAuthorityObservedTransactionBatch>,
    window: u32,
    event_mask: u32,
) -> (SurfaceId, u16) {
    let gc = window + 1;
    create_selecting_window(client, window, event_mask);
    map_window(client, window);
    create_gc(client, gc, window);
    image_text8(client, window, gc, b"AaZz");
    (learn_drawn_surface(transactions), 4)
}

/// Every completion the recovery ledger minted for this delivery.
fn delivery_cell(
    registry: &XServerFrontendRouteRegistry,
    delivery: u64,
) -> Option<Arc<PrivateDeliveryCompletion>> {
    registry
        .input_recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(delivery))
        .expect("a readable recovery")
}

/// One 32-byte core event, read whole within the bound.
fn read_event(client: &mut UnixStream, seconds: u64) -> Option<[u8; 32]> {
    read_within(client, 32, seconds).map(|bytes| {
        let mut event = [0u8; 32];
        event.copy_from_slice(&bytes);
        event
    })
}

/// The core ButtonPress/ButtonRelease event a request at the origin of an
/// 8x8 window mapped at the root's origin produces, hand-encoded from the X
/// protocol: type, detail (core button 1 for BTN_LEFT), the connection's own
/// sequence, the root, the event window, no child, coordinates 0, the button
/// state BEFORE the event (nothing for the press, Button1Mask for the
/// release of button 1), same screen. The time field is the request's time.
fn expected_button_event(
    pressed: bool,
    sequence: u16,
    window: u32,
    time_msec: u32,
) -> [u8; 32] {
    let mut event = [0u8; 32];
    event[0] = if pressed { 4 } else { 5 };
    event[1] = 1;
    event[2..4].copy_from_slice(&sequence.to_le_bytes());
    event[4..8].copy_from_slice(&time_msec.to_le_bytes());
    event[8..12].copy_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    event[12..16].copy_from_slice(&window.to_le_bytes());
    let state: u16 = if pressed { 0 } else { 1 << 8 };
    event[28..30].copy_from_slice(&state.to_le_bytes());
    event[30] = 1;
    event
}

/// A button event with an explicit core button and prior state.
fn expected_chord_event(pressed: bool, sequence: u16, window: u32, button: u8, state: u16) -> [u8; 32] {
    let mut event = expected_button_event(pressed, sequence, window, 1);
    event[1] = button;
    event[28..30].copy_from_slice(&state.to_le_bytes());
    event
}

fn produced_outcome(launched: ProducingLaunch, what: &str) -> ProducedOutcome {
    launch_outcome(launched.handle, &launched.finished, false, what)
}

/// The same, keeping the caller's side of the port to ask after the exit.
fn produced_outcome_keeping_access(
    launched: ProducingLaunch,
    what: &str,
) -> (ProducedOutcome, PrivateProducerAccess) {
    let ProducingLaunch {
        handle,
        finished,
        access,
        ..
    } = launched;
    (launch_outcome(handle, &finished, false, what), access)
}

/// The FocusIn a FocusSurface control's writer sends to a window selecting
/// FocusChange, hand-encoded: type 9, detail NotifyNonlinear (3), the
/// connection's sequence, the window, mode NotifyNormal (0).
fn expected_focus_in(sequence: u16, window: u32) -> [u8; 32] {
    let mut event = [0u8; 32];
    event[0] = 9;
    event[1] = 3;
    event[2..4].copy_from_slice(&sequence.to_le_bytes());
    event[4..8].copy_from_slice(&window.to_le_bytes());
    event[8] = 0;
    event
}

/// The acknowledgement the writer published for one control transaction,
/// read from the receiver the launch kept alive, within a bound.
fn ack_for(
    acks: &Receiver<XAuthorityClientControlAck>,
    transaction: u64,
) -> Option<XAuthorityClientControlAck> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match acks.recv_timeout(Duration::from_millis(50)) {
            Ok(ack) if ack.acknowledgement.transaction == TransactionId::from_raw(transaction) => {
                return Some(ack);
            }
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
    None
}

#[test]
fn focus_then_a_press_and_release_submitted_through_the_services_own_producers_reach_the_client_as_exact_events()
{
    let (launched, socket_path) = launch_producing("producer-press", 9601, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .expect("readiness is published once the runner is prepared and the listener bound");
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let window = 0x0020_0e01;
    let owner = Arc::clone(&launched.owner);
    let (surface, sequence) = selecting_window(
        &mut client,
        &launched.transactions,
        window,
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 21),
    );
    let custody = wait_attached(&launched.registry);
    let client_id = custody.cleanup_record().client;
    let lease = owner.lease();
    let control = launched
        .access
        .control_producer(&lease)
        .expect("the service issues its control producer");
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("the service issues an ingress for its admitted connection");
    // THE APPLIED STATE FIRST, THROUGH THE SAME ORDER: a FocusSurface control
    // is routed by the runner's turn to this connection's writer, which
    // applies it, publishes the applied focus and acknowledges. Routed is
    // not applied: the acknowledgement and the FocusIn on the wire are.
    let focus_position = control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(96001),
                    surface,
                },
            },
        )
        .expect("the order accepts the control");
    let focus_ack = ack_for(&launched.acks, 96001);
    let focus_in = read_event(&mut client, 5);
    // THE PRESS, THROUGH THE REAL PRODUCER, SERVED BY A LATER TURN.
    let press_position = ingress
        .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(96010), 272, true))
        .expect("the order accepts the press");
    let press = read_event(&mut client, 5);
    let press_cell = delivery_cell(&launched.registry, 96010);
    let press_diagnostic = (
        press_cell.as_ref().and_then(|cell| cell.answer()),
        launched.deliveries.try_iter().collect::<Vec<_>>(),
        launched.registry.occupancy.state_of(client_id),
    );
    // THE RELEASE, IN A LATER TURN STILL, RESOLVED AGAINST THE HELD PRESS:
    // the runner's continuing history is what turns it into a delivery.
    let release_submitted = ingress
        .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(96011), 272, false))
        .map_err(|refusal| format!("{refusal:?}"));
    let release = read_event(&mut client, 5);
    let release_cell = delivery_cell(&launched.registry, 96011);
    // THE HISTORY ACROSS TURNS, AGAIN WITH ANOTHER BUTTON: button 2
    // (BTN_MIDDLE, evdev 274) goes down in one turn and comes up in a later
    // one; the release's state field names the button the one continuing
    // history still held before it, which the release's own turn could not
    // know. (A release of one button while another is held is not delivered
    // by the native hold path today and is not asked of it here.)
    let mut chord = Vec::new();
    for (delivery, button, pressed) in [(96020, 274, true), (96021, 274, false)] {
        let submitted = ingress
            .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(delivery), button, pressed))
            .map_err(|refusal| format!("{refusal:?}"));
        let event = read_event(&mut client, 5);
        chord.push((submitted.map(|_| ()), event));
    }
    let press_answer = press_cell.as_ref().and_then(|cell| {
        waited_for(|| cell.answer().is_some());
        cell.answer()
    });
    let release_answer = release_cell.as_ref().and_then(|cell| {
        waited_for(|| cell.answer().is_some());
        cell.answer()
    });
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let registry = launched.registry.clone();
    let (outcome, access) = produced_outcome_keeping_access(launched, "producer press");
    let seen = observe_worker(&custody, &registry);
    let diagnostic = format!(
        "focus ack {focus_ack:?}; focus_in {focus_in:?}; press event {press:?}; press diagnostic {press_diagnostic:?}; order {:?}; worker {seen:?}; error {:?}",
        outcome.order, outcome.error
    );
    assert_eq!(
        focus_ack.map(|ack| (ack.client, ack.acknowledgement.outcome)),
        Some((client_id, XAuthorityControlOutcome::Delivered)),
        "the writer applied and acknowledged the focus: {diagnostic}"
    );
    assert_eq!(
        focus_in,
        Some(expected_focus_in(sequence, window)),
        "the exact FocusIn, before any input: {diagnostic}"
    );
    let release_position = release_submitted
        .unwrap_or_else(|refusal| panic!("the order accepts the release: {refusal}; {diagnostic}"));
    assert!(
        focus_position < press_position && press_position < release_position,
        "one order, in submission order"
    );
    assert_eq!(
        press,
        Some(expected_button_event(true, sequence, window, 1)),
        "the exact ButtonPress: {diagnostic}"
    );
    assert_eq!(
        release,
        Some(expected_button_event(false, sequence, window, 1)),
        "the exact ButtonRelease: {release:?}"
    );
    assert_eq!(
        press_answer.map(|answer| (answer.delivery, answer.outcome)),
        Some((XAuthorityInputDeliveryId::from_raw(96010), XAuthorityInputDeliveryOutcome::Flushed)),
        "the press is answered for through its own completion"
    );
    assert_eq!(
        release_answer.map(|answer| (answer.delivery, answer.outcome)),
        Some((XAuthorityInputDeliveryId::from_raw(96011), XAuthorityInputDeliveryOutcome::Flushed)),
        "and so is the release"
    );
    let expected_chord = [
        expected_chord_event(true, sequence, window, 2, 0),
        expected_chord_event(false, sequence, window, 2, 1 << 9),
    ];
    for (index, ((submitted, event), expected)) in chord.iter().zip(expected_chord.iter()).enumerate() {
        assert_eq!(submitted, &Ok(()), "chord event {index} accepted: {diagnostic}");
        assert_eq!(event, &Some(*expected), "the exact chord event {index}: {diagnostic}");
    }
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    let order = outcome.order.expect("the return carries the tally");
    assert_eq!(order.taken, 5, "the runner took the control and the four inputs: {order:?}");
    assert_eq!(order.dispatched, 4, "four capsules were handed to this connection's queue: {order:?}");
    assert_eq!(order.refused, 0, "{order:?}");
    assert_eq!(order.routed, 1, "the one control was routed from the order: {order:?}");
    assert!(order.turns >= 3, "served over turns, not drained: {order:?}");
    assert_eq!(order.producers_issued, 2);
    assert_eq!(order.producers_refused, 0);
    assert_eq!(outcome.workers.len(), 1);
    assert!(outcome.uncollected.is_empty() && !outcome.unwound);
    assert_eq!(outcome.maintenance.len(), 1, "one deferred cleanup visit reported");
    assert!(outcome.maintenance[0].result.is_ok(), "{:?}", outcome.maintenance[0]);
    assert_eq!(outcome.after.custodies_kept, 1, "the custody stays with the owner");
    assert_collected_running(&seen, "producer press");
    // AFTER THE EXIT: the port has ended, and the producers already issued
    // refuse at acceptance -- the runner's admission closed before anything
    // was waited for.
    assert_eq!(access.standing(), PrivatePortStanding::Ended);
    assert_eq!(access.control_producer(&lease).err(), Some(PrivateProducerRefusal::Ended));
    assert!(
        ingress
            .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(96030), 272, true))
            .is_err(),
        "an issued ingress refuses after the service closed its admission"
    );
    assert!(
        control
            .submit(
                &lease,
                XAuthorityClientControlCommand {
                    client: client_id,
                    command: XAuthorityControlCommand::ClearFocus {
                        transaction: TransactionId::from_raw(96002),
                        surface,
                    },
                },
            )
            .is_err(),
        "and so does an issued control producer"
    );
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn a_press_before_applied_focus_is_answered_refused_and_a_fresh_press_can_follow_focus() {
    // THE PENDING/APPLIED DISTINCTION, FROM THE INPUT SIDE: nothing has
    // published this connection's applied state, so the runner's execution
    // refuses the press (Unpublished) rather than delivering it somewhere
    // plausible. The actual refusal answers the original receipt, allowing
    // a fresh request after focus; applying focus never replays the old one.
    // Final observations follow ordinary stop and worker collection.
    let (launched, socket_path) = launch_producing("producer-unpublished", 9602, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .expect("readiness");
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let window = 0x0020_0e11;
    let (surface, sequence) = selecting_window(
        &mut client,
        &launched.transactions,
        window,
        (1 << 2) | (1 << 3) | (1 << 21),
    );
    let custody = wait_attached(&launched.registry);
    let client_id = custody.cleanup_record().client;
    let lease = launched.owner.lease();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("an ingress");
    let control = launched.access.control_producer(&lease).expect("the control producer");
    ingress
        .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(96110), 272, true))
        .expect("the order accepts the press");
    let nothing = read_event(&mut client, 2);
    let cell = delivery_cell(&launched.registry, 96110);
    // The applied state arrives afterwards; the refused press is not replayed.
    control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(96101),
                    surface,
                },
            },
        )
        .expect("the order accepts the control");
    let focus_ack = ack_for(&launched.acks, 96101);
    let focus_in = read_event(&mut client, 5);
    let still_nothing = read_event(&mut client, 2);
    let answered_after_focus = cell.as_ref().and_then(|cell| cell.answer());
    let fresh = ingress
        .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(96111), 272, true))
        .map_err(|refusal| format!("{refusal:?}"));
    let new_press = read_event(&mut client, 3);
    let fresh_answer = delivery_cell(&launched.registry, 96111).and_then(|cell| {
        waited_for(|| cell.answer().is_some());
        cell.answer()
    });
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "unpublished press");
    let seen = observe_worker(&custody, &registry);
    let order = outcome.order.expect("the tally");
    assert_eq!(nothing, None, "nothing was delivered for the refused press");
    assert!(fresh.is_ok(), "the answered refusal allows a fresh request: {fresh:?}");
    assert_eq!(
        focus_ack.map(|ack| ack.acknowledgement.outcome),
        Some(XAuthorityControlOutcome::Delivered)
    );
    assert_eq!(focus_in, Some(expected_focus_in(sequence, window)));
    assert_eq!(still_nothing, None, "the applied focus replays nothing");
    assert_eq!(answered_after_focus.map(|receipt| receipt.outcome), Some(XAuthorityInputDeliveryOutcome::RouteRejected));
    assert_eq!(new_press, Some(expected_button_event(true, sequence, window, 1)), "fresh={fresh_answer:?}; order={order:?}; error={:?}; terminal={:?}", outcome.error, outcome.terminal);
    assert_eq!(fresh_answer.map(|receipt| receipt.outcome), Some(XAuthorityInputDeliveryOutcome::Flushed));
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert_eq!(order.taken, 3, "two presses and the control: {order:?}");
    assert_eq!(order.refused, 1, "{order:?}");
    assert_eq!(
        order.last_refusal,
        Some(PrivateExecutionRefusal::Native(private_native::Refusal::Resolution(
            PrivateAppliedRefusal::Unpublished
        ))),
        "{order:?}"
    );
    assert_eq!(order.routed, 1);
    assert_collected_running(&seen, "unpublished press");
    let _ = std::fs::remove_file(&socket_path);
}
