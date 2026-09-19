fn run_x_authority_vkcube_admission_smoke()
-> Result<XAuthorityVkcubeAdmissionSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("vkcube", "vkcube")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    let (display, socket_path) = temp_xauthority_display(6685)?;
    let (transaction_sender, transaction_receiver) = sync_channel(4_096);
    let (control_ack_sender, control_ack_receiver) = sync_channel(64);
    let broker = XServerFrontendRouteBroker::with_control_ack_sender(
        NonZeroUsize::new(64).expect("vkcube admission route capacity is nonzero"),
        control_ack_sender,
    );
    let control_sender = broker.control_sender();
    let protocol_router = broker.protocol_router();
    let (service_sender, service_receiver) = sync_channel(1);
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(64))?
        .with_render_device_provider(provider)
        .with_policy_map_deferred(true);
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
        .env_remove("WAYLAND_DISPLAY")
        .args(["--wsi", "xcb", "--c", "2", "--suppress_popups"])
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(12);
        let presentation_started = Instant::now();
        let mut intent_observed = false;
        let mut admission_delivered = false;
        let mut dma_bufs = 0usize;
        let mut presents = 0usize;
        let mut feedback = 0usize;
        let mut present_fences = BTreeMap::new();
        while Instant::now() < deadline && feedback < 2 {
            let batch = match transaction_receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(batch) => batch,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    ensure_external_probe_client_alive("vkcube", &mut child)?;
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("vkcube admission transaction channel disconnected".into());
                }
            };
            if !batch.protocol_errors.is_empty() {
                return Err(format!(
                    "vkcube admission observed {} unexpected protocol errors",
                    batch.protocol_errors.len()
                )
                .into());
            }
            for intent in &batch.presentation_intents {
                if admission_delivered
                    || intent.kind
                        != sophia_protocol::SurfacePresentationIntentKind::Request
                {
                    continue;
                }
                intent_observed = true;
                let client = batch
                    .client
                    .ok_or("deferred vkcube map intent omitted its routed client")?;
                let admission_transaction = TransactionId::from_raw(9_100_000);
                control_sender.send(XAuthorityClientControlCommand {
                    client,
                    command: XAuthorityControlCommand::AdmitSurface {
                        transaction: admission_transaction,
                        surface: intent.surface,
                        geometry: intent.geometry,
                    },
                })?;
                let acknowledgement =
                    control_ack_receiver.recv_timeout(Duration::from_secs(2))?;
                admission_delivered = acknowledgement.client == client
                    && acknowledgement.acknowledgement.transaction
                        == admission_transaction
                    && acknowledgement.acknowledgement.surface == intent.surface
                    && acknowledgement.acknowledgement.outcome
                        == XAuthorityControlOutcome::Delivered;
                if !admission_delivered {
                    return Err("deferred vkcube map admission was not delivered".into());
                }
            }
            dma_bufs = dma_bufs.saturating_add(batch.dma_buf_registrations.len());
            for registration in batch.fence_registrations {
                present_fences.insert(registration.handle, registration.fd);
            }
            for released in batch.released_fences {
                present_fences.remove(&released);
            }
            for submission in batch.present_submissions {
                if !admission_delivered {
                    return Err("vkcube submitted Present before deferred map admission".into());
                }
                presents = presents.saturating_add(1);
                route_external_present_feedback(
                    "vkcube admission smoke",
                    &protocol_router,
                    submission.transaction,
                    submission.idle_fence,
                    &present_fences,
                    presentation_started,
                )?;
                feedback = feedback.saturating_add(1);
            }
            ensure_external_probe_client_alive("vkcube", &mut child)?;
        }
        if !intent_observed {
            return Err("vkcube admission smoke observed no deferred map intent".into());
        }
        if !admission_delivered {
            return Err("vkcube admission smoke did not deliver admission".into());
        }
        if dma_bufs == 0 || presents < 2 || feedback < 2 {
            return Err(format!(
                "vkcube admission smoke lacked continued DRI3/Present traffic: dma_bufs={dma_bufs} presents={presents} feedback={feedback}"
            )
            .into());
        }
        Ok(XAuthorityVkcubeAdmissionSmokeReport {
            display: display.clone(),
            intent_observed,
            admission_delivered,
            dma_bufs,
            presents,
            feedback,
        })
    })();

    if child.try_wait()?.is_none()
        && let Some(group) = rustix::process::Pid::from_raw(child.id() as i32)
    {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
        std::thread::sleep(Duration::from_millis(25));
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
    let output = child.wait_with_output()?;
    let _ = service_sender.send(XServerFrontendServiceCommand::StopAccepting);
    drop(service_sender);
    drop(control_sender);
    let server_result = server
        .join()
        .map_err(|_| "vkcube admission smoke server thread panicked")?;
    let _ = std::fs::remove_file(&socket_path);
    server_result.map_err(|error| format!("vkcube admission smoke server failed: {error}"))?;
    result.map_err(|error: Box<dyn std::error::Error>| {
        format!(
            "{error}; vkcube_status={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        )
        .into()
    })
}

fn ensure_external_probe_client_alive(
    label: &str,
    child: &mut std::process::Child,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(status) = child.try_wait()? {
        return Err(format!("{label} exited before proof: {status}").into());
    }
    Ok(())
}
