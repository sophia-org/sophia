pub(crate) fn run_x_authority_xterm_input_smoke()
-> Result<XAuthorityXtermInputSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("xterm", "xterm")?;
    let (display, socket_path) = temp_xauthority_display(150)?;
    let input_result = XtermInputResultFile {
        path: std::env::temp_dir().join(format!(
            "sophia-xterm-input-{}-{}",
            std::process::id(),
            display.trim_start_matches(':')
        )),
    };
    let _result_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&input_result.path)?;
    let server_path = socket_path.clone();
    let (transaction_sender, transaction_receiver) = sync_channel(256);
    let (input_sender, input_receiver) = sync_channel(32);
    let (control_sender, control_receiver) = sync_channel(8);
    let (control_ack_sender, control_ack_receiver) = sync_channel(8);
    let server = std::thread::spawn(move || {
        run_x11_core_socket_server_once_session_channels(
            &server_path,
            NamespaceId::from_raw(49),
            transaction_sender,
            input_receiver,
            control_receiver,
            control_ack_sender,
        )
    });
    wait_for_socket_path(&socket_path)?;

    let mut child = std::process::Command::new(command)
        .env("DISPLAY", &display)
        .args([
            "-cm",
            "-dc",
            "-geometry",
            "40x8",
            "-e",
            "sh",
            "-c",
            "printf 'type sophia then Return: '; read line; umask 077; printf '%s' \"$line\" > \"$1\"; printf 'received:%s\\n' \"$line\"; sleep 3",
            "sophia-xterm-input",
        ])
        .arg(&input_result.path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let mut cpu_buffers = std::collections::BTreeMap::new();
    let mut route = None;
    let initial = wait_for_xterm_cpu_state(
        &transaction_receiver,
        &mut child,
        std::time::Instant::now() + Duration::from_secs(6),
        None,
        &mut cpu_buffers,
        &mut route,
    )?;
    let (client, surface) = route.ok_or("xterm input smoke did not observe a client surface route")?;
    focus_xterm_client(
        &control_sender,
        &control_ack_receiver,
        client,
        surface,
        1,
    )?;
    let mut time_msec = 1u32;
    for keycode in b"sophia"
        .iter()
        .copied()
        .map(x11_keycode_for_ascii)
        .chain(std::iter::once(Some(36)))
    {
        let keycode = keycode.ok_or("input smoke character has no X keycode")?;
        for pressed in [true, false] {
            input_sender.send(XAuthorityClientInputEvent {
                client,
                event: XAuthorityKeyEvent {
                    keycode,
                    pressed,
                    state: 0,
                    modifiers_after: 0,
                    time_msec,
                }
                .into(),
                target_window: None,
                xi_event_type: None,
                xi_event_window: None,
                xi_emulated_button_type: None,
                xi_emulated_button_window: None,
                xi_pointer_crossing_mask: 0,
                grab_crossing: None,
                grab_target: None,
                delivery: None,
            })?;
            time_msec = time_msec.saturating_add(1);
        }
    }
    let final_state = wait_for_xterm_cpu_state(
        &transaction_receiver,
        &mut child,
        std::time::Instant::now() + Duration::from_secs(4),
        Some(initial),
        &mut cpu_buffers,
        &mut route,
    );

    if child.try_wait()?.is_none() {
        let _ = child.kill();
    }
    let output = child.wait_with_output()?;
    drop(input_sender);
    drop(control_sender);
    let server_result = server
        .join()
        .map_err(|_| "X authority xterm input server thread panicked")?;
    let _ = std::fs::remove_file(&socket_path);
    server_result.map_err(|error| format!("X authority xterm input server failed: {error}"))?;
    let final_state = final_state.map_err(|error| {
        format!(
            "{error}; xterm_status={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })?;
    let received = std::fs::read(&input_result.path)?;
    if received != b"sophia" {
        return Err(format!(
            "xterm input smoke received incorrect terminal bytes: expected=6 received={}",
            received.len(),
        )
        .into());
    }

    Ok(XAuthorityXtermInputSmokeReport {
        display,
        keys: 7,
        initial_generation: initial.0,
        final_generation: final_state.0,
        initial_checksum: initial.1,
        final_checksum: final_state.1,
        text_match: true,
    })
}

/// Launches two independent real xterms against the bounded routed frontend.
///
/// Each terminal receives different Engine-addressed keystrokes only after the
/// authority has observed both clients' surface routes. This is intentionally a
/// compatibility proof, not the normal persistent-session launcher.
pub(crate) fn run_x_authority_xterm_two_client_smoke()
-> Result<XAuthorityXtermTwoClientSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("xterm", "xterm")?;
    let (display, socket_path) = temp_xauthority_display(152)?;
    let server_path = socket_path.clone();
    let (transaction_sender, transaction_receiver) = sync_channel(256);
    let (control_ack_sender, control_ack_receiver) = sync_channel(64);
    let (input_delivery_sender, input_delivery_receiver) = channel();
    let broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(64).unwrap(),
        control_ack_sender,
        input_delivery_sender,
    );
    // Raw ingress is refusable, and these smokes drive an ungated broker, so
    // a refusal here means the harness is not the shape this test assumes.
    let input_sender = broker
        .input_sender()
        .map_err(|refusal| format!("raw X11 ingress refused: {refusal:?}"))?;
    let control_sender = broker.control_sender();
    let (service_command_sender, service_command_receiver) = sync_channel(1);
    let config = XServerFrontendConfig::new(&server_path, NamespaceId::from_raw(53))?
        .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
    let server = std::thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(
            config,
            transaction_sender,
            broker,
            service_command_receiver,
        )
    });
    wait_for_socket_path(&socket_path)?;

    let mut first = spawn_xterm_two_client_probe(&command, &display, "first", "40x8+40+40")?;
    let mut second = spawn_xterm_two_client_probe(&command, &display, "second", "40x8+420+40")?;
    let mut state = XtermTwoClientState::default();
    let result = (|| {
        wait_for_two_xterm_routes(
            &transaction_receiver,
            &mut first,
            &mut second,
            std::time::Instant::now() + Duration::from_secs(8),
            &mut state,
        )?;
        let initial = state.fingerprint();
        let clients = state.clients.iter().copied().collect::<Vec<_>>();
        if clients.len() != 2 {
            return Err(format!(
                "two-client xterm smoke observed {} routed clients",
                clients.len()
            )
            .into());
        }

        let mut time_msec = 1u32;
        let mut next_delivery = 1u64;
        focus_xterm_client(
            &control_sender,
            &control_ack_receiver,
            clients[0],
            state.surface_for_client(clients[0])?,
            1,
        )?;
        let first_deliveries = send_xterm_text_to_client(
            &input_sender,
            clients[0],
            b"alpha",
            &mut time_msec,
            &mut next_delivery,
        )?;
        wait_for_xterm_input_deliveries(&input_delivery_receiver, &first_deliveries)?;
        let after_first = wait_for_two_xterm_change(
            &transaction_receiver,
            &mut first,
            &mut second,
            std::time::Instant::now() + Duration::from_secs(5),
            initial,
            &mut state,
        )?;
        focus_xterm_client(
            &control_sender,
            &control_ack_receiver,
            clients[1],
            state.surface_for_client(clients[1])?,
            2,
        )?;
        let second_deliveries = send_xterm_text_to_client(
            &input_sender,
            clients[1],
            b"bravo",
            &mut time_msec,
            &mut next_delivery,
        )?;
        wait_for_xterm_input_deliveries(&input_delivery_receiver, &second_deliveries)?;
        let final_state = wait_for_two_xterm_change(
            &transaction_receiver,
            &mut first,
            &mut second,
            std::time::Instant::now() + Duration::from_secs(5),
            after_first,
            &mut state,
        )?;

        Ok(XAuthorityXtermTwoClientSmokeReport {
            display: display.clone(),
            clients: clients.len(),
            routed_keys: first_deliveries.len() + second_deliveries.len(),
            initial_generation: initial.0,
            final_generation: final_state.0,
            initial_checksum: initial.1,
            final_checksum: final_state.1,
        })
    })();

    let first_output = stop_xterm_two_client_probe(&mut first)?;
    let second_output = stop_xterm_two_client_probe(&mut second)?;
    let _ = service_command_sender.send(XServerFrontendServiceCommand::StopAccepting);
    drop(service_command_sender);
    drop(input_sender);
    let server_result = server
        .join()
        .map_err(|_| "two-client xterm authority server thread panicked")?;
    let _ = std::fs::remove_file(&socket_path);
    server_result.map_err(|error| format!("two-client X authority server failed: {error}"))?;
    result.map_err(|error: Box<dyn std::error::Error>| {
        format!("{error}; first_status={first_output} second_status={second_output}",).into()
    })
}

