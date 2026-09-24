fn print_external_probe_smoke_report(
    command_name: &str,
    report: &XAuthorityExternalProbeSmokeReport,
) {
    println!(
        "{} display={} outcome={} status={} stdout_bytes={} stderr_bytes={} requests={} opcode_count={} opcodes={} transactions={} runtime_committed={} runtime_surfaces={} cpu_buffers={} cpu_buffer_bytes={} nonzero_pixel_bytes={} ascii_marker_match={} first_error={}",
        command_name,
        report.display,
        report.outcome,
        report.status,
        report.stdout_bytes,
        report.stderr_bytes,
        report.requests,
        report.opcode_count,
        report.opcodes,
        report.transactions,
        report.runtime_committed,
        report.runtime_surfaces,
        report.cpu_buffers,
        report.cpu_buffer_bytes,
        report.nonzero_pixel_bytes,
        report.ascii_marker_match,
        report.first_error.as_deref().unwrap_or("none")
    );
}

struct ExternalProbeInvocation<'a> {
    label: &'a str,
    command: &'a std::path::Path,
    display_mode: ExternalProbeDisplayMode,
    command_args: &'a [&'a str],
    /// Environment the client needs beyond the display, as a property of the
    /// client rather than a special case on its name.
    extra_env: &'a [(&'a str, &'a str)],
    display: String,
    socket_path: std::path::PathBuf,
    namespace: NamespaceId,
    require_transactions: bool,
    pixel_proof: ExternalProbePixelProof,
    allow_proof_kill_without_transactions: bool,
    allow_client_failure_without_x_error: bool,
    render_device_provider: Option<Arc<dyn XServerFrontendRenderDeviceProvider>>,
    /// Originates buffers for a client that expects the server to own the
    /// storage. `None` still serves the client-allocated half.
    pixmap_allocator: Option<Arc<dyn sophia_x_authority::XServerFrontendPixmapAllocator>>,
    /// How long the client gets to prove itself.
    ///
    /// A property of the client, not of its name: a terminal draws in
    /// milliseconds and a browser starts a GPU process first. Carried here so
    /// a slow client is described rather than special-cased by label.
    proof_timeout: Duration,
    /// Whether the client is given a deliberately unreachable session bus.
    ///
    /// An absent address is not neutral: a toolkit that finds none autolaunches
    /// its own daemon, which opens connections and takes time the proof then
    /// attributes to the graphics path. An address that parses and cannot be
    /// reached makes desktop integration fail immediately instead.
    isolate_session_bus: bool,
}

