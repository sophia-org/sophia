//! Owner-loop service of independently negotiated component roles.
use super::*;
use metadata_shell::component_session::{ShellComponentService, ShellComponentSession};
use metadata_shell::indicators::IndicatorServiceError;

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
) -> Result<(), Box<dyn std::error::Error>> {
    components.set_presentation_available(available)?;
    match components.poll(64 * 1024) {
        Ok(visit) => {
            for (key, result) in visit.negotiations.into_iter().flatten() {
                match result {
                    Ok(welcome) => crate::session_println!(
                        "sophia_shell_component schema=1 status=negotiated slot={} connection_epoch={} revision={}",
                        key.slot,
                        key.grant.connection_epoch,
                        welcome.selected_revision,
                    ),
                    Err(error) => crate::session_eprintln!(
                        "sophia_shell_component schema=1 status=negotiation_failed slot={} reason={error}",
                        key.slot,
                    ),
                }
            }
            for event in visit.processes.into_iter().flatten() {
                crate::session_println!(
                    "sophia_shell_component schema=1 status=process_event event={event:?}"
                );
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
    // and input remain separately gated; the public config guard is still held.
    if let Err(error) = components.start_next(Instant::now(), |role| {
        role == sophia_config::ShellComponentRole::Bar || catalog.ready()
    }) {
        crate::session_eprintln!(
            "sophia_shell_component schema=1 status=start_failed reason={error}"
        );
    }
    if !available {
        return Ok(());
    }
    let bounds = wm_output_bounds(outputs);
    let root = wm_root_bounds(&bounds).ok_or("component content has no output bounds")?;
    let publication = wm.as_ref().and_then(LiveWmSession::indicator_publication);
    let active_output = wm.as_ref().and_then(LiveWmSession::active_output);
    for (key, role) in components.connected_roles().into_iter().flatten() {
        if role == sophia_config::ShellComponentRole::ApplicationLauncher {
            let result = components.with_service(key, |_, transport| {
                let complete = catalog.publish(transport)?;
                transport.poll_io_bounded(64 * 1024)?;
                Ok::<_, Box<dyn std::error::Error>>(complete)
            })?;
            if let Err(error) = result {
                crate::session_eprintln!(
                    "sophia_shell_component schema=1 status=catalog_failed slot={} reason={error}",
                    key.slot
                );
                components.stop(key)?;
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
    components.settle_revocations(Some(runtime))?;
    Ok(())
}

pub(super) fn issue_panel_activation(
    components: &mut ShellComponentSession,
    target: sophia_engine::PresentedContentTarget,
    runtime: &LiveProductionVisualRuntime,
) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    let Some((key, role)) = components
        .connected_roles()
        .into_iter()
        .flatten()
        .find(|(key, _)| key.grant == target.grant)
    else {
        return Ok(None);
    };
    if role != sophia_config::ShellComponentRole::Bar {
        return Ok(None);
    }
    let result = components.with_service(key, |service, transport| {
        let ShellComponentService::Bar(bar) = service else {
            return Ok(None);
        };
        bar.issue_activation(transport, target, runtime)
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