#[derive(Default)]
struct XtermTwoClientState {
    routes: XAuthorityClientSurfaceRoutes,
    clients: BTreeSet<sophia_x_authority::XServerFrontendClientId>,
    surfaces: BTreeMap<sophia_x_authority::XServerFrontendClientId, SurfaceId>,
    buffers: BTreeMap<u64, XAuthorityCpuBufferSnapshot>,
}

impl XtermTwoClientState {
    fn observe(
        &mut self,
        batch: XAuthorityObservedTransactionBatch,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = batch.client
            && (!batch.transactions.is_empty() || !batch.cpu_buffer_updates.is_empty())
        {
            self.clients.insert(client);
            for transaction in &batch.transactions {
                self.surfaces.insert(client, transaction.surface);
            }
            for intent in &batch.presentation_intents {
                self.surfaces.insert(client, intent.surface);
            }
        }
        self.routes.observe(&batch)?;
        for update in batch.cpu_buffer_updates {
            update.apply_to(&mut self.buffers)?;
        }
        Ok(())
    }

    fn has_two_live_cpu_routes(&self) -> bool {
        self.clients.len() == 2
            && self.routes.len() >= 2
            && self
                .buffers
                .values()
                .filter(|buffer| buffer.bytes.iter().any(|byte| *byte != 0))
                .count()
                >= 2
    }