fn run_x_authority_external_probe_smoke(
    invocation: ExternalProbeInvocation<'_>,
) -> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let ExternalProbeInvocation {
        label,
        command,
        display_mode,
        command_args,
        extra_env,
        display,
        socket_path,
        namespace,
        require_transactions,
        pixel_proof,
        allow_proof_kill_without_transactions,
        allow_client_failure_without_x_error,
        render_device_provider,
        pixmap_allocator,
        proof_timeout,
        isolate_session_bus,
    } = invocation;
    let server_path = socket_path.clone();
    // One X request can produce an opcode, detail, transaction, and buffer
    // update. Keep the diagnostic channel large enough that a replacement
    // update cannot be dropped while a later patch is retained.
    let (sender, receiver) = sync_channel(4_096);
    let mut server_config = XServerFrontendConfig::new(&server_path, namespace)?;
    if label == "kitty" {
        server_config = server_config.with_output_topology(two_output_external_probe_topology())?;
    }
    if let Some(provider) = render_device_provider {
        server_config = server_config.with_render_device_provider(provider);
    }
    if let Some(allocator) = pixmap_allocator {
        server_config = server_config.with_pixmap_allocator(allocator);
    }
    let server = std::thread::spawn(move || {
        run_x11_core_socket_server_once_config_traced_with_idle_timeout(
            server_config,
            proof_timeout,
            |trace| {
                let _ = sender.try_send(ExternalProbeObservation::Opcode(trace.major_opcode));
                if trace.request_stage != sophia_x_authority::X11ObservedRequestStage::Other {
                    let _ = sender.try_send(ExternalProbeObservation::Detail(
                        trace.request_stage.evidence_name().to_owned(),
                    ));
                }
                if trace.failure.is_some() {
                    let _ = sender.try_send(ExternalProbeObservation::Error(format!(
                        "parse_error:major={}:minor={}",
                        trace.major_opcode, trace.minor_opcode
                    )));
                }
                for output in &trace.result.outputs {
                    if let XClientOutput::Error(error) = output {
                        if error.code == sophia_x_authority::XErrorCode::BadWindow
                            && error.resource_id == 0
                            && error.minor_code == 0
                            && matches!(error.major_code, 3 | 14)
                        {
                            continue;
                        }
                        let _ = sender.try_send(ExternalProbeObservation::Error(format!(
                            "{:?}:major={}:minor={}:resource={:#x}",
                            error.code, error.major_code, error.minor_code, error.resource_id
                        )));
                    }
                }
                if let Some(response) = &trace.result.response
                    && !response.transactions.is_empty()
                {
                    let _ = sender.try_send(ExternalProbeObservation::Transactions(
                        response.transactions.clone(),
                    ));
                }
                for buffer in &trace.cpu_buffer_updates {
                    let _ =
                        sender.try_send(ExternalProbeObservation::CpuBufferUpdate(buffer.clone()));
                }
                let _ = sender.try_send(ExternalProbeObservation::RequestProof(
                    ExternalProbeRequestProof {
                        opcode: trace.major_opcode,
                        transactions: trace
                            .result
                            .response
                            .as_ref()
                            .map(|response| response.transactions.clone())
                            .unwrap_or_default(),
                        removed_surfaces: trace
                            .result
                            .response
                            .as_ref()
                            .map(|response| response.removed_surfaces.clone())
                            .unwrap_or_default(),
                        cpu_buffer_handle: trace
                            .cpu_buffer_updates
                            .first()
                            .map(|update| update.handle()),
                    },
                ));
                Ok(())
            },
        )
    });
    wait_for_socket_path(&socket_path)?;

    let mut command = std::process::Command::new(command);
    let firefox_profile = (label == "firefox").then(|| {
        std::env::temp_dir().join(format!(
            "sophia-firefox-profile-{}-{}",
            std::process::id(),
            namespace.raw()
        ))
    });
    if let Some(profile) = firefox_profile.as_ref() {
        std::fs::create_dir(profile)?;
        command.arg("--profile").arg(profile);
    }
    match display_mode {
        ExternalProbeDisplayMode::Argument(display_arg) => {
            command.arg(display_arg).arg(&display);
        }
        ExternalProbeDisplayMode::Environment => {
            command
                .env("DISPLAY", &display)
                .env("GDK_BACKEND", "x11")
                .env("GTK_USE_PORTAL", "0")
                .env("MOZ_ENABLE_WAYLAND", "0")
                .env_remove("WAYLAND_DISPLAY");
            if isolate_session_bus
                && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
            {
                command.env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null");
            }
        }
    }
    for (name, value) in extra_env {
        command.env(name, value);
    }
    command
        .args(command_args)
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;

    let deadline = std::time::Instant::now() + proof_timeout;
    let mut transactions = Vec::new();
    let mut cpu_buffers = std::collections::BTreeMap::new();
    let mut cpu_buffer_updates = 0usize;
    let mut first_error = None;
    let mut opcodes = std::collections::BTreeSet::new();
    let mut request_proofs = Vec::new();
    let mut fixed_text_scroll_proved = false;
    let mut details = std::collections::BTreeSet::new();
    let mut requests = 0usize;
    let minimum_transactions = if label == "xmobar" { 6 } else { 1 };

    while std::time::Instant::now() < deadline {
        while let Ok(observation) = receiver.try_recv() {
            match observation {
                ExternalProbeObservation::Opcode(opcode) => {
                    requests = requests.saturating_add(1);
                    opcodes.insert(opcode);
                }
                ExternalProbeObservation::Transactions(batch) => transactions.extend(batch),
                ExternalProbeObservation::CpuBufferUpdate(update) => {
                    cpu_buffer_updates = cpu_buffer_updates.saturating_add(1);
                    update.apply_to(&mut cpu_buffers)?;
                }
                ExternalProbeObservation::Detail(detail) => {
                    details.insert(detail);
                }
                ExternalProbeObservation::Error(error) => {
                    first_error.get_or_insert(error);
                }
                ExternalProbeObservation::RequestProof(proof) => request_proofs.push(proof),
            }
        }
        let pixel_proof_ready = match pixel_proof {
            ExternalProbePixelProof::None => true,
            ExternalProbePixelProof::Nonzero if label == "xmobar" => {
                latest_transaction_cpu_buffer(&transactions, &cpu_buffers)
                    .is_some_and(|buffer| buffer.bytes.iter().any(|byte| *byte != 0))
            }
            ExternalProbePixelProof::Nonzero => cpu_buffers
                .values()
                .any(|buffer| buffer.bytes.iter().any(|byte| *byte != 0)),
            ExternalProbePixelProof::Fixed6x13WhiteOnBlack(marker) => {
                latest_transaction_cpu_buffer(&transactions, &cpu_buffers).is_some_and(|buffer| {
                    cpu_buffer_contains_fixed_text(buffer, marker, Some((0x00ff_ffff, 0)))
                })
            }
        };
        fixed_text_scroll_proved |=
            label == "xterm_render" && fixed_text_scroll_proof(&request_proofs, &cpu_buffers);
        let required_opcodes_ready = label != "xterm_render" || fixed_text_scroll_proved;
        if transactions.len() >= minimum_transactions && pixel_proof_ready && required_opcodes_ready
        {
            break;
        }
        if child.try_wait()?.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut proof_window_killed = false;
    let output = if child.try_wait()?.is_none() {
        if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
            std::thread::sleep(Duration::from_millis(25));
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        proof_window_killed = true;
        child.wait_with_output()?
    } else {
        child.wait_with_output()?
    };
    let status = output.status.code().unwrap_or(-1);

    let _ = std::fs::remove_file(&socket_path);
    if let Some(profile) = firefox_profile.as_ref() {
        let _ = std::fs::remove_dir_all(profile);
    }
    // Join only when the client actually reached the server. A probe that exits
    // before it opens the display -- a usage error, a missing argument -- leaves
    // nothing to drain, and waiting on it used to deadlock with no report at all,
    // because the report prints after this function returns.
    if requests > 0 && (!allow_proof_kill_without_transactions || !proof_window_killed) {
        server
            .join()
            .map_err(|_| format!("X authority {label} socket server thread panicked"))?
            .map_err(|error| format!("X authority {label} socket server failed: {error}"))?;
    }

    while let Ok(observation) = receiver.try_recv() {
        match observation {
            ExternalProbeObservation::Opcode(opcode) => {
                requests = requests.saturating_add(1);
                opcodes.insert(opcode);
            }
            ExternalProbeObservation::Transactions(batch) => transactions.extend(batch),
            ExternalProbeObservation::CpuBufferUpdate(update) => {
                cpu_buffer_updates = cpu_buffer_updates.saturating_add(1);
                update.apply_to(&mut cpu_buffers)?;
            }
            ExternalProbeObservation::Detail(detail) => {
                details.insert(detail);
            }
            ExternalProbeObservation::Error(error) => {
                first_error.get_or_insert(error);
            }
            ExternalProbeObservation::RequestProof(proof) => request_proofs.push(proof),
        }
    }

    fixed_text_scroll_proved |=
        label == "xterm_render" && fixed_text_scroll_proof(&request_proofs, &cpu_buffers);
    let opcode_count = opcodes.len();
    let opcodes = opcodes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let details = details.into_iter().collect::<Vec<_>>().join(",");

    let required_graphics_stages: &[&str] = match label {
        "kitty" => &[
            "GLX:QueryServerString",
            "GLX:GetFBConfigs",
            "GLX:CreateContext",
            "GLX:CreateWindow",
            "DRI3:PixmapFromBuffers",
            "PRESENT:Pixmap",
        ],
        "glxgears" => &[
            "GLX:GetVisualConfigs",
            "GLX:CreateContext",
            "DRI3:PixmapFromBuffers",
            "PRESENT:Pixmap",
        ],
        // Measured, not assumed: once RENDER was advertised this trace grew
        // from 33 opcodes to 34 and from 326 requests to 329, so Qt does ask.
        // Asserting the stage keeps a future regression in the advertisement
        // from silently returning the shell to core drawing.
        "quickshell" => &["RENDER:QueryPictFormats"],
        _ => &[],
    };
    for required in required_graphics_stages {
        if !details.contains(required) {
            return Err(format!(
                "{label} trace omitted required stage {required} for {display}: details={details}"
            )
            .into());
        }
    }

    if let Some(error) = &first_error
        && !probe_tolerates_client_error(label, error)
    {
        return Err(format!(
            "{label} produced an X protocol error for {display}: status={status} requests={requests} opcode_count={opcode_count} opcodes={opcodes} details={details} first_error={error} stderr={}",
            String::from_utf8_lossy(&output.stderr).trim(),
        )
        .into());
    }

    if require_transactions && transactions.is_empty() {
        return Err(format!(
            "{label} did not produce an authority transaction for {display}: status={status} requests={requests} opcode_count={opcode_count} opcodes={opcodes} details={details} stderr={} first_error={}",
            String::from_utf8_lossy(&output.stderr).trim(),
            first_error.as_deref().unwrap_or("none")
        )
        .into());
    }

    if !(require_transactions
        || output.status.success()
        || (allow_proof_kill_without_transactions && proof_window_killed)
        || (allow_client_failure_without_x_error && requests > 0))
    {
        return Err(format!(
            "{label} probe failed for {display}: status={status} requests={requests} opcode_count={opcode_count} opcodes={opcodes} details={details} stderr={} first_error={}",
            String::from_utf8_lossy(&output.stderr).trim(),
            first_error.as_deref().unwrap_or("none")
        )
        .into());
    }

    // A sustained client is meant to cross the runtime adapter's 64-observation
    // per-tick bound. Replay consecutive bounded ticks instead of treating that
    // adapter capacity as a client-lifetime limit.
    let mut runtime_committed = 0u64;
    let mut runtime_surfaces = 0u64;
    const RUNTIME_FIXED_OBSERVATION_RESERVE: usize = 16;
    let transaction_capacity = sophia_runtime::MAX_SESSION_RUNTIME_OBSERVATION_BATCH
        .saturating_sub(RUNTIME_FIXED_OBSERVATION_RESERVE);
    for bounded_tick in transactions.chunks(transaction_capacity) {
        let state = runtime_state_from_observed_transactions(bounded_tick)?;
        runtime_committed =
            runtime_committed.saturating_add(state.authority_transactions_committed);
        runtime_surfaces = runtime_surfaces.max(state.authority_surfaces_applied);
    }
    let cpu_buffer_bytes = cpu_buffers.values().map(|buffer| buffer.bytes.len()).sum();
    let nonzero_pixel_bytes = cpu_buffers
        .values()
        .flat_map(|buffer| buffer.bytes.iter())
        .filter(|byte| **byte != 0)
        .count();
    let ascii_marker_match = latest_transaction_cpu_buffer(&transactions, &cpu_buffers)
        .is_some_and(|buffer| cpu_buffer_contains_fixed_text(buffer, b"Sophia", None));
    let pixel_proof_passed = match pixel_proof {
        ExternalProbePixelProof::None => true,
        ExternalProbePixelProof::Nonzero if label == "xmobar" => {
            latest_transaction_cpu_buffer(&transactions, &cpu_buffers)
                .is_some_and(|buffer| buffer.bytes.iter().any(|byte| *byte != 0))
        }
        ExternalProbePixelProof::Nonzero => nonzero_pixel_bytes != 0,
        ExternalProbePixelProof::Fixed6x13WhiteOnBlack(marker) => {
            latest_transaction_cpu_buffer(&transactions, &cpu_buffers).is_some_and(|buffer| {
                cpu_buffer_contains_fixed_text(buffer, marker, Some((0x00ff_ffff, 0)))
            })
        }
    };
    if !pixel_proof_passed {
        return Err(format!(
            "{label} did not satisfy its pixel proof for {display}: requests={requests} opcodes={opcodes} details={details}"
        )
        .into());
    }
    if label == "xterm_render" && !fixed_text_scroll_proved {
        return Err(format!(
            "{label} did not prove sustained current-surface fixed-text scrolling for {display}: required=ImageText8->CopyArea->ImageText8+rows077-080 observed={opcodes}"
        )
        .into());
    }
    if require_transactions
        && (runtime_committed < u64::try_from(minimum_transactions).unwrap_or(u64::MAX)
            || runtime_surfaces == 0)
    {
        return Err(format!(
            "{label} transactions did not commit through runtime for {display}: transactions={} minimum={} committed={} surfaces={}",
            transactions.len(),
            minimum_transactions,
            runtime_committed,
            runtime_surfaces
        )
        .into());
    }

    let outcome = if proof_window_killed {
        "proof_window_killed"
    } else if output.status.success() {
        "client_exited_success"
    } else {
        "client_exited_failure"
    };

    Ok(XAuthorityExternalProbeSmokeReport {
        display,
        outcome: outcome.to_owned(),
        status,
        stdout_bytes: output.stdout.len(),
        stderr_bytes: output.stderr.len(),
        requests,
        opcode_count,
        opcodes,
        transactions: transactions.len(),
        runtime_committed,
        runtime_surfaces,
        cpu_buffers: cpu_buffer_updates,
        cpu_buffer_bytes,
        nonzero_pixel_bytes,
        ascii_marker_match,
        first_error,
        observed_transactions: transactions,
        observed_cpu_buffers: cpu_buffers.into_values().collect(),
    })
}

