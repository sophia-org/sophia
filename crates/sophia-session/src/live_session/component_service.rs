//! Owner-loop service of independently negotiated component roles.
use super::*;
mod native_input;
#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
use crate::shell_component_connections::combined_row::{PassSlot, WitnessPass, witness};
use metadata_shell::component_session::{ShellComponentService, ShellComponentSession};
use metadata_shell::indicators::IndicatorServiceError;
pub(super) use native_input::{dispatch_native_input, synchronize_native_capture};

#[allow(clippy::too_many_arguments)]
pub(super) fn service_components(
    components: &mut ShellComponentSession,
    catalog: &mut component_catalog::ComponentCatalog,
    runtime: &mut LiveProductionVisualRuntime,
    scene: &LiveProductionCpuScene,
    mut native: Option<&mut LiveProductionNativeScanout>,
    outputs: &[sophia_engine::HeadlessOutput],
    bounds: &[(sophia_protocol::OutputId, sophia_protocol::Rect)],
    wm: &mut Option<LiveWmSession>,
    available: bool,
    launches: &mut SessionLaunchQueue,
    children: &mut Vec<ManagedSessionChild>,
    config: &PersistentXtermSessionConfig,
    xauthority: &std::path::Path,
    admission_started: &mut Option<Instant>,
    authenticated_launches: &mut crate::launch_authentication::LaunchCoordinator,
    owner_wake: &OwnerWake,
    application_launches: &mut LiveApplicationLaunches,
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session"
    ))]
    position: Option<sophia_protocol::Point>,
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session",
        feature = "diagnostic-combined-witness"
    ))]
    exit: &PassEnd,
) -> Result<(), Box<dyn std::error::Error>> {
    if !available {
        catalog.cancel_open_request();
    }
    crate::combined_witness_check!(
        exit,
        "error:presentation_available",
        components.set_presentation_available(available)
    )?;
    // Gate 4 C3: with native scanout, this pass's image services are hooked
    // over the combined row from one turn built here, at the pass's one
    // Instant, cursor and layer snapshot, and dropped before role service,
    // which borrows the runtime mutably. Without native scanout the pass is
    // today's: the ordinary poll, ordinary content, no walk; the row keeps
    // whatever it holds.
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session"
    ))]
    let instant = Instant::now();
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session"
    ))]
    let (cursor, plane) =
        crate::shell_component_connections::combined_row::turn_cursor(native.is_some(), position);
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session"
    ))]
    let layers = components.content_layers();
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session"
    ))]
    let polled = match native.as_deref_mut() {
        Some(native) => components.poll_authenticated_combined(
            64 * 1024,
            authenticated_launches,
            owner_wake.notifier(),
            &mut crate::shell_component_connections::combined_row::CombinedTurn {
                native,
                runtime: &*runtime,
                scene,
                outputs,
                layers,
                cursor,
                plane,
                instant,
                #[cfg(all(
                    feature = "experimental-combined-frame-job",
                    feature = "experimental-native-frame-transaction",
                    feature = "native-session",
                    feature = "diagnostic-combined-witness"
                ))]
                witness: exit.pass(),
            },
        ),
        None => {
            components.poll_authenticated(64 * 1024, authenticated_launches, owner_wake.notifier())
        }
    };
    #[cfg(not(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session"
    )))]
    let polled =
        components.poll_authenticated(64 * 1024, authenticated_launches, owner_wake.notifier());
    match polled {
        Ok(visit) => {
            for (key, result) in visit.negotiations.into_iter().flatten() {
                match result {
                    Ok(welcome) => {
                        // A negotiation completed during a pause may already
                        // have been revoked by poll; do not report it admitted.
                        if !components
                            .connected_roles()
                            .into_iter()
                            .flatten()
                            .any(|(current, _)| current == key)
                        {
                            continue;
                        }
                        let (role, gpu) = crate::combined_witness_check!(
                            exit,
                            "error:negotiation",
                            components.launch_evidence(key)
                        )?;
                        if welcome.capabilities
                            & sophia_protocol::shell_files::SHELL_FRAME_CAPABILITY
                            != 0
                        {
                            crate::session_println!(
                                "sophia_native_frame_negotiated schema=2 slot={} connection_epoch={} content_grant_epoch={} revision={} capabilities={:#x} terminal_only={} lean={}",
                                key.slot,
                                key.grant.connection_epoch,
                                key.grant.content_grant_epoch,
                                welcome.selected_revision,
                                welcome.capabilities,
                                welcome.capabilities & sophia_protocol::shell_files::SHELL_FRAME_TERMINAL_ONLY_CAPABILITY != 0,
                                welcome.capabilities & sophia_protocol::shell_files::SHELL_FRAME_LEAN_CAPABILITY != 0,
                            );
                        }
                        let role = match role {
                            sophia_config::ShellComponentRole::Descriptor => "descriptor",
                            sophia_config::ShellComponentRole::Bar => "bar",
                            sophia_config::ShellComponentRole::Dock => "dock",
                            sophia_config::ShellComponentRole::ApplicationLauncher => {
                                "application_launcher"
                            }
                        };
                        crate::session_println!(
                            "sophia_shell_component schema=1 status=negotiated slot={} role={} connection_epoch={} content_grant_epoch={} revision={} gpu_mode={} gpu_grant_epoch={} device_major={} device_minor={}",
                            key.slot,
                            role,
                            key.grant.connection_epoch,
                            key.grant.content_grant_epoch,
                            welcome.selected_revision,
                            if gpu.is_some() { "direct" } else { "denied" },
                            gpu.map_or(0, |g| g.epoch),
                            gpu.map_or(0, |g| g.major),
                            gpu.map_or(0, |g| g.minor),
                        );
                    }
                    Err(error) => crate::session_eprintln!(
                        "sophia_shell_component schema=1 status=negotiation_failed slot={} connection_epoch={} content_grant_epoch={} reason={error}",
                        key.slot,
                        key.grant.connection_epoch,
                        key.grant.content_grant_epoch,
                    ),
                }
            }
            for event in visit.processes.into_iter().flatten() {
                use crate::shell_component_processes::ComponentProcessEvent;
                match event {
                    ComponentProcessEvent::ProcessRetired(key, result) => crate::session_println!(
                        "sophia_shell_component schema=1 status=process_retired slot={} connection_epoch={} content_grant_epoch={} endpoint_released={}",
                        key.slot,
                        key.grant.connection_epoch,
                        key.grant.content_grant_epoch,
                        result.is_ok(),
                    ),
                    ComponentProcessEvent::Failed(key, error) => crate::session_eprintln!(
                        "sophia_shell_component schema=1 status=process_failed slot={} connection_epoch={} content_grant_epoch={} reason={error}",
                        key.slot,
                        key.grant.connection_epoch,
                        key.grant.content_grant_epoch,
                    ),
                }
            }
            for (key, error) in visit.stop_errors.into_iter().flatten() {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=stop_failed slot={} reason={error}",
                    key.slot
                );
            }
        }
        Err(error) => {
            crate::combined_witness!(exit.poll_failed(););
            crate::session_eprintln!(
                "sophia_shell_component schema=1 status=poll_failed reason={error}"
            );
        }
    }
    crate::combined_witness_check!(
        exit,
        "error:settle_revocations_early",
        components.settle_revocations(Some(runtime))
    )?;
    // Native startup may negotiate once its source snapshot is ready. Opening
    // and input remain separately gated by current native presentation authority.
    if let Err(error) = components.start_next(Instant::now(), |role| {
        role == sophia_config::ShellComponentRole::Bar || catalog.ready()
    }) {
        // `slot` is already an approved field for this record; without it a
        // retained start failure reduces to schema and status alone and cannot
        // name the component that failed. A failure raised before any slot was
        // selected has none to report.
        // `reason` is free text and reduction drops it, so the refusal also
        // carries an approved code. Without one a retained record says that a
        // start failed and never why, which cost 841 records and a
        // configuration change to answer once.
        let cause = crate::component_start_cause::classify(&error.to_string()).as_str();
        match components.last_start_slot() {
            Some(slot) => crate::session_eprintln!(
                "sophia_shell_component schema=1 status=start_failed slot={slot} cause={cause} reason={error}"
            ),
            None => crate::session_eprintln!(
                "sophia_shell_component schema=1 status=start_failed cause={cause} reason={error}"
            ),
        }
    }
    // Recorded on the visit the spacing widens, not on every attempt after it.
    // A reader seeing this knows the retries that follow are deliberately
    // sparse rather than stalled.
    if let Some(slot) = components.entered_backoff() {
        crate::session_eprintln!(
            "sophia_shell_component schema=1 status=start_backoff slot={slot}"
        );
    }
    crate::combined_witness_check!(
        exit,
        "error:reconcile_early",
        reconcile_catalog_connections(components, catalog, launches)
    )?;
    if !available {
        // Gate 4 C3: the paused pass still walks the row, so Held results,
        // Release debt and unconfirmed cancellations are serviced while
        // presentation is unavailable; a slot's failure is logged, as the
        // image drain's is, and not retried.
        #[cfg(all(
            feature = "experimental-combined-frame-job",
            feature = "experimental-native-frame-transaction",
            feature = "native-session"
        ))]
        if let Some(native) = native.as_deref_mut() {
            for (key, error) in components
                .turn_combined_walk(
                    native,
                    instant,
                    #[cfg(all(
                        feature = "experimental-combined-frame-job",
                        feature = "experimental-native-frame-transaction",
                        feature = "native-session",
                        feature = "diagnostic-combined-witness"
                    ))]
                    exit.pass(),
                )
                .into_iter()
                .flatten()
            {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=image_drain_failed slot={} reason={error}",
                    key.slot
                );
            }
        }
        crate::combined_witness_check!(
            exit,
            "error:service_execution_paused",
            catalog.service_execution(
                None,
                config,
                xauthority,
                launches,
                children,
                admission_started,
                &mut ApplicationLaunchTurn {
                    applications: application_launches,
                    authentication: authenticated_launches,
                    wake: owner_wake.notifier(),
                },
            )
        )?;
        crate::combined_witness!(exit.at("paused"););
        return Ok(());
    }
    let root = crate::combined_witness_check!(
        exit,
        "error:root_bounds",
        wm_root_bounds(bounds).ok_or("component content has no output bounds")
    )?;
    let publication = wm
        .as_mut()
        .and_then(LiveWmSession::shell_indicator_publication);
    let active_output = wm.as_ref().and_then(LiveWmSession::active_output);
    for (key, role) in components.connected_roles().into_iter().flatten() {
        if role == sophia_config::ShellComponentRole::Dock {
            let result = crate::combined_witness_check!(
                exit,
                "error:role_loop",
                components.with_service(key, |service, transport| {
                    let ShellComponentService::Dock(dock) = service else {
                        return Err("catalog component role mismatch".into());
                    };
                    if catalog.publish(transport)? {
                        catalog.service_dock(
                            dock,
                            transport,
                            runtime,
                            scene,
                            native.as_deref_mut(),
                            outputs,
                            bounds,
                            root,
                            launches,
                            children.len(),
                        )?;
                    }
                    Ok::<_, Box<dyn std::error::Error>>(())
                })
            )?;
            if let Err(error) = result {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=dock_failed slot={} reason={error}",
                    key.slot
                );
                launches.revoke_native_catalog_grant(key.grant);
                crate::combined_witness_check!(exit, "error:role_loop", components.stop(key))?;
            }
            continue;
        }
        if role == sophia_config::ShellComponentRole::ApplicationLauncher {
            let result = crate::combined_witness_check!(
                exit,
                "error:role_loop",
                components.with_service(key, |service, transport| {
                    let ShellComponentService::Launcher { content, actions } = service else {
                        return Err("native component role mismatch".into());
                    };
                    let complete = catalog.publish(transport)?;
                    transport.poll_io()?;
                    if complete {
                        catalog.service_actions(
                            actions,
                            transport,
                            runtime,
                            launches,
                            children.len(),
                        )?;
                        content.close_admitted(transport, catalog.mint_transaction()?)?;
                        catalog.service_open_content(
                            content,
                            transport,
                            runtime,
                            scene,
                            native.as_deref_mut(),
                            outputs,
                            bounds,
                            root,
                        )?;
                    }
                    Ok::<_, Box<dyn std::error::Error>>(complete)
                })
            )?;
            if let Err(error) = result {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=catalog_failed slot={} reason={error}",
                    key.slot
                );
                launches.revoke_native_catalog_grant(key.grant);
                crate::combined_witness_check!(exit, "error:role_loop", components.stop(key))?;
                catalog.cancel_open_request();
            }
            continue;
        }
        let serve_bar = |service: &mut ShellComponentService,
                         transport: &mut sophia_runtime::ShellTransportConnection<'_>,
                         #[cfg(all(feature = "experimental-combined-frame-job", feature = "experimental-native-frame-transaction", feature = "native-session"))]
                         row: &mut crate::shell_component_connections::combined_row::CombinedRow| {
            let ShellComponentService::Bar(bar) = service else {
                return Err(IndicatorServiceError::Poll(
                    "bar service role mismatch".into(),
                ));
            };
            let content = (|| -> Result<(), Box<dyn std::error::Error>> {
                bar.service_indicators(transport, publication.as_deref(), active_output)?;
                let presented = runtime
                    .input_projections()
                    .iter()
                    .flat_map(|projection| projection.content.iter());
                bar.service_actions_from(transport, presented)?;
                #[cfg(not(all(
                    feature = "experimental-combined-frame-job",
                    feature = "experimental-native-frame-transaction",
                    feature = "native-session"
                )))]
                bar.service_content(
                    transport,
                    runtime,
                    scene,
                    native.as_deref_mut(),
                    outputs,
                    bounds,
                    root,
                )?;
                // Gate 4 C3: with native scanout the row is lent to the bar's
                // content service under this pass's cursor and Instant; without
                // it `combined` is None, today's ordinary content.
                #[cfg(all(
                    feature = "experimental-combined-frame-job",
                    feature = "experimental-native-frame-transaction",
                    feature = "native-session"
                ))]
                {
                    let present = native.is_some();
                    bar.service_content_on(
                        transport,
                        runtime,
                        scene,
                        crate::live_session::metadata_shell::ContentNative {
                            native: native.as_deref_mut(),
                            combined: present.then_some(
                                crate::shell_component_connections::combined_row::CombinedConsume {
                                    row,
                                    cursor,
                                    plane,
                                    instant,
                                    #[cfg(all(
                                        feature = "experimental-combined-frame-job",
                                        feature = "experimental-native-frame-transaction",
                                        feature = "native-session",
                                        feature = "diagnostic-combined-witness"
                                    ))]
                                    witness: PassSlot {
                                        pass: exit.pass(),
                                        slot: key.slot,
                                    },
                                },
                            ),
                        },
                        outputs,
                        bounds,
                        root,
                    )?;
                }
                bar.observe_presentation(transport, runtime)?;
                Ok(())
            })();
            content.map_err(IndicatorServiceError::Poll)?;
            bar.service_indicator_activation(transport, |action, output| {
                wm.as_mut()
                    .map_or(Ok(LiveIndicatorAdmissionResult::unavailable()), |wm| {
                        wm.enqueue_indicator_action(action, output)
                    })
            })?;
            Ok(())
        };
        #[cfg(not(all(
            feature = "experimental-combined-frame-job",
            feature = "experimental-native-frame-transaction",
            feature = "native-session"
        )))]
        let result = crate::combined_witness_check!(
            exit,
            "error:role_loop",
            components.with_service(key, serve_bar)
        )?;
        #[cfg(all(
            feature = "experimental-combined-frame-job",
            feature = "experimental-native-frame-transaction",
            feature = "native-session"
        ))]
        let result = crate::combined_witness_check!(
            exit,
            "error:role_loop",
            components.with_combined_service(key, serve_bar)
        )?;
        match result {
            Ok(()) => {}
            // Preserve the production distinction: an admitted WM effect is
            // never turned into a connection retry with uncertain replay.
            Err(IndicatorServiceError::Completion(error)) => {
                crate::combined_witness!(exit.at("error:completion"););
                return Err(error);
            }
            Err(IndicatorServiceError::Poll(error)) => {
                crate::combined_witness_check!(
                    exit,
                    "error:role_loop",
                    components.fail_service(key, &error)
                )?;
            }
        }
    }
    crate::combined_witness_check!(
        exit,
        "error:reconcile",
        reconcile_catalog_connections(components, catalog, launches)
    )?;
    // Gate 4 C3: the row's walk, after content and before native service; a
    // failure of a slot's current attempt ends that component as a content
    // failure would. Entries are never disposed of by inference.
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        feature = "experimental-native-frame-transaction",
        feature = "native-session"
    ))]
    if let Some(native) = native {
        for (key, error) in components
            .turn_combined_walk(
                native,
                instant,
                #[cfg(all(
                    feature = "experimental-combined-frame-job",
                    feature = "experimental-native-frame-transaction",
                    feature = "native-session",
                    feature = "diagnostic-combined-witness"
                ))]
                exit.pass(),
            )
            .into_iter()
            .flatten()
        {
            crate::combined_witness_check!(
                exit,
                "error:walk_fail_service",
                components.fail_service(key, &error)
            )?;
        }
    }
    // The single verification worker is visited once, with its exact current
    // connection. An unrelated peer cannot revoke or consume its pending result.
    let owner = catalog.execution_owner(launches);
    let key = components
        .connected_roles()
        .into_iter()
        .flatten()
        .find(|(key, _)| Some(key.grant) == owner)
        .map(|(key, _)| key);
    if let Some(key) = key {
        let execution = crate::combined_witness_check!(
            exit,
            "error:role_loop_service",
            components.with_service(key, |_, transport| {
                catalog.service_execution(
                    Some(transport),
                    config,
                    xauthority,
                    launches,
                    children,
                    admission_started,
                    &mut ApplicationLaunchTurn {
                        applications: application_launches,
                        authentication: authenticated_launches,
                        wake: owner_wake.notifier(),
                    },
                )
            })
        )?;
        crate::combined_witness_check!(exit, "error:service_execution", execution)?;
    } else {
        crate::combined_witness_check!(
            exit,
            "error:service_execution",
            catalog.service_execution(
                None,
                config,
                xauthority,
                launches,
                children,
                admission_started,
                &mut ApplicationLaunchTurn {
                    applications: application_launches,
                    authentication: authenticated_launches,
                    wake: owner_wake.notifier(),
                },
            )
        )?;
    }
    crate::combined_witness_check!(
        exit,
        "error:settle_revocations",
        components.settle_revocations(Some(runtime))
    )?;
    crate::combined_witness!(exit.at("normal"););
    Ok(())
}