    fn fingerprint(&self) -> (u64, u64) {
        let generation = self
            .buffers
            .values()
            .map(|buffer| buffer.generation)
            .max()
            .unwrap_or(0);
        let checksum = self
            .buffers
            .values()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, buffer| {
                buffer.bytes.iter().fold(hash, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
                })
            });
        (generation, checksum)
    }

    fn surface_for_client(
        &self,
        client: sophia_x_authority::XServerFrontendClientId,
    ) -> Result<SurfaceId, Box<dyn std::error::Error>> {
        self.surfaces
            .get(&client)
            .copied()
            .ok_or_else(|| {
                format!(
                    "two-client xterm route has no surface for client {}",
                    client.raw()
                )
                .into()
            })
    }
}

fn focus_xterm_client(
    sender: &std::sync::mpsc::SyncSender<XAuthorityClientControlCommand>,
    acknowledgements: &std::sync::mpsc::Receiver<sophia_x_authority::XAuthorityClientControlAck>,
    client: sophia_x_authority::XServerFrontendClientId,
    surface: SurfaceId,
    transaction: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let transaction = TransactionId::from_raw(transaction);
    sender.send(XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction,
            surface,
        },
    })?;
    let acknowledgement = acknowledgements.recv_timeout(Duration::from_secs(2))?;
    if acknowledgement.client != client
        || acknowledgement.acknowledgement.transaction != transaction
        || acknowledgement.acknowledgement.surface != surface
        || acknowledgement.acknowledgement.outcome != XAuthorityControlOutcome::Delivered
    {
        return Err("two-client xterm focus control was not delivered".into());
    }
    Ok(())
}

