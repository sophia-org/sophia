pub fn run_x11_core_socket_server_once(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_once_observed(path, namespace, |_| {})
}

/// Runs one X11 authority listener until its enclosing process is stopped.
///
/// Clients are served sequentially and share one authority state. Concurrent
/// multi-client dispatch and client-specific resource allocation remain a
/// separate milestone.
#[cfg(unix)]
pub fn run_x11_core_socket_server(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_observed(path, namespace, |_| {})
}
#[cfg(unix)]
pub fn run_x11_core_socket_server_observed(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    mut observer: impl FnMut(&XDispatchResult),
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_traced(path, namespace, move |trace| {
        observer(&trace.result);
        Ok(())
    })
}
pub fn run_x11_core_socket_server_traced(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    let config = XServerFrontendConfig::new(path.as_ref(), namespace)?;
    let mut frontend = XServerFrontend::bind(config)?;
    frontend.serve_forever_traced(observer)
}

#[cfg(unix)]
pub fn run_x11_core_socket_server_channel(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    sender: SyncSender<XAuthorityObservedTransactionBatch>,
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_traced(path, namespace, move |trace| {
        try_emit_x_authority_observation(&sender, &trace)
            .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        Ok(())
    })
}

#[cfg(unix)]
pub fn run_x11_core_socket_server_once_observed(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    mut observer: impl FnMut(&XDispatchResult),
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_once_traced(path, namespace, move |trace| {
        let result = trace.result;
        observer(&result);
        Ok(())
    })
}

#[cfg(unix)]
pub fn run_x11_core_socket_server_once_traced(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_once_with_trace_observer(path, namespace, None, observer)
}

#[cfg(unix)]
pub fn run_x11_core_socket_server_once_traced_with_idle_timeout(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    idle_timeout: Duration,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_once_with_trace_observer(
        path,
        namespace,
        Some(idle_timeout),
        observer,
    )
}

/// Runs one bounded client against an explicitly assembled frontend config.
///
/// This retains the external-probe idle timeout while allowing a caller to
/// inject backend-owned capabilities such as the DRI3 render-device provider.
#[cfg(unix)]
pub fn run_x11_core_socket_server_once_config_traced_with_idle_timeout(
    mut config: XServerFrontendConfig,
    idle_timeout: Duration,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    let listener = bind_x11_core_socket_server(config.socket_path())?;
    let state = X11CoreSocketServerState::with_output_topology_xkb_config_and_font_path(
        config.output_topology().clone(),
        config.xkb_config(),
        config.font_path(),
    )?
    .with_optional_render_device_provider(config.render_device_provider())
        .with_optional_pixmap_allocator(config.pixmap_allocator());
    state.install_configured_device_bundle(&mut config)?;
    state.latch_pixmap_texture_support()?;
    serve_x11_core_socket_listener_once_with_setup_authorization(
        &listener,
        config.namespace(),
        &state,
        config.setup_authorization(),
        config.admission_policy(),
        config.injection_policy(),
        Some(idle_timeout),
        observer,
    )
}
pub fn run_x11_core_socket_server_once_channel(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    sender: SyncSender<XAuthorityObservedTransactionBatch>,
) -> Result<(), X11SetupSocketError> {
    run_x11_core_socket_server_once_with_trace_observer(path, namespace, None, move |trace| {
        try_emit_x_authority_observation(&sender, &trace)
            .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        Ok(())
    })
}

#[cfg(unix)]
pub fn run_x11_core_socket_server_once_channels(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    input_receiver: Receiver<XAuthorityInputEvent>,
) -> Result<(), X11SetupSocketError> {
    let listener = bind_x11_core_socket_server(path)?;
    let (mut stream, _) = listener.accept().map_err(|error| {
        X11SetupSocketError::new(format!("failed to accept X11 core client: {error}"))
    })?;
    let state = X11CoreSocketServerState::new();
    serve_x11_core_socket_client_with_trace_observer_and_input(
        &mut stream,
        namespace,
        &state,
        X11ClientConnectionInputs {
            input_receiver: Some(X11InputEventReceiver::Plain(input_receiver)),
            control_channels: None,
            client_routing: None,
        },
        X11ClientAdmissionContext {
            authorization: &XServerFrontendSetupAuthorization::default(),
            admission_policy: None,
            injection_policy: None,
            worker_admission: None,
        },
        move |trace| {
            try_emit_x_authority_observation(&transaction_sender, &trace)
                .map(|batch| batch.map(|batch| batch.transaction))
                .map_err(|error| X11SetupSocketError::new(error.to_string()))
        },
    )
}