fn reconcile_catalog_connections(
    components: &ShellComponentSession,
    catalog: &mut component_catalog::ComponentCatalog,
    launches: &mut SessionLaunchQueue,
) -> Result<(), Box<dyn std::error::Error>> {
    let connected = components.connected_roles();
    let mut grants = [sophia_protocol::ContentGrant {
        connection_epoch: 0,
        content_grant_epoch: 0,
    }; sophia_config::MAX_SHELL_COMPONENTS];
    let mut count = 0;
    for (key, role) in connected.into_iter().flatten() {
        if matches!(
            role,
            sophia_config::ShellComponentRole::ApplicationLauncher
                | sophia_config::ShellComponentRole::Dock
        ) {
            grants[count] = key.grant;
            count += 1;
        }
    }
    catalog.reconcile_connections(&grants[..count], launches)
}

pub(super) fn issue_component_activation(
    components: &mut ShellComponentSession,
    target: sophia_engine::PresentedContentTarget,
    runtime: &LiveProductionVisualRuntime,
    catalog: &mut component_catalog::ComponentCatalog,
) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    let Some((key, _role)) = components
        .connected_roles()
        .into_iter()
        .flatten()
        .find(|(key, _)| key.grant == target.grant)
    else {
        return Ok(None);
    };
    let result = components.with_service(key, |service, transport| match service {
        ShellComponentService::Bar(bar) => bar.issue_activation(transport, target, runtime),
        ShellComponentService::Dock(dock) => dock.issue_activation(transport, target, runtime),
        ShellComponentService::Launcher { actions, .. } => {
            let transaction = catalog.mint_transaction()?;
            let now = catalog.action_now_msec()?;
            Ok(actions.issue(transport, target, transaction, now)?)
        }
    })?;
    match result {
        Ok(event) => Ok(event),
        Err(error) => {
            crate::session_eprintln!(
                "sophia_shell_component schema=1 status=input_failed slot={} reason={error}",
                key.slot
            );
            components.stop(key)?;
            Ok(None)
        }
    }
}