fn latest_transaction_cpu_buffer<'a>(
    transactions: &[SurfaceTransaction],
    buffers: &'a std::collections::BTreeMap<u64, XAuthorityCpuBufferSnapshot>,
) -> Option<&'a XAuthorityCpuBufferSnapshot> {
    let handle = transactions
        .last()
        .and_then(|transaction| match transaction.target_buffer() {
            BufferSource::CpuBuffer { handle } => Some(handle),
            _ => None,
        })?;
    buffers.get(&handle)
}

fn two_output_external_probe_topology() -> sophia_protocol::OutputTopologySnapshot {
    sophia_protocol::OutputTopologySnapshot {
        generation: 1,
        primary: sophia_protocol::OutputId::from_raw(1),
        outputs: vec![
            sophia_protocol::OutputTopologyEntry {
                output: sophia_protocol::OutputId::from_raw(1),
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 720,
                },
                pixel_size: Size {
                    width: 1280,
                    height: 720,
                },
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            },
            sophia_protocol::OutputTopologyEntry {
                output: sophia_protocol::OutputId::from_raw(2),
                logical: Rect {
                    x: 1280,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                pixel_size: Size {
                    width: 1920,
                    height: 1080,
                },
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            },
        ],
    }
}

/// Errors a client asks for itself, which say nothing about the server.
///
/// The bar is otherwise `first_error=none`, and it stays there: this admits
/// one exact shape and nothing else.
///
/// Qt's xcb backend destroys and recreates its window when the requested
/// format changes, and Mesa's DRI3 loader then unselects Present events during
/// drawable teardown -- naming, three requests later on the same connection, a
/// window the client has already destroyed. `BadWindow` is the correct answer
/// and Xorg gives the same one; the client ignores it and carries on. Failing
/// the probe for it would be reporting a client's own race as a server defect.
fn probe_tolerates_client_error(label: &str, error: &str) -> bool {
    label == "quickshell" && error.starts_with("BadWindow:major=138:minor=3:")
}