#[cfg(unix)]
pub fn run_x11_core_socket_server_once_session_channels(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    input_receiver: Receiver<XAuthorityClientInputEvent>,
    control_receiver: Receiver<XAuthorityClientControlCommand>,
    control_ack_sender: SyncSender<XAuthorityClientControlAck>,
) -> Result<(), X11SetupSocketError> {
    let listener = bind_x11_core_socket_server(path)?;
    let (mut stream, _) = listener.accept().map_err(|error| {
        X11SetupSocketError::new(format!("failed to accept X11 core client: {error}"))
    })?;
    let state = X11CoreSocketServerState::new();
    serve_x11_core_socket_client_with_trace_observer_and_input(
        &mut stream,
        namespace,
        &state,
        X11ClientConnectionInputs {
            input_receiver: Some(X11InputEventReceiver::Routed {
                receiver: input_receiver,
                deliveries: None,
                recovery: None,
            }),
            control_channels: Some(X11ControlChannels::Routed {
                receiver: control_receiver,
                acknowledgements: control_ack_sender,
                completion: None,
            }),
            client_routing: None,
        },
        X11ClientAdmissionContext {
            authorization: &XServerFrontendSetupAuthorization::default(),
            admission_policy: None,
            injection_policy: None,
            worker_admission: None,
        },
        move |trace| {
            try_emit_x_authority_observation(&transaction_sender, &trace)
                .map(|batch| batch.map(|batch| batch.transaction))
                .map_err(|error| X11SetupSocketError::new(error.to_string()))
        },
    )
}