/// Gate 4 C3b: the component pass's exit record (spec 04 R3). A label is set
/// only when a wrapped outer result IS an error, so a successful fallible
/// statement leaves nothing behind and an unwrapped exit prints
/// `error:unlabelled`; the two plain returns set `paused` and `normal`. The
/// end record is printed once: by `finish` after the pass returned, or by
/// `Drop` while unwinding. Copy data and cells only; it borrows nothing of
/// the pass and changes no `?`, return or error value.
#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
pub(super) struct PassEnd {
    pass: WitnessPass,
    label: std::cell::Cell<Option<&'static str>>,
    poll_failed: std::cell::Cell<bool>,
    finished: std::cell::Cell<bool>,
}

#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
impl PassEnd {
    fn new(pass: WitnessPass) -> Self {
        Self {
            pass,
            label: std::cell::Cell::new(None),
            poll_failed: std::cell::Cell::new(false),
            finished: std::cell::Cell::new(false),
        }
    }

    pub(super) fn pass(&self) -> WitnessPass {
        self.pass
    }

    /// The result unchanged; the label is kept only when it is an error.
    pub(super) fn check<T, E>(&self, label: &'static str, result: Result<T, E>) -> Result<T, E> {
        if result.is_err() {
            self.label.set(Some(label));
        }
        result
    }