fn spawn_xterm_two_client_probe(
    command: &std::path::Path,
    display: &str,
    label: &str,
    geometry: &str,
) -> Result<std::process::Child, Box<dyn std::error::Error>> {
    let script = format!(
        "printf '{label}: '; IFS= read -r line; printf '{label}-received:%s\\n' \"$line\"; sleep 3"
    );
    Ok(std::process::Command::new(command)
        .env("DISPLAY", display)
        .args(["-cm", "-dc", "-geometry", geometry, "-e", "sh", "-c"])
        .arg(script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?)
}

fn stop_xterm_two_client_probe(
    child: &mut std::process::Child,
) -> Result<std::process::ExitStatus, Box<dyn std::error::Error>> {
    if child.try_wait()?.is_none() {
        let _ = child.kill();
    }
    Ok(child.wait()?)
}

fn wait_for_two_xterm_routes(
    receiver: &std::sync::mpsc::Receiver<XAuthorityObservedTransactionBatch>,
    first: &mut std::process::Child,
    second: &mut std::process::Child,
    deadline: std::time::Instant,
    state: &mut XtermTwoClientState,
) -> Result<(), Box<dyn std::error::Error>> {
    while std::time::Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(batch) => {
                state.observe(batch)?;
                if state.has_two_live_cpu_routes() {
                    return Ok(());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("two-client xterm transaction channel disconnected".into());
            }
        }
        ensure_xterm_two_client_alive(first, "first")?;
        ensure_xterm_two_client_alive(second, "second")?;
    }
    Err(format!(
        "timed out waiting for two routed xterm CPU surfaces: clients={} routes={} buffers={}",
        state.clients.len(),
        state.routes.len(),
        state.buffers.len(),
    )
    .into())
}

fn wait_for_two_xterm_change(
    receiver: &std::sync::mpsc::Receiver<XAuthorityObservedTransactionBatch>,
    first: &mut std::process::Child,
    second: &mut std::process::Child,
    deadline: std::time::Instant,
    previous: (u64, u64),
    state: &mut XtermTwoClientState,
) -> Result<(u64, u64), Box<dyn std::error::Error>> {
    while std::time::Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(batch) => {
                state.observe(batch)?;
                let current = state.fingerprint();
                if current.0 > previous.0 && current.1 != previous.1 {
                    return Ok(current);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("two-client xterm transaction channel disconnected".into());
            }
        }
        ensure_xterm_two_client_alive(first, "first")?;
        ensure_xterm_two_client_alive(second, "second")?;
    }
    Err("timed out waiting for targeted xterm input to change CPU pixels".into())
}

fn ensure_xterm_two_client_alive(
    child: &mut std::process::Child,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(status) = child.try_wait()? {
        return Err(format!("{label} xterm exited before two-client pixel proof: {status}").into());
    }
    Ok(())
}

fn send_xterm_text_to_client(
    sender: &std::sync::mpsc::SyncSender<XAuthorityClientInputEvent>,
    client: sophia_x_authority::XServerFrontendClientId,
    text: &[u8],
    time_msec: &mut u32,
    next_delivery: &mut u64,
) -> Result<Vec<XAuthorityInputDeliveryId>, Box<dyn std::error::Error>> {
    let mut deliveries = Vec::new();
    for byte in text.iter().copied().chain(std::iter::once(b'\n')) {
        let events = match byte {
            b':' => vec![
                (50, true, 0, 1),
                (47, true, 1, 1),
                (47, false, 1, 1),
                (50, false, 1, 0),
            ],
            b'\n' => vec![(36, true, 0, 0), (36, false, 0, 0)],
            _ => {
                let keycode = x11_keycode_for_ascii(byte)
                    .ok_or("two-client input smoke character has no X keycode")?;
                vec![(keycode, true, 0, 0), (keycode, false, 0, 0)]
            }
        };
        for (keycode, pressed, state, modifiers_after) in events {
            let delivery = XAuthorityInputDeliveryId::from_raw(*next_delivery);
            *next_delivery = next_delivery
                .checked_add(1)
                .ok_or("two-client xterm smoke exhausted input delivery IDs")?;
            sender.send(XAuthorityClientInputEvent {
                client,
                event: XAuthorityKeyEvent {
                    keycode,
                    pressed,
                    state,
                    modifiers_after,
                    time_msec: *time_msec,
                }
                .into(),
                target_window: None,
                xi_event_type: None,
                xi_event_window: None,
                xi_emulated_button_type: None,
                xi_emulated_button_window: None,
                xi_pointer_crossing_mask: 0,
                grab_crossing: None,
                grab_target: None,
                delivery: Some(delivery),
            })?;
            *time_msec = time_msec.saturating_add(1);
            deliveries.push(delivery);
        }
    }
    Ok(deliveries)
}