/// Runs one routed concurrent X11 client until it disconnects.
///
/// The caller owns the broker's input/control senders and must stop producing
/// routes before joining this helper. This is the migration bridge from the
/// single-client live-session transport to the general bounded concurrent
/// frontend service: the connection uses the same private worker queues as a
/// multi-client frontend, while this helper intentionally accepts only one
/// client.
#[cfg(unix)]
pub fn run_x11_core_socket_server_once_routed(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    mut broker: XServerFrontendRouteBroker,
) -> Result<(), X11SetupSocketError> {
    let config = XServerFrontendConfig::new(path.as_ref().to_path_buf(), namespace)?;
    let mut frontend = XServerFrontend::bind(config)?;
    frontend
        .state
        .runtime
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .set_input_authority(broker.registry.input_authority.clone());
    let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
        try_emit_x_authority_observation(&transaction_sender, &trace)
            .map(|batch| batch.map(|batch| batch.transaction))
            .map_err(x_authority_observation_client_error)
    });
    frontend.serve_next_concurrently_routed_traced(&broker, observer)?;
    while frontend.active_client_worker_count() != 0 {
        let routed = broker
            .route_pending()
            .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        frontend.poll_client_workers()?;
        if routed == 0 && frontend.active_client_worker_count() != 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

/// Runs a bounded routed X11 frontend until supervision stops accepting.
///
/// While accepting, the service starts every ready local connection up to the
/// configured worker limit, routes all pending Engine input/control into the
/// owning worker's private queues, and reaps completed workers. A
/// [`XServerFrontendServiceCommand::StopAccepting`] command closes admission
/// without closing client streams; the service then drains the workers that
/// already exist. The caller remains responsible for its session process
/// policy and should stop producing Engine routes before sending that command.
#[cfg(unix)]
pub fn run_x_server_frontend_routed_until_stopped(
    config: XServerFrontendConfig,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    broker: XServerFrontendRouteBroker,
    service_commands: Receiver<XServerFrontendServiceCommand>,
) -> Result<(), X11SetupSocketError> {
    run_x_server_frontend_routed_until_stopped_with_backpressure_observer(
        config,
        transaction_sender,
        broker,
        service_commands,
        Arc::new(trace_x_authority_backpressure),
    )
}

include!("server/ordered_egress.rs");

#[cfg(unix)]
pub fn run_x_server_frontend_routed_until_stopped_with_backpressure_observer(
    config: XServerFrontendConfig,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    mut broker: XServerFrontendRouteBroker,
    service_commands: Receiver<XServerFrontendServiceCommand>,
    backpressure_observer: Arc<XAuthorityBackpressureObserver>,
) -> Result<(), X11SetupSocketError> {
    let mut frontend = XServerFrontend::bind(config)?;
    frontend
        .state
        .runtime
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?
        .set_input_authority(broker.registry.input_authority.clone());
    let cancellation = Arc::new(AtomicBool::new(false));
    let ordered_egress = Arc::new(XAuthorityOrderedEgress::new(
        transaction_sender,
        cancellation,
        backpressure_observer,
    ));
    let worker_egress = ordered_egress.clone();
    let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
        if trace.failure == Some(X11ObservedDispatchFailure::UnpublishedEffects) {
            worker_egress.cancel();
            return Err(X11SetupSocketError::new("X11 dispatch ended with unpublished authority effects"));
        }
        let batch = XAuthorityObservedTransactionBatch::from_dispatch_observation(&trace);
        let receipt = batch.as_ref().map(|batch| batch.transaction);
        worker_egress.submit_blocking(XAuthorityBoundedEgressEnvelope::new(trace.transaction, batch))?;
        Ok(receipt)
    });
    let mut pending_raster_egress = None::<XAuthorityBoundedEgressEnvelope>;
    // Shared with the private service through the owned public broker. Its
    // private producer hooks are no-ops on this public path.
    let service_result = drive_routed_service(
        &mut frontend,
        &mut broker,
        &service_commands,
        &ordered_egress,
        &observer,
        &mut pending_raster_egress,
    );

    let mut cleanup_failures = Vec::new();
    if service_result.is_err() {
        ordered_egress.cancel();
        if let Some(mut envelope) = pending_raster_egress.take()
            && let Err(error) = ordered_egress.cancel_envelope(&mut envelope)
        {
            cleanup_failures.push(format!("pending raster cancellation failed: {error}"));
        }
        if let Err(error) = frontend.shutdown_all_client_workers() {
            cleanup_failures.push(format!("worker shutdown failed: {error}"));
        }
        if let Err(error) = frontend.wait_for_clients() {
            cleanup_failures.push(format!("worker reap failed: {error}"));
        }
    }
    drop(observer);
    let report = ordered_egress.report();
    let status = if service_result.is_err() {
        "error"
    } else if ordered_egress.cancelled() {
        "cancelled"
    } else {
        "drained"
    };
    if let Ok(report) = report.as_ref() {
        tracing::info!(
            "sophia_x11_authority_egress schema=1 status={} tickets_advanced={} batches_delivered={} peak_waiting_producers={} wait_episodes={} resumed={} cancelled={}",
            status,
            report.tickets_advanced,
            report.batches_delivered,
            report.peak_waiting_producers,
            report.wait_episodes,
            report.resumed,
            report.cancelled,
        );
    }
    match (service_result, report) {
        (Ok(()), Ok(_)) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
        (Err(original), report) => {
            if let Err(error) = report {
                cleanup_failures.push(format!("authority egress report failed: {error}"));
            }
            Err(original.with_cleanup_failures(cleanup_failures))
        }
    }
}

#[cfg(unix)]
fn x_authority_observation_client_error(
    error: crate::XAuthorityTransportError,
) -> X11SetupSocketError {
    match error {
        crate::XAuthorityTransportError::Cancelled { .. } => {
            X11SetupSocketError::service_shutdown(error.to_string())
        }
        crate::XAuthorityTransportError::Disconnected { .. } => {
            X11SetupSocketError::new(error.to_string())
        }
        crate::XAuthorityTransportError::Backpressure { .. } => {
            X11SetupSocketError::client_failure(error.to_string())
        }
    }
}

