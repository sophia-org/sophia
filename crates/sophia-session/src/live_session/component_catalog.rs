//! Catalog worker custody outside the owner loop and independent peer epochs.
use super::*;
use crate::application_catalog::{
    ApplicationCatalog, ApplicationCatalogEnvironment, ApplicationLaunchCommand,
    CatalogProcessEnvironment, NativeCatalogService, NativeCatalogServiceEvent,
    RegisteredCatalogApplication,
};

#[derive(Default)]
pub(super) struct ComponentCatalog {
    service: Option<NativeCatalogService>,
    snapshot: Option<(u64, ApplicationCatalog)>,
    refresh_pending: bool,
    stopped: bool,
    started: Option<Instant>,
}
impl ComponentCatalog {
    /// Initial scan only. Native peer publication and execution are joined by
    /// their connected-owner service; scanning itself grants neither authority.
    pub(super) fn visit_scan(
        &mut self,
        config: &PersistentXtermSessionConfig,
        launches: &mut SessionLaunchQueue,
        xauthority: &std::path::Path,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.stopped
            || !config
                .session_profile
                .candidate()
                .components
                .shell_components
                .iter()
                .any(|entry| entry.role == sophia_config::ShellComponentRole::ApplicationLauncher)
        {
            return Ok(false);
        }
        if self.snapshot.is_some() {
            return Ok(true);
        }
        if self.service.is_none() {
            let catalog = config
                .application_catalog
                .clone()
                .ok_or("native launcher needs a catalog")?;
            let registered = config
                .applications
                .applications
                .values()
                .map(|app| RegisteredCatalogApplication {
                    name: app.id.clone(),
                    command: ApplicationLaunchCommand {
                        executable: app.executable.clone(),
                        arguments: app.arguments.clone(),
                        working_directory: None,
                    },
                })
                .collect();
            // Store the actual worker before submitting anything fallible.
            self.service = Some(NativeCatalogService::start(
                catalog,
                registered,
                environment(),
            )?);
            self.started = Some(Instant::now());
        }
        let service = self.service.as_mut().ok_or("catalog worker absent")?;
        if self.snapshot.is_none() && !self.refresh_pending {
            if !service.refresh(1) {
                return Err("initial catalog refresh refused".into());
            }
            self.refresh_pending = true;
        }
        let elapsed = self.started.ok_or("catalog time origin absent")?.elapsed();
        let now = u64::try_from(elapsed.as_millis()).map_err(|_| "catalog clock overflow")?;
        // No connected grant is supplied in this initial-scan phase. This cannot
        // execute even if a stale native dispatch was left in the shared queue.
        let event = service.service(
            None,
            launches,
            CatalogProcessEnvironment {
                display: &config.display,
                xauthority,
                control_socket: config.control_socket.as_deref(),
            },
            now,
        );
        match event {
            NativeCatalogServiceEvent::Catalog(1, result) if self.refresh_pending => {
                self.snapshot = Some((1, result.map_err(|e| format!("native catalog: {e}"))?));
                self.refresh_pending = false;
                crate::session_println!(
                    "sophia_shell_component_catalog schema=1 status=built generation=1 entries={}",
                    self.snapshot
                        .as_ref()
                        .map_or(0, |(_, catalog)| catalog.entries.len())
                );
            }
            NativeCatalogServiceEvent::Idle | NativeCatalogServiceEvent::Rejected => {}
            NativeCatalogServiceEvent::Started(child) => {
                // service(None) forbids execution. Keep the real unexpected
                // child in an error carrier instead of silently dropping it.
                return Err(Box::new(native_owner_retirement::RetirementFailure::new(
                    "catalog scan unexpectedly executed a child".into(),
                    NativeRetirement::<LiveProductionNativeScanout>::default(),
                    child,
                )));
            }
            _ => return Err("unexpected native catalog scan result".into()),
        }
        if self.refresh_pending && elapsed >= Duration::from_secs(5) {
            return Err("initial native catalog scan timed out".into());
        }
        Ok(self.snapshot.is_some())
    }

    /// Terminal cleanup, never a seat acknowledgement. On timeout or failure
    /// this actual service remains in the outer Session error carrier.
    pub(super) fn stop(&mut self, launches: &mut SessionLaunchQueue, failures: &mut Vec<String>) {
        self.stopped = true;
        let Some(service) = self.service.as_mut() else {
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match service.drain_shutdown(launches) {
                Ok(true) => {
                    self.service = None;
                    self.refresh_pending = false;
                    return;
                }
                Ok(false) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(false) => {
                    failures.push("native catalog worker retained after shutdown deadline".into());
                    return;
                }
                Err(error) => {
                    failures.push(format!("native catalog shutdown unresolved: {error}"));
                    return;
                }
            }
        }
    }
}

fn environment() -> ApplicationCatalogEnvironment {
    ApplicationCatalogEnvironment {
        search_path: std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|p| p.is_absolute())
            .collect(),
        locale: std::env::var("LC_ALL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_else(|| "C".into()),
        current_desktop: std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_else(|_| "Sophia".into())
            .split(':')
            .map(str::to_owned)
            .collect(),
    }
}