fn fixed_text_scroll_proof(
    requests: &[ExternalProbeRequestProof],
    buffers: &std::collections::BTreeMap<u64, XAuthorityCpuBufferSnapshot>,
) -> bool {
    let rows = [
        b"SophiaStream077".as_slice(),
        b"SophiaStream078".as_slice(),
        b"SophiaStream079".as_slice(),
        b"SophiaStream080".as_slice(),
    ];
    let mut current = std::collections::BTreeMap::new();
    for request in requests {
        for surface in &request.removed_surfaces {
            current.remove(surface);
        }
        for transaction in &request.transactions {
            current.insert(transaction.surface, transaction);
        }
    }
    current.into_iter().any(|(surface, transaction)| {
        if transaction.target_geometry.width <= 0 || transaction.target_geometry.height <= 0 {
            return false;
        }
        let BufferSource::CpuBuffer { handle } = transaction.target_buffer() else {
            return false;
        };
        buffers.get(&handle).is_some_and(|buffer| {
            let rows_match =
                fixed_text_rows_match_adjacent(buffer, &rows, Some((0x00ff_ffff, 0)));
            let causal_requests = request_proves_scrolling_surface(requests, surface);
            tracing::debug!(
                target: "sophia_xterm_render_proof",
                surface = ?surface,
                handle,
                rows_match,
                causal_requests,
                "evaluated current fixed-text scroll candidate"
            );
            rows_match && causal_requests
        })
    })
}