#[cfg(unix)]
fn trace_x_authority_backpressure(event: XAuthorityBackpressureTelemetry) {
    let client = event.client.map(XServerFrontendClientId::raw);
    let client_id = client.unwrap_or(0);
    let client_known = client.is_some();
    let waited_msec = u64::try_from(event.waited.as_millis()).unwrap_or(u64::MAX);
    let failure = match event.failure {
        None => "none",
        Some(crate::XAuthorityBackpressureFailure::Cancelled) => "cancelled",
        Some(crate::XAuthorityBackpressureFailure::Disconnected) => "disconnected",
    };
    match event.kind {
        XAuthorityBackpressureTelemetryKind::Wait => tracing::warn!(
            "sophia_x11_authority_backpressure schema=1 status=waiting client={} client_known={} transaction={} waited_msec={} failure={}",
            client_id,
            client_known,
            event.transaction.raw(),
            waited_msec,
            failure,
        ),
        XAuthorityBackpressureTelemetryKind::Resume => tracing::info!(
            "sophia_x11_authority_backpressure schema=1 status=resumed client={} client_known={} transaction={} waited_msec={} failure={}",
            client_id,
            client_known,
            event.transaction.raw(),
            waited_msec,
            failure,
        ),
        XAuthorityBackpressureTelemetryKind::Shutdown => tracing::info!(
            "sophia_x11_authority_backpressure schema=1 status=shutdown client={} client_known={} transaction={} waited_msec={} failure={}",
            client_id,
            client_known,
            event.transaction.raw(),
            waited_msec,
            failure,
        ),
        XAuthorityBackpressureTelemetryKind::TransportFailure => tracing::warn!(
            "sophia_x11_authority_backpressure schema=1 status=transport_failure client={} client_known={} transaction={} waited_msec={} failure={}",
            client_id,
            client_known,
            event.transaction.raw(),
            waited_msec,
            failure,
        ),
    }
}

/// Convenience form of [`run_x_server_frontend_routed_until_stopped`] for an
/// unauthenticated local socket using the default frontend configuration.
#[cfg(unix)]
pub fn run_x11_core_socket_server_routed_until_stopped(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    broker: XServerFrontendRouteBroker,
    service_commands: Receiver<XServerFrontendServiceCommand>,
) -> Result<(), X11SetupSocketError> {
    run_x_server_frontend_routed_until_stopped(
        XServerFrontendConfig::new(path.as_ref().to_path_buf(), namespace)?,
        transaction_sender,
        broker,
        service_commands,
    )
}

#[cfg(unix)]
fn run_x11_core_socket_server_once_with_trace_observer(
    path: impl AsRef<Path>,
    namespace: NamespaceId,
    idle_timeout: Option<Duration>,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    let listener = bind_x11_core_socket_server(path)?;
    let state = X11CoreSocketServerState::new();
    let authorization = XServerFrontendSetupAuthorization::default();
    serve_x11_core_socket_listener_once_with_setup_authorization(
        &listener,
        namespace,
        &state,
        &authorization,
        None,
        None,
        idle_timeout,
        observer,
    )
}

/// Binds the private X11 core socket, refusing any path that already exists.
///
/// THE KERNEL DECIDES, AND IT DECIDES ATOMICALLY. There is no stat, no connect
/// probe and no unlink here: `bind(2)` on a path that exists fails with
/// `EADDRINUSE` on its own, and that refusal cannot be raced. Every preflight
/// alternative is weaker. A stat cannot tell a live listener's socket from a
/// crash leftover, because both are sockets. A connect probe can, but only with
/// a window between the probe and the bind in which the owner may appear or
/// depart, and it costs a live server an accept it must drain. A lock file
/// excludes only those who agree to take the lock, so it would still leave this
/// path unlinking a pre-existing listener that never took one.
///
/// So a stale path in this mode is an explicit refusal rather than authority to
/// reclaim it. A private service that finds its socket occupied has not
/// established anything and must say so, because the alternative is what this
/// exists to prevent: unlinking a live listener's inode, binding a fresh one at
/// the same path, and leaving the displaced service serving an unreachable
/// inode while both believe they are the owner.
///
/// Reclaiming a genuinely stale path is a separate decision for whoever can
/// establish that nothing is serving it. It is not this function's to make, and
/// it is deliberately not offered as a flag here.
#[cfg(unix)]
pub fn bind_x11_core_socket_server_exclusive(
    path: impl AsRef<Path>,
) -> Result<UnixListener, X11SetupSocketError> {
    let path = path.as_ref();
    let listener = UnixListener::bind(path).map_err(|error| {
        if error.kind() == ErrorKind::AddrInUse {
            return X11SetupSocketError::new(format!(
                "refusing to bind X11 core socket {}: the path is already \
                 present and this service binds exclusively; another service \
                 may be serving it, and it is not reclaimed",
                path.display()
            ));
        }
        X11SetupSocketError::new(format!(
            "failed to bind X11 core socket {}: {error}",
            path.display()
        ))
    })?;
    restrict_x11_core_socket_to_owner(path)?;
    Ok(listener)
}

