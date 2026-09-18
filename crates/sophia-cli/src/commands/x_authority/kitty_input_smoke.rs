fn run_x_authority_kitty_input_smoke()
-> Result<XAuthorityKittyInputSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("kitty", "kitty")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider {
        device: first_openable_render_node()?,
    });
    let (display, socket_path) = temp_xauthority_display(6692)?;
    let result_file = XtermInputResultFile {
        path: std::env::temp_dir().join(format!(
            "sophia-kitty-input-{}-{}",
            std::process::id(),
            display.trim_start_matches(':')
        )),
    };
    let _result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&result_file.path)?;

    let (transaction_sender, transaction_receiver) = sync_channel(4_096);
    let (control_ack_sender, control_ack_receiver) = sync_channel(64);
    let (input_delivery_sender, input_delivery_receiver) = channel();
    let broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(64).expect("Kitty input smoke route capacity is nonzero"),
        control_ack_sender,
        input_delivery_sender,
    );
    // Raw ingress is refusable, and these smokes drive an ungated broker, so
    // a refusal here means the harness is not the shape this test assumes.
    let input_sender = broker
        .input_sender()
        .map_err(|refusal| format!("raw X11 ingress refused: {refusal:?}"))?;
    let control_sender = broker.control_sender();
    let protocol_router = broker.protocol_router();
    let (service_sender, service_receiver) = sync_channel(1);
    let defer_policy_map = std::env::var_os("SOPHIA_KITTY_SMOKE_DEFER_POLICY_MAP").is_some();
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(63))?
        .with_render_device_provider(provider)
        .with_policy_map_deferred(defer_policy_map);
    let server = std::thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(
            config,
            transaction_sender,
            broker,
            service_receiver,
        )
    });
    wait_for_socket_path(&socket_path)?;

    let mut child = std::process::Command::new(command)
        .env("DISPLAY", &display)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null")
        .env_remove("WAYLAND_DISPLAY")
        .args([
            "--config",
            "NONE",
            "--override",
            "close_on_child_death=yes",
            "--override",
            "linux_display_server=x11",
            "--override",
            "sync_to_monitor=no",
            "--debug-keyboard",
            "--title",
            "Sophia Kitty input proof",
            "sh",
            "-c",
            "printf 'type :ll then Return: '; IFS= read -r line; umask 077; printf '%s' \"$line\" > \"$1\"; sleep 2",
            "sophia-kitty-input",
        ])
        .arg(&result_file.path)
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = (|| {
        let deadline = std::time::Instant::now() + Duration::from_secs(12);
        let mut client = None;
        let mut surface = None;
        let mut present_before_input = 0usize;
        let presentation_started = Instant::now();
        let mut pending_present_feedback = None;
        let mut present_fences = BTreeMap::new();
        let mut admission_delivered = false;
        while std::time::Instant::now() < deadline
            && (client.is_none() || surface.is_none() || present_before_input < 2)
        {
            let batch = match transaction_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(batch) => batch,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    ensure_kitty_input_client_alive(&mut child)?;
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Kitty input transaction channel disconnected".into());
                }
            };
            if let Some(candidate) = batch.client {
                client = Some(candidate);
            }
            if defer_policy_map {
                for presentation in &batch.surface_presentations {
                    eprintln!(
                        "sophia_kitty_input_smoke schema=1 stage=surface_observed surface={} role={:?} mapped={}",
                        presentation.surface.index(),
                        presentation.role,
                        presentation.mapped,
                    );
                }
            }
            for intent in &batch.presentation_intents {
                if !defer_policy_map
                    || admission_delivered
                    || intent.kind
                        != sophia_protocol::SurfacePresentationIntentKind::Request
                {
                    continue;
                }
                let admitted_client =
                    client.ok_or("deferred Kitty map intent omitted its routed client")?;
                let admission_transaction = TransactionId::from_raw(9_000_000);
                control_sender.send(XAuthorityClientControlCommand {
                    client: admitted_client,
                    command: XAuthorityControlCommand::AdmitSurface {
                        transaction: admission_transaction,
                        surface: intent.surface,
                        geometry: intent.geometry,
                    },
                })?;
                let acknowledgement =
                    control_ack_receiver.recv_timeout(Duration::from_secs(2))?;
                if acknowledgement.client != admitted_client
                    || acknowledgement.acknowledgement.transaction != admission_transaction
                    || acknowledgement.acknowledgement.surface != intent.surface
                    || acknowledgement.acknowledgement.outcome
                        != XAuthorityControlOutcome::Delivered
                {
                    return Err("deferred Kitty map admission was not delivered".into());
                }
                admission_delivered = true;
                eprintln!(
                    "sophia_kitty_input_smoke schema=1 stage=map_admitted surface={}",
                    intent.surface.index(),
                );
            }
            for registration in batch.fence_registrations {
                present_fences.insert(registration.handle, registration.fd);
            }
            for released in batch.released_fences {
                present_fences.remove(&released);
            }
            if let Some(transaction) = batch.transactions.first() {
                surface = Some(transaction.surface);
            }
            for submission in batch.present_submissions {
                surface = Some(submission.surface);
                present_before_input = present_before_input.saturating_add(1);
                if present_before_input == 1 {
                    route_external_present_feedback(
                        "Kitty input smoke",
                        &protocol_router,
                        submission.transaction,
                        submission.idle_fence,
                        &present_fences,
                        presentation_started,
                    )?;
                } else {
                    pending_present_feedback =
                        Some((submission.transaction, submission.idle_fence));
                }
            }
            ensure_kitty_input_client_alive(&mut child)?;
        }
        let client = client.ok_or("Kitty input smoke observed no routed X11 client")?;
        let surface = surface.ok_or("Kitty input smoke observed no mapped surface")?;
        if defer_policy_map && !admission_delivered {
            return Err("Kitty input smoke observed no deferred map intent".into());
        }
        let keymap = run_xkbcommon_x11_probe(&display)?;
        let keymap_text = String::from_utf8_lossy(&keymap.stdout);
        eprintln!(
            "sophia_kitty_input_smoke schema=1 stage=xkb_snapshot status={} bytes={} has_l={} stderr={}",
            keymap.status,
            keymap.stdout.len(),
            keymap_text.contains("keysym=l"),
            String::from_utf8_lossy(&keymap.stderr).trim(),
        );
        eprintln!(
            "sophia_kitty_input_smoke schema=1 stage=present_ready client={} surface={} presents={present_before_input}",
            client.raw(),
            surface.index(),
        );
        let (transaction, idle_fence) = pending_present_feedback
            .take()
            .ok_or("Kitty input smoke omitted its second Present")?;
        route_external_present_feedback(
            "Kitty input smoke",
            &protocol_router,
            transaction,
            idle_fence,
            &present_fences,
            presentation_started,
        )?;

        // Two buffers prove GLX/DRI3 startup, but not that Kitty has finished
        // installing its terminal-window callbacks. Keep servicing Present
        // during a bounded readiness interval before injecting the proof.
        let readiness_deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < readiness_deadline {
            match transaction_receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(batch) => {
                    for registration in batch.fence_registrations {
                        present_fences.insert(registration.handle, registration.fd);
                    }
                    for released in batch.released_fences {
                        present_fences.remove(&released);
                    }
                    for submission in batch.present_submissions {
                        present_before_input = present_before_input.saturating_add(1);
                        route_external_present_feedback(
                            "Kitty input smoke",
                            &protocol_router,
                            submission.transaction,
                            submission.idle_fence,
                            &present_fences,
                            presentation_started,
                        )?;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Kitty input transaction channel disconnected".into());
                }
            }
            ensure_kitty_input_client_alive(&mut child)?;
        }
        eprintln!(
            "sophia_kitty_input_smoke schema=1 stage=input_ready presents={present_before_input}"
        );

        let focus_transaction = TransactionId::from_raw(9_000_001);
        control_sender.send(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: focus_transaction,
                surface,
            },
        })?;
        let acknowledgement = control_ack_receiver.recv_timeout(Duration::from_secs(2))?;
        if acknowledgement.client != client
            || acknowledgement.acknowledgement.transaction != focus_transaction
            || acknowledgement.acknowledgement.surface != surface
            || acknowledgement.acknowledgement.outcome != XAuthorityControlOutcome::Delivered
        {
            return Err("Kitty input smoke focus control was not delivered".into());
        }
        eprintln!("sophia_kitty_input_smoke schema=1 stage=focus_delivered");

        let focus_settle_deadline = Instant::now() + Duration::from_secs(3);
        let mut last_focus_present = Instant::now();
        let mut focus_presents = 0usize;
        while Instant::now() < focus_settle_deadline {
            match transaction_receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(batch) => {
                    for registration in batch.fence_registrations {
                        present_fences.insert(registration.handle, registration.fd);
                    }
                    for released in batch.released_fences {
                        present_fences.remove(&released);
                    }
                    for submission in batch.present_submissions {
                        focus_presents = focus_presents.saturating_add(1);
                        present_before_input = present_before_input.saturating_add(1);
                        route_external_present_feedback(
                            "Kitty input smoke",
                            &protocol_router,
                            submission.transaction,
                            submission.idle_fence,
                            &present_fences,
                            presentation_started,
                        )?;
                        last_focus_present = Instant::now();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if last_focus_present.elapsed() >= Duration::from_millis(150) =>
                {
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Kitty input transaction channel disconnected".into());
                }
            }
            ensure_kitty_input_client_alive(&mut child)?;
        }
        eprintln!(
            "sophia_kitty_input_smoke schema=1 stage=focus_settled presents={focus_presents} quiet_msec={}",
            last_focus_present.elapsed().as_millis(),
        );

        let mut time_msec = x11_monotonic_time_msec()?;
        let mut next_delivery = 1u64;
        let deliveries = send_xterm_text_to_client(
            &input_sender,
            client,
            b":ll",
            &mut time_msec,
            &mut next_delivery,
        )?;
        eprintln!(
            "sophia_kitty_input_smoke schema=1 stage=input_queued events={}",
            deliveries.len()
        );
        wait_for_xterm_input_deliveries(&input_delivery_receiver, &deliveries)?;
        eprintln!("sophia_kitty_input_smoke schema=1 stage=input_flushed");
        let input_deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut present_after_input = 0usize;
        while std::time::Instant::now() < input_deadline {
            while let Ok(batch) = transaction_receiver.try_recv() {
                for registration in batch.fence_registrations {
                    present_fences.insert(registration.handle, registration.fd);
                }
                for released in batch.released_fences {
                    present_fences.remove(&released);
                }
                for submission in batch.present_submissions {
                    present_after_input = present_after_input.saturating_add(1);
                    route_external_present_feedback(
                        "Kitty input smoke",
                        &protocol_router,
                        submission.transaction,
                        submission.idle_fence,
                        &present_fences,
                        presentation_started,
                    )?;
                }
            }
            if std::fs::read(&result_file.path)? == b":ll" && present_after_input != 0 {
                eprintln!(
                    "sophia_kitty_input_smoke schema=1 stage=proof_complete presents={present_after_input}"
                );
                return Ok(XAuthorityKittyInputSmokeReport {
                    display: display.clone(),
                    routed_keys: deliveries.len(),
                    present_before_input,
                    present_after_input,
                    text_match: true,
                });
            }
            ensure_kitty_input_client_alive(&mut child)?;
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(format!(
            "Kitty did not consume routed input: received_bytes={} post_input_presents={present_after_input}",
            std::fs::read(&result_file.path)?.len(),
        )
        .into())
    })();

    if child.try_wait()?.is_none()
        && let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
            std::thread::sleep(Duration::from_millis(50));
            if child.try_wait()?.is_none() {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
        }
    eprintln!("sophia_kitty_input_smoke schema=1 stage=client_stopping");
    let output = child.wait_with_output()?;
    let _ = service_sender.send(XServerFrontendServiceCommand::StopAccepting);
    drop(service_sender);
    drop(input_sender);
    drop(control_sender);
    eprintln!("sophia_kitty_input_smoke schema=1 stage=server_stopping");
    let server_result = server
        .join()
        .map_err(|_| "Kitty input smoke server thread panicked")?;
    let _ = std::fs::remove_file(&socket_path);
    server_result.map_err(|error| format!("Kitty input smoke server failed: {error}"))?;
    result.map_err(|error: Box<dyn std::error::Error>| {
        format!(
            "{error}; kitty_status={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        )
        .into()
    })
}

