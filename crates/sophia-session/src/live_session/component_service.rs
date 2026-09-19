//! Owner-loop service of independently negotiated component roles.
use super::*;
mod native_input;
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
    wm: &mut Option<LiveWmSession>,
    available: bool,
    launches: &mut SessionLaunchQueue,
    children: &mut Vec<ManagedSessionChild>,
    config: &PersistentXtermSessionConfig,
    xauthority: &std::path::Path,
    admission_started: &mut Option<Instant>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !available {
        catalog.cancel_open_request();
    }
    components.set_presentation_available(available)?;
    match components.poll(64 * 1024) {
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
                        let (role, gpu) = components.launch_evidence(key)?;
                        let role = match role {
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
        Err(error) => crate::session_eprintln!(
            "sophia_shell_component schema=1 status=poll_failed reason={error}"
        ),
    }
    components.settle_revocations(Some(runtime))?;
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
        let cause = super::component_start_cause::classify(&error.to_string()).as_str();
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
    reconcile_catalog_connections(components, catalog, launches)?;
    if !available {
        catalog.service_execution(
            None,
            config,
            xauthority,
            launches,
            children,
            admission_started,
        )?;
        return Ok(());
    }
    let bounds = wm_output_bounds(outputs);
    let root = wm_root_bounds(&bounds).ok_or("component content has no output bounds")?;
    let publication = wm.as_ref().and_then(LiveWmSession::indicator_publication);
    let active_output = wm.as_ref().and_then(LiveWmSession::active_output);
    for (key, role) in components.connected_roles().into_iter().flatten() {
        if role == sophia_config::ShellComponentRole::Dock {
            let result = components.with_service(key, |service, transport| {
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
                        &bounds,
                        root,
                        launches,
                        children.len(),
                    )?;
                }
                Ok::<_, Box<dyn std::error::Error>>(())
            })?;
            if let Err(error) = result {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=dock_failed slot={} reason={error}",
                    key.slot
                );
                launches.revoke_native_catalog_grant(key.grant);
                components.stop(key)?;
            }
            continue;
        }
        if role == sophia_config::ShellComponentRole::ApplicationLauncher {
            let result = components.with_service(key, |service, transport| {
                let ShellComponentService::Launcher { content, actions } = service else {
                    return Err("native component role mismatch".into());
                };
                let complete = catalog.publish(transport)?;
                transport.poll_io_bounded(64 * 1024)?;
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
                        &bounds,
                        root,
                    )?;
                }
                Ok::<_, Box<dyn std::error::Error>>(complete)
            })?;
            if let Err(error) = result {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=catalog_failed slot={} reason={error}",
                    key.slot
                );
                launches.revoke_native_catalog_grant(key.grant);
                components.stop(key)?;
                catalog.cancel_open_request();
            }
            continue;
        }
        let result = components.with_service(key, |service, transport| {
            let ShellComponentService::Bar(bar) = service else {
                return Err(IndicatorServiceError::Poll(
                    "bar service role mismatch".into(),
                ));
            };
            let content = (|| -> Result<(), Box<dyn std::error::Error>> {
                bar.service_indicators(transport, publication.as_ref(), active_output)?;
                let presented = runtime
                    .input_projections()
                    .iter()
                    .flat_map(|projection| projection.content.iter().cloned())
                    .collect::<Vec<_>>();
                bar.service_actions(transport, &presented)?;
                bar.service_content(
                    transport,
                    runtime,
                    scene,
                    native.as_deref_mut(),
                    outputs,
                    &bounds,
                    root,
                )?;
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
        })?;
        match result {
            Ok(()) => {}
            // Preserve the production distinction: an admitted WM effect is
            // never turned into a connection retry with uncertain replay.
            Err(IndicatorServiceError::Completion(error)) => return Err(error),
            Err(IndicatorServiceError::Poll(error)) => {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=service_failed slot={} reason={error}",
                    key.slot
                );
                components.stop(key)?;
            }
        }
    }
    reconcile_catalog_connections(components, catalog, launches)?;
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
        components.with_service(key, |_, transport| {
            catalog.service_execution(
                Some(transport),
                config,
                xauthority,
                launches,
                children,
                admission_started,
            )
        })??;
    } else {
        catalog.service_execution(
            None,
            config,
            xauthority,
            launches,
            children,
            admission_started,
        )?;
    }
    components.settle_revocations(Some(runtime))?;
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