/// The permission narrowing both bind paths share.
#[cfg(unix)]
fn restrict_x11_core_socket_to_owner(path: &Path) -> Result<(), X11SetupSocketError> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|error| {
        X11SetupSocketError::new(format!(
            "failed to restrict X11 core socket {} to its owner: {error}",
            path.display()
        ))
    })
}

/// Binds the X11 core socket, removing a pre-existing socket at the path.
///
/// THE LEGACY PATH, AND IT RECLAIMS. Kept unchanged for the established callers
/// that depend on being able to restart over their own leftover socket. It
/// cannot distinguish a leftover from a live listener, which is exactly why the
/// private service uses [`bind_x11_core_socket_server_exclusive`] instead.
#[cfg(unix)]
pub fn bind_x11_core_socket_server(
    path: impl AsRef<Path>,
) -> Result<UnixListener, X11SetupSocketError> {
    let path = path.as_ref();
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            std::fs::remove_file(path).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to remove stale X11 core socket {}: {error}",
                    path.display()
                ))
            })?;
        }
        Ok(_) => {
            return Err(X11SetupSocketError::new(format!(
                "refusing to replace non-socket X11 core path {}",
                path.display()
            )));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(X11SetupSocketError::new(format!(
                "failed to inspect X11 core socket {}: {error}",
                path.display()
            )));
        }
    }

    let listener = UnixListener::bind(path).map_err(|error| {
        X11SetupSocketError::new(format!(
            "failed to bind X11 core socket {}: {error}",
            path.display()
        ))
    })?;
    restrict_x11_core_socket_to_owner(path)?;
    Ok(listener)
}

#[cfg(unix)]
pub fn serve_x11_core_socket_listener_once(
    listener: &UnixListener,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
) -> Result<(), X11SetupSocketError> {
    serve_x11_core_socket_listener_once_traced(listener, namespace, state, |_| Ok(()))
}

#[cfg(unix)]
pub fn serve_x11_core_socket_listener_once_traced(
    listener: &UnixListener,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    let authorization = XServerFrontendSetupAuthorization::default();
    serve_x11_core_socket_listener_once_with_setup_authorization(
        listener,
        namespace,
        state,
        &authorization,
        None,
        None,
        None,
        observer,
    )
}

#[cfg(unix)]
pub fn serve_x11_core_socket_listener(
    listener: &UnixListener,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
) -> Result<(), X11SetupSocketError> {
    serve_x11_core_socket_listener_traced(listener, namespace, state, |_| Ok(()))
}

#[cfg(unix)]
pub fn serve_x11_core_socket_listener_traced(
    listener: &UnixListener,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    let authorization = XServerFrontendSetupAuthorization::default();
    serve_x11_core_socket_listener_with_setup_authorization(
        listener,
        namespace,
        state,
        &authorization,
        None,
        None,
        observer,
    )
}

#[cfg(unix)]
fn serve_x11_core_socket_listener_with_setup_authorization(
    listener: &UnixListener,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    authorization: &XServerFrontendSetupAuthorization,
    admission_policy: Option<Arc<dyn XServerFrontendAdmissionPolicy>>,
    injection_policy: Option<Arc<dyn crate::XServerFrontendInjectionPolicy>>,
    mut observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    loop {
        serve_x11_core_socket_listener_once_with_setup_authorization(
            listener,
            namespace,
            state,
            authorization,
            admission_policy.clone(),
            injection_policy.clone(),
            None,
            &mut observer,
        )?;
    }
}

/// Waits for one client, giving up rather than waiting forever.
///
/// `UnixListener` has no accept deadline, so this polls a non-blocking listener
/// when one is asked for. The bound is the same the connected client gets: a
/// client that has not arrived within it is not going to.
#[cfg(unix)]
fn accept_within(
    listener: &UnixListener,
    idle_timeout: Option<Duration>,
) -> Result<
    (
        std::os::unix::net::UnixStream,
        std::os::unix::net::SocketAddr,
    ),
    X11SetupSocketError,