fn route_external_present_feedback(
    label: &str,
    router: &sophia_x_authority::XServerFrontendProtocolRouter,
    transaction: TransactionId,
    idle_fence: Option<sophia_protocol::FenceHandle>,
    present_fences: &BTreeMap<sophia_protocol::FenceHandle, Arc<std::os::fd::OwnedFd>>,
    presentation_started: Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    // Match production's compositor-copy retirement: the source becomes idle
    // before display completion, after one ordinary refresh interval.
    std::thread::sleep(Duration::from_millis(17));
    // Production scanout reports UST relative to the graphics-session start.
    // Using host uptime here creates a discontinuity large enough for a client
    // to stop scheduling frames before it consumes the injected keyboard data.
    let ust = presentation_started.elapsed().as_micros() as u64;
    let msc = 7_000_000_u64.saturating_add(ust / 16_667);
    if let Some(handle) = idle_fence {
        let fence = present_fences
            .get(&handle)
            .ok_or("external Present smoke omitted its registered idle fence")?;
        sophia_xshmfence::trigger(fence)?;
        eprintln!(
            "sophia_external_present_smoke schema=1 label={label:?} stage=idle_fence_triggered transaction={} fence={}",
            transaction.raw(),
            handle.raw(),
        );
    }
    let idle = router.route_present_idle(transaction)?;
    let complete =
        router.route_present_complete(transaction, ust, msc, XPresentCompletionMode::Copy)?;
    eprintln!(
        "sophia_external_present_smoke schema=1 label={label:?} stage=present_feedback transaction={} complete_routed={complete} idle_routed={idle}",
        transaction.raw(),
    );
    if !complete || !idle {
        return Err(format!(
            "{label} Present feedback was not routed: transaction={} complete={complete} idle={idle}",
            transaction.raw(),
        )
        .into());
    }
    Ok(())
}

fn x11_monotonic_time_msec() -> Result<u32, Box<dyn std::error::Error>> {
    let uptime = std::fs::read_to_string("/proc/uptime")?;
    let seconds = uptime
        .split_whitespace()
        .next()
        .ok_or("/proc/uptime omitted monotonic uptime")?
        .parse::<f64>()?;
    Ok((seconds * 1_000.0) as u32)
}

fn ensure_kitty_input_client_alive(
    child: &mut std::process::Child,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(status) = child.try_wait()? {
        return Err(format!("Kitty input client exited before proof: {status}").into());
    }
    Ok(())
}

fn first_openable_render_node() -> Result<std::fs::File, Box<dyn std::error::Error>> {
    let mut candidates = std::fs::read_dir("/dev/dri")
        .map_err(|error| format!("cannot inspect DRM render nodes: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("renderD"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    for path in candidates {
        if let Ok(device) = std::fs::File::options().read(true).write(true).open(&path) {
            return Ok(device);
        }
    }
    Err("vkcube probe found no openable DRM render node".into())
}