fn wait_for_xterm_input_deliveries(
    receiver: &std::sync::mpsc::Receiver<sophia_x_authority::XAuthorityClientInputDelivery>,
    deliveries: &[XAuthorityInputDeliveryId],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pending = deliveries.iter().copied().collect::<BTreeSet<_>>();
    // GL clients can expose their first Present before their event loop
    // installs the core keyboard mask. The frontend keeps those startup keys
    // boundedly pending for up to five seconds, so the proof acknowledgement
    // must cover that same readiness window.
    let deadline = std::time::Instant::now() + Duration::from_secs(7);
    while !pending.is_empty() && std::time::Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(delivery) if pending.remove(&delivery.delivery) => {
                if delivery.outcome != XAuthorityInputDeliveryOutcome::Flushed {
                    return Err(format!(
                        "two-client xterm input delivery failed for client {}: {:?}",
                        delivery.client.raw(),
                        delivery.outcome,
                    )
                    .into());
                }
            }
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("two-client xterm input delivery channel disconnected".into());
            }
        }
    }
    if pending.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "two-client xterm input delivery timed out with {} pending events",
            pending.len(),
        )
        .into())
    }
}

fn wait_for_xterm_cpu_state(
    receiver: &std::sync::mpsc::Receiver<XAuthorityObservedTransactionBatch>,
    child: &mut std::process::Child,
    deadline: std::time::Instant,
    previous: Option<(u64, u64)>,
    latest: &mut std::collections::BTreeMap<u64, XAuthorityCpuBufferSnapshot>,
    route: &mut Option<(sophia_x_authority::XServerFrontendClientId, SurfaceId)>,
) -> Result<(u64, u64), Box<dyn std::error::Error>> {
    let mut candidate = None;
    while std::time::Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(batch) => {
                if let Some(client) = batch.client {
                    for transaction in &batch.transactions {
                        route.get_or_insert((client, transaction.surface));
                    }
                    for intent in &batch.presentation_intents {
                        route.get_or_insert((client, intent.surface));
                    }
                }
                for update in batch.cpu_buffer_updates {
                    update.apply_to(latest)?;
                }
                let generation = latest
                    .values()
                    .map(|buffer| buffer.generation)
                    .max()
                    .unwrap_or(0);
                let checksum = latest
                    .values()
                    .fold(0xcbf2_9ce4_8422_2325u64, |hash, buffer| {
                        buffer.bytes.iter().fold(hash, |hash, byte| {
                            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
                        })
                    });
                let has_pixels = latest
                    .values()
                    .any(|buffer| buffer.bytes.iter().any(|byte| *byte != 0));
                if has_pixels {
                    candidate = Some((generation, checksum));
                    if previous.is_some_and(|(old_generation, old_checksum)| {
                        generation > old_generation && checksum != old_checksum
                    }) {
                        return Ok((generation, checksum));
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if previous.is_none()
                    && let Some(candidate) = candidate
                {
                    return Ok(candidate);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("X authority xterm input transaction channel disconnected".into());
            }
        }
        if let Some(status) = child.try_wait()? {
            return Err(format!("xterm input client exited before pixel proof: {status}").into());
        }
    }
    Err("timed out waiting for xterm input to change CPU pixels".into())
}

pub(crate) fn x11_keycode_for_ascii(byte: u8) -> Option<u8> {
    sophia_session::support::x11_keycode_for_ascii(byte)
}