> {
    let Some(timeout) = idle_timeout else {
        return listener.accept().map_err(|error| {
            X11SetupSocketError::new(format!("failed to accept X11 core client: {error}"))
        });
    };
    listener.set_nonblocking(true).map_err(|error| {
        X11SetupSocketError::new(format!("failed to poll for an X11 core client: {error}"))
    })?;
    let deadline = std::time::Instant::now() + timeout;
    let accepted = loop {
        match listener.accept() {
            Ok(accepted) => break Ok(accepted),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    break Err(X11SetupSocketError::new(format!(
                        "no X11 core client connected within {}ms",
                        timeout.as_millis()
                    )));
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => {
                break Err(X11SetupSocketError::new(format!(
                    "failed to accept X11 core client: {error}"
                )));
            }
        }
    };
    // The serve loop relies on the accepted stream's read timeout rather than on
    // spinning, so both listener and stream go back to blocking.
    listener.set_nonblocking(false).map_err(|error| {
        X11SetupSocketError::new(format!("failed to restore X11 core listener: {error}"))
    })?;
    let (stream, address) = accepted?;
    stream.set_nonblocking(false).map_err(|error| {
        X11SetupSocketError::new(format!("failed to restore X11 core client: {error}"))
    })?;
    Ok((stream, address))
}

#[cfg(unix)]
fn serve_x11_core_socket_listener_once_with_setup_authorization(
    listener: &UnixListener,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    authorization: &XServerFrontendSetupAuthorization,
    admission_policy: Option<Arc<dyn XServerFrontendAdmissionPolicy>>,
    injection_policy: Option<Arc<dyn crate::XServerFrontendInjectionPolicy>>,
    idle_timeout: Option<Duration>,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    // The idle timeout below guards a client that connects and then goes quiet.
    // A client that never connects at all -- one that exits on a usage error
    // before it opens the display -- would otherwise park this thread forever,
    // and a caller waiting on it has nothing to print. Bound that wait too.
    let (mut stream, _) = accept_within(listener, idle_timeout)?;
    if let Some(timeout) = idle_timeout {
        stream.set_read_timeout(Some(timeout)).map_err(|error| {
            X11SetupSocketError::new(format!("failed to set X11 core read timeout: {error}"))
        })?;
    }
    serve_x11_core_socket_client_with_trace_observer_and_setup_authorization(
        &mut stream,
        namespace,
        state,
        authorization,
        admission_policy,
        injection_policy,
        observer,
    )
}

#[cfg(unix)]
pub fn serve_x11_setup_socket_client(
    stream: &mut UnixStream,
) -> Result<XSetupRequest, X11SetupSocketError> {
    serve_x11_setup_socket_client_with_root_size(
        stream,
        Size {
            width: i32::from(crate::X_SETUP_ROOT_WIDTH),
            height: i32::from(crate::X_SETUP_ROOT_HEIGHT),
        },
    )
}

#[cfg(unix)]
pub fn serve_x11_setup_socket_client_with_root_size(
    stream: &mut UnixStream,
    root_size: Size,
) -> Result<XSetupRequest, X11SetupSocketError> {
    let authorization = XServerFrontendSetupAuthorization::default();
    serve_x11_setup_socket_client_with_setup_authorization(stream, &authorization, |_| {
        let mut success = XSetupSuccess::client_compatible();
        success.root_size = root_size;
        Ok(Some(success))
    })?
    .map(|(request, _)| request)
    .ok_or_else(|| {
        X11SetupSocketError::new("default X11 setup authorization unexpectedly rejected")
    })
}

