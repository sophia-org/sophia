//! Outer Session custody across owner-loop success and error returns.
use super::*;
use metadata_shell::component_session::ShellComponentSession;

/// The retained token for a component's role, as reduction admits it.
fn role_token(role: sophia_config::ShellComponentRole) -> &'static str {
    match role {
        sophia_config::ShellComponentRole::Bar => "bar",
        sophia_config::ShellComponentRole::Dock => "dock",
        sophia_config::ShellComponentRole::ApplicationLauncher => "application_launcher",
    }
}

/// One record per component refused by a direct-grant refusal, in declared
/// order. A denied component asked for no grant and is never refused here.
///
/// SLOT IS THE DECLARED POSITION, NOT A RUNTIME ONE. A refused component never
/// reaches the process layer and is never assigned a slot there, so this names
/// the entry in the operator's profile, which is what they can act on. Carrying
/// it at all is the repair this path existed to make: the 841 records that
/// opened the investigation could not name the component that failed.
pub(super) fn refusal_records(
    selected: &[sophia_config::ShellComponentConfig],
    error: &str,
) -> Vec<String> {
    let cause = crate::component_start_cause::classify(error).as_str();
    selected
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.gpu == sophia_config::ShellGpuMode::Direct)
        .map(|(slot, entry)| {
            format!(
                "sophia_shell_component schema=1 status=start_refused cause={cause} slot={slot} role={} gpu_mode=direct",
                role_token(entry.role),
            )
        })
        .collect()
}

/// What survives a direct-grant refusal: the components that never asked for
/// one, which keep the CPU rasterize path that needs no device.
pub(super) fn admitted_without_direct(
    selected: &[sophia_config::ShellComponentConfig],
) -> Vec<sophia_config::ShellComponentConfig> {
    selected
        .iter()
        .filter(|entry| entry.gpu != sophia_config::ShellGpuMode::Direct)
        .cloned()
        .collect()
}

pub(super) fn prepare(
    config: &PersistentXtermSessionConfig,
    client_render_devices: Option<&render_devices::LiveRenderDeviceCoordinator>,
) -> Result<(Option<ShellComponentSession>, Option<std::path::PathBuf>), Box<dyn std::error::Error>>
{
    let mut component_directory = None;
    let shell_components = {
        let selected = &config
            .session_profile
            .candidate()
            .components
            .shell_components;
        if selected.is_empty() {
            None
        } else {
            if config.shell_process.is_some() {
                return Err("legacy and independent shell ownership are mutually exclusive".into());
            }
            // A direct grant needs the implementation, the operator's policy
            // and the launch resources to agree, and all three are decidable
            // here -- before any child runs. Refusing now replaces a component
            // that starts, negotiates and fails in service at every backoff
            // interval for ever with one record saying why it will not start.
            //
            // The device is session-global: there is one active render device,
            // so every direct component shares its fate. A refusal therefore
            // refuses all of them and none of the denied ones, which keep the
            // CPU rasterize path that needs no grant.
            let refusal = selected
                .iter()
                .any(|entry| entry.gpu == sophia_config::ShellGpuMode::Direct)
                .then(|| {
                    client_render_devices
                        .ok_or_else(|| {
                            "component GPU access requires native client rendering".to_string()
                        })
                        .and_then(render_devices::LiveRenderDeviceCoordinator::shell_gpu_device)
                })
                .transpose();
            let (device, admitted) = match refusal {
                Ok(device) => (device, std::borrow::Cow::Borrowed(selected.as_slice())),
                Err(error) => {
                    for record in refusal_records(selected, &error) {
                        crate::session_println!("{}", record);
                    }
                    (
                        None,
                        std::borrow::Cow::Owned(admitted_without_direct(selected)),
                    )
                }
            };
            // Every selected component was refused. The session comes up
            // without a shell component rather than not at all: one that
            // cannot get a device is not a reason to withhold the desktop.
            if admitted.is_empty() {
                return Ok((None, None));
            }
            let directory = std::env::temp_dir().join(format!(
                "sophia-live-components-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos()
            ));
            use std::os::unix::fs::DirBuilderExt as _;
            std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
            let prepared = metadata_shell::component_session::ShellComponentSession::prepare(
                &admitted,
                config.shell_panel_thickness.unwrap_or(0),
                device,
                &directory,
                if config.shell_content_enabled {
                    sophia_runtime::ShellContentAdmissionPolicy::Granted {
                        discrete_input: config.shell_content_input_enabled,
                    }
                } else {
                    sophia_runtime::ShellContentAdmissionPolicy::Denied
                },
            );
            let owner = match prepared {
                Ok(owner) => owner,
                Err(error) => {
                    let _ = std::fs::remove_dir(&directory);
                    return Err(error);
                }
            };
            component_directory = Some(directory);
            Some(owner)
        }
    };
    Ok((shell_components, component_directory))
}

