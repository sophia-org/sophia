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
        let expected = self.execution_owner(launches);
        if connection.and_then(|c| c.content_grant())
            != expected.filter(|g| self.connected_grants.contains(&Some(*g)))
        {
            return Err("catalog worker visit borrowed the wrong connected owner".into());
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
                let grant = child.launch.cause.grant();
                let (cause, output, event_id) = match &child.launch.cause {
                    crate::session_actions::CatalogLaunchCause::Transient(value) => (
                        "transient",
                        value.event.binding.output.id,
                        value.event.event_id,
                    ),
                    crate::session_actions::CatalogLaunchCause::Persistent(value) => {
                        ("persistent", value.action.output.id, value.action.event_id)
                    }
                };
                children.push(ManagedSessionChild::from(child));
                *admission_started = Some(Instant::now());
                crate::session_println!(
                    "sophia_catalog_launch schema=1 status=process_started transaction={} cause={} connection_epoch={} content_grant_epoch={} output={} event_id={}",
                    transaction.raw(),
                    cause,
                    grant.connection_epoch,
                    grant.content_grant_epoch,
                    output,
                    event_id
                );
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