#[cfg(unix)]
fn serve_x11_setup_socket_client_with_setup_authorization(
    stream: &mut UnixStream,
    authorization: &XServerFrontendSetupAuthorization,
    setup_success: impl FnOnce(&XSetupRequest) -> Result<Option<XSetupSuccess>, X11SetupSocketError>,
) -> Result<Option<(XSetupRequest, XSetupSuccess)>, X11SetupSocketError> {
    let request = read_x11_setup_request(stream)?;
    if !authorization.permits(&request) {
        write_x11_setup_failure(
            stream,
            request.byte_order,
            b"Sophia X11 authorization failed",
        )?;
        return Ok(None);
    }
    let Some(setup_success) = setup_success(&request)? else {
        write_x11_setup_failure(stream, request.byte_order, b"Sophia X11 admission failed")?;
        return Ok(None);
    };
    let response =
        encode_x11_setup_success(request.byte_order, &setup_success).map_err(|error| {
            X11SetupSocketError::new(format!("failed to encode X11 setup success: {error}"))
        })?;
    // The client may have gone between sending its setup and reading the
    // answer. Writing into a closed peer is that client ending, not this
    // server failing, and must not be reported as a service fault.
    stream
        .write_all(&response)
        .map_err(|error| setup_write_failure("setup", &error))?;
    stream
        .flush()
        .map_err(|error| setup_write_failure("setup", &error))?;
    Ok(Some((request, setup_success)))
}

#[cfg(unix)]
fn write_x11_setup_failure(
    stream: &mut UnixStream,
    byte_order: XByteOrder,
    reason: &[u8],
) -> Result<(), X11SetupSocketError> {
    let response =
        encode_x11_setup_failure(byte_order, &XSetupFailure::new(reason)).map_err(|error| {
            X11SetupSocketError::new(format!("failed to encode X11 setup failure: {error}"))
        })?;
    // A refused client is especially likely to be gone already.
    stream
        .write_all(&response)
        .map_err(|error| setup_write_failure("setup failure", &error))?;
    stream
        .flush()
        .map_err(|error| setup_write_failure("setup failure", &error))
}

#[cfg(unix)]
pub fn serve_x11_core_socket_client(
    stream: &mut UnixStream,
    namespace: NamespaceId,
) -> Result<(), X11SetupSocketError> {
    let state = X11CoreSocketServerState::new();
    serve_x11_core_socket_client_with_state(stream, namespace, &state)
}

#[cfg(unix)]
pub fn serve_x11_core_socket_client_with_state(
    stream: &mut UnixStream,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
) -> Result<(), X11SetupSocketError> {
    serve_x11_core_socket_client_with_trace_observer(stream, namespace, state, |_| Ok(()))
}

#[cfg(unix)]
pub fn serve_x11_core_socket_client_observed(
    stream: &mut UnixStream,
    namespace: NamespaceId,
    mut observer: impl FnMut(&XDispatchResult),
) -> Result<(), X11SetupSocketError> {
    let state = X11CoreSocketServerState::new();
    serve_x11_core_socket_client_with_state_observed(stream, namespace, &state, move |result| {
        observer(result);
        Ok(())
    })
}

#[cfg(unix)]
pub fn serve_x11_core_socket_client_with_state_observed(
    stream: &mut UnixStream,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    mut observer: impl FnMut(&XDispatchResult) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    serve_x11_core_socket_client_with_trace_observer(stream, namespace, state, move |trace| {
        observer(&trace.result)
    })
}

#[cfg(unix)]
fn serve_x11_core_socket_client_with_trace_observer(
    stream: &mut UnixStream,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    let authorization = XServerFrontendSetupAuthorization::default();
    serve_x11_core_socket_client_with_trace_observer_and_setup_authorization(
        stream,
        namespace,
        state,
        &authorization,
        None,
        None,
        observer,
    )
}

#[cfg(unix)]
fn serve_x11_core_socket_client_with_trace_observer_and_setup_authorization(
    stream: &mut UnixStream,
    namespace: NamespaceId,
    state: &X11CoreSocketServerState,
    authorization: &XServerFrontendSetupAuthorization,
    admission_policy: Option<Arc<dyn XServerFrontendAdmissionPolicy>>,
    injection_policy: Option<Arc<dyn crate::XServerFrontendInjectionPolicy>>,
    mut observer: impl FnMut(X11DispatchObservation) -> Result<(), X11SetupSocketError>,
) -> Result<(), X11SetupSocketError> {
    serve_x11_core_socket_client_with_trace_observer_and_input(
        stream,
        namespace,
        state,
        X11ClientConnectionInputs {
            input_receiver: None,
            control_channels: None,
            client_routing: None,
        },
        X11ClientAdmissionContext {
            authorization,
            admission_policy,
            injection_policy,
            worker_admission: None,
        },
        move |trace| observer(trace).map(|()| None),
    )
}