/// Terminal cleanup only: never used to acknowledge a seat event. The bounded
/// reap waits here retain the owner on timeout/error for RetirementFailure.
pub(super) fn stop(
    components: Option<&mut ShellComponentSession>,
    mut runtime: Option<&mut LiveProductionVisualRuntime>,
    failures: &mut Vec<String>,
) {
    // This owner lives outside the loop even when completion returned early.
    // Stop IPC before native retirement and settle claims against the still-owned
    // runtime. A missing runtime deliberately retains the cleanup inventory.
    if let Some(components) = components {
        if let Err(error) = components.request_shutdown() {
            failures.push(format!("component shutdown failed: {error}"));
        }
        if let Err(error) = components.settle_revocations(runtime.as_deref_mut()) {
            failures.push(format!("component claims retained: {error}"));
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while components.retained_processes() != 0 && Instant::now() < deadline {
            match components.poll(64 * 1024) {
                Ok(visit) => {
                    for event in visit.processes.into_iter().flatten() {
                        match event {
                            crate::shell_component_processes::ComponentProcessEvent::Failed(_, error) =>
                                failures.push(format!("component process cleanup failed: {error}")),
                            crate::shell_component_processes::ComponentProcessEvent::ProcessRetired(_, Err(error)) =>
                                failures.push(format!("component endpoint cleanup failed: {error}")),
                            _ => {},
                        }
                    }
                    for (_, error) in visit.stop_errors.into_iter().flatten() {
                        failures.push(format!("component stop failed: {error}"));
                    }
                }
                Err(error) => {
                    failures.push(format!("component reap failed: {error}"));
                    break;
                }
            }
            if components.retained_processes() != 0 {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        // A reap may add its exact revoked identity after the initial pass.
        if let Err(error) = components.settle_revocations(runtime) {
            failures.push(format!("component final claims retained: {error}"));
        }
        if components.retained_processes() != 0 {
            failures.push("component process custody remains after shutdown deadline".into());
        }
    }
}

/// Called only after native/CPU/handoff disposition was attempted. A failed
/// native retirement leaves both component registry and directory owned.
pub(super) fn finish(
    shell_components: &mut Option<ShellComponentSession>,
    component_directory: &mut Option<std::path::PathBuf>,
    native_disposed: bool,
    session_succeeded: bool,
    failures: &mut Vec<String>,
) {
    if !native_disposed {
        if shell_components.is_some() || component_directory.is_some() {
            failures.push("component owner retained with unresolved native retirement".into());
        }
        return;
    }
    if let Some(components) = shell_components.as_mut() {
        match components.finish_after_backend_drop(()) {
            Ok((_, accounting)) if accounting.quiescent() => {
                crate::session_println!(
                    "sophia_shell_components_shutdown schema=1 status=quiescent"
                );
            }
            Ok(_) => failures.push("component content owners remain after native shutdown".into()),
            Err(()) => failures.push("component final owner transfer refused".into()),
        }
    }
    if failures.is_empty() && session_succeeded {
        drop(shell_components.take());
        if let Some(directory) = component_directory.take()
            && let Err(error) = std::fs::remove_dir(&directory)
        {
            *component_directory = Some(directory);
            failures.push(format!("component directory cleanup failed: {error}"));
        }
    }
}