fn request_proves_scrolling_surface(
    requests: &[ExternalProbeRequestProof],
    surface: sophia_protocol::SurfaceId,
) -> bool {
    let mut stage = 0u8;
    for request in requests {
        let updates_surface = request.cpu_buffer_handle.is_some_and(|handle| {
            request.transactions.iter().any(|transaction| {
                transaction.surface == surface
                    && transaction.target_buffer() == BufferSource::CpuBuffer { handle }
            })
        });
        if !updates_surface {
            continue;
        }
        tracing::debug!(
            target: "sophia_xterm_render_proof",
            surface = ?surface,
            opcode = request.opcode,
            stage,
            handle = ?request.cpu_buffer_handle,
            "accepted same-surface CPU drawing request"
        );
        stage = match (stage, request.opcode) {
            (0, 76) => 1,
            (1, 62) => 2,
            (2, 76) => 3,
            (current, _) => current,
        };
    }
    stage == 3
}

fn fixed_text_rows_match_adjacent(
    buffer: &XAuthorityCpuBufferSnapshot,
    rows: &[&[u8]],
    expected_colors: Option<(u32, u32)>,
) -> bool {
    if rows.is_empty() || rows.iter().any(|row| row.is_empty()) {
        return false;
    }
    let Ok(width) = usize::try_from(buffer.size.width) else {
        return false;
    };
    let Ok(height) = usize::try_from(buffer.size.height) else {
        return false;
    };
    let Some(max_text_width) = rows
        .iter()
        .filter_map(|row| row.len().checked_mul(6))
        .max()
    else {
        return false;
    };
    let Some(rows_height) = rows.len().checked_mul(13) else {
        return false;
    };
    if width < max_text_width || height < rows_height {
        return false;
    }
    (0..=height - rows_height).any(|top| {
        (0..=width - max_text_width).any(|left| {
            rows.iter().enumerate().all(|(index, row)| {
                fixed_text_matches_at(
                    buffer,
                    left,
                    top + index * 13,
                    row,
                    expected_colors,
                )
            })
        })
    })
}

