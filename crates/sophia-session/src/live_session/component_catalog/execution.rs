//! Connected worker result ownership and immediate adoption by Session.
use super::*;
use sophia_runtime::ShellTransportConnection;

impl ComponentCatalog {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::live_session) fn service_execution(
        &mut self,
        connection: Option<&ShellTransportConnection<'_>>,
        config: &PersistentXtermSessionConfig,
        xauthority: &std::path::Path,
        launches: &mut SessionLaunchQueue,
        children: &mut Vec<ManagedSessionChild>,
        admission_started: &mut Option<Instant>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let grant = connection.and_then(|c| c.content_grant());
        if self.execution_grant != grant {
            if let Some(old) = self.execution_grant {
                launches.revoke_native_catalog_grant(old);
            }
            self.execution_grant = grant;
        }
        // Scan owns worker results until its immutable snapshot is ready.
        if !self.ready() {
            return Ok(());
        }
        let now = self.action_now_msec()?;
        children.try_reserve(1)?;
        let service = self
            .service
            .as_mut()
            .ok_or("native catalog worker absent")?;
        let event = service.service(
            connection,
            launches,
            CatalogProcessEnvironment {
                display: &config.display,
                xauthority,
                control_socket: config.control_socket.as_deref(),
            },
            now,
        );
        match event {
            NativeCatalogServiceEvent::Started(child) => {
                // No fallible effect before the real child and exact origin
                // enter the existing supervisor. Capacity was reserved above.
                let transaction = child.launch.transaction;
                children.push(ManagedSessionChild::from(child));
                *admission_started = Some(Instant::now());
                crate::session_println!(
                    "sophia_native_launcher schema=1 status=process_started transaction={}",
                    transaction.raw()
                );
            }
            NativeCatalogServiceEvent::Idle => {}
            NativeCatalogServiceEvent::Rejected => {
                crate::session_println!("sophia_native_launcher schema=1 status=execution_rejected")
            }
            NativeCatalogServiceEvent::SpawnFailed(error) => crate::session_eprintln!(
                "sophia_native_launcher schema=1 status=spawn_failed reason={error}"
            ),
            _ => return Err("unexpected connected catalog worker result".into()),
        }
        Ok(())
    }
}