    /// A plain return's label, set on the line before it.
    pub(super) fn at(&self, label: &'static str) {
        self.label.set(Some(label));
    }

    /// The poll's log-and-continue error: not an exit, recorded on the end.
    pub(super) fn poll_failed(&self) {
        self.poll_failed.set(true);
    }

    fn finish(&self, slots_after: &[usize]) {
        self.finished.set(true);
        witness::cpass_end(
            self.pass,
            self.label.get().unwrap_or("error:unlabelled"),
            self.poll_failed.get(),
            slots_after,
        );
    }
}

#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
impl Drop for PassEnd {
    fn drop(&mut self) {
        if self.finished.get() {
            return;
        }
        let exit = if std::thread::panicking() {
            "unwinding"
        } else {
            self.label.get().unwrap_or("error:unlabelled")
        };
        witness::cpass_end(self.pass, exit, self.poll_failed.get(), &[]);
    }
}

/// Gate 4 C3b: `service_components` inside its witness records. The pass
/// begins with the slots expected to be visited, read before the poll; the
/// body runs with the exit guard; the end record carries the slots after
/// it. No decision, wait, retry or borrow is added to the pass.
#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
#[allow(clippy::too_many_arguments)]
pub(super) fn service_components_witnessed(
    iteration: Option<u64>,
    components: &mut ShellComponentSession,
    catalog: &mut component_catalog::ComponentCatalog,
    runtime: &mut LiveProductionVisualRuntime,
    scene: &LiveProductionCpuScene,
    native: Option<&mut LiveProductionNativeScanout>,
    outputs: &[sophia_engine::HeadlessOutput],
    bounds: &[(sophia_protocol::OutputId, sophia_protocol::Rect)],
    wm: &mut Option<LiveWmSession>,
    available: bool,
    launches: &mut SessionLaunchQueue,
    children: &mut Vec<ManagedSessionChild>,
    config: &PersistentXtermSessionConfig,
    xauthority: &std::path::Path,
    admission_started: &mut Option<Instant>,
    authenticated_launches: &mut crate::launch_authentication::LaunchCoordinator,
    owner_wake: &OwnerWake,
    application_launches: &mut LiveApplicationLaunches,
    position: Option<sophia_protocol::Point>,
) -> Result<(), Box<dyn std::error::Error>> {
    let pass = WitnessPass {
        iteration,
        cpass: components.begin_combined_pass(),
    };
    witness::cpass_begin(
        pass,
        native.is_some(),
        !available,
        &components.witness_expected_slots(),
    );
    let exit = PassEnd::new(pass);
    let result = service_components(
        components,
        catalog,
        runtime,
        scene,
        native,
        outputs,
        bounds,
        wm,
        available,
        launches,
        children,
        config,
        xauthority,
        admission_started,
        authenticated_launches,
        owner_wake,
        application_launches,
        position,
        &exit,
    );
    exit.finish(&components.witness_expected_slots());
    result
}