fn cpu_buffer_contains_fixed_text(
    buffer: &XAuthorityCpuBufferSnapshot,
    text: &[u8],
    expected_colors: Option<(u32, u32)>,
) -> bool {
    if text.is_empty() {
        return false;
    }
    let Ok(width) = usize::try_from(buffer.size.width) else {
        return false;
    };
    let Ok(height) = usize::try_from(buffer.size.height) else {
        return false;
    };
    let Some(text_width) = text.len().checked_mul(6) else {
        return false;
    };
    if width < text_width || height < 13 {
        return false;
    }
    (0..=height - 13).any(|top| {
        (0..=width - text_width)
            .any(|left| fixed_text_matches_at(buffer, left, top, text, expected_colors))
    })
}

fn fixed_text_matches_at(
    buffer: &XAuthorityCpuBufferSnapshot,
    left: usize,
    top: usize,
    text: &[u8],
    expected_colors: Option<(u32, u32)>,
) -> bool {
    let Some(background) = xrgb_pixel(buffer, left, top) else {
        return false;
    };
    let first_rows = x_fixed_glyph_rows(text[0]);
    let Some((first_row, first_column)) = first_rows.iter().enumerate().find_map(|(row, bits)| {
        (0..6)
            .find(|column| bits & (1 << (5 - column)) != 0)
            .map(|column| (row, column))
    }) else {
        return false;
    };
    let Some(foreground) = xrgb_pixel(
        buffer,
        left.saturating_add(first_column),
        top.saturating_add(first_row),
    ) else {
        return false;
    };
    if foreground == background {
        return false;
    }
    if let Some((expected_foreground, expected_background)) = expected_colors
        && (foreground != expected_foreground || background != expected_background)
    {
        return false;
    }
    for (index, byte) in text.iter().copied().enumerate() {
        let rows = x_fixed_glyph_rows(byte);
        let cell_left = left.saturating_add(index.saturating_mul(6));
        for (row, bits) in rows.into_iter().enumerate() {
            for column in 0..6 {
                let expected = if bits & (1 << (5 - column)) != 0 {
                    foreground
                } else {
                    background
                };
                if xrgb_pixel(
                    buffer,
                    cell_left.saturating_add(column),
                    top.saturating_add(row),
                ) != Some(expected)
                {
                    return false;
                }
            }
        }
    }
    true
}

fn xrgb_pixel(buffer: &XAuthorityCpuBufferSnapshot, x: usize, y: usize) -> Option<u32> {
    let stride = usize::try_from(buffer.stride).ok()?;
    let offset = y.checked_mul(stride)?.checked_add(x.checked_mul(4)?)?;
    Some(u32::from_le_bytes(
        buffer
            .bytes
            .get(offset..offset.checked_add(4)?)?
            .try_into()
            .ok()?,
    ))
}

#[derive(Clone, Debug)]
enum ExternalProbeObservation {
    Opcode(u8),
    Transactions(Vec<SurfaceTransaction>),
    CpuBufferUpdate(XAuthorityCpuBufferUpdate),
    Detail(String),
    Error(String),
    RequestProof(ExternalProbeRequestProof),
}

#[derive(Clone, Debug)]
struct ExternalProbeRequestProof {
    opcode: u8,
    transactions: Vec<SurfaceTransaction>,
    removed_surfaces: Vec<sophia_protocol::SurfaceId>,
    cpu_buffer_handle: Option<u64>,
}

#[path = "external_probe/tests.rs"]
mod external_probe_tests;
