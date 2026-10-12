use super::*;

pub(super) struct SessionProcessGuard {
    pub(super) child: Option<ManagedSessionChild>,
    pub(super) diagnostic: Option<crate::diagnostics::application::LaunchTicket>,
    pub(super) secondary_children: Vec<ManagedSessionChild>,
    pub(super) socket_path: Option<std::path::PathBuf>,
    pub(super) grouped: bool,
}

pub(super) struct ManagedSessionChild {
    pub(super) id: Option<String>,
    pub(super) launch_transaction: Option<TransactionId>,
    pub(super) catalog_launch: bool,
    pub(super) native_catalog: Option<std::sync::Arc<crate::session_actions::NativeCatalogLaunch>>,
    pub(super) child: crate::namespace_launch::ApplicationProcess,
    pub(super) diagnostic: Option<crate::diagnostics::application::LaunchTicket>,
    pub(super) process_identity: Option<crate::launch_origin::ProcessIdentity>,
    buffered_event: Option<crate::namespace_launch::ApplicationProcessEvent>,
    pub(super) shell_launch: Option<(TransactionId, sophia_protocol::ShellLauncherActivation)>,
    pub(super) developer_launch: Option<sophia_runtime::developer_launch::DeveloperLaunchTicket>,
}

impl ManagedSessionChild {
    /// A queued launch is not a Started outcome. Preserve the activation across
    /// preparation and consume it once, including failure or exit before readiness.
    pub(super) fn shell_outcome(
        &mut self,
        event: Option<&crate::namespace_launch::ApplicationProcessEvent>,
    ) -> Option<(
        TransactionId,
        sophia_protocol::ShellLauncherActivation,
        sophia_protocol::ShellLaunchStatus,
    )> {
        use crate::namespace_launch::ApplicationProcessEvent;
        let status = match event? {
            ApplicationProcessEvent::Started { .. } => sophia_protocol::ShellLaunchStatus::Started,
            ApplicationProcessEvent::Failed(_) | ApplicationProcessEvent::Exited(_) => {
                sophia_protocol::ShellLaunchStatus::Failed
            }
        };
        let (transaction, activation) = self.shell_launch.take()?;
        Some((transaction, activation, status))
    }

    pub(super) fn matches_admission(&self, launches: &SessionLaunchQueue) -> bool {
        self.launch_transaction.is_some_and(|transaction| {
            launches.matches_child_launch(
                transaction,
                self.catalog_launch,
                self.native_catalog.as_deref(),
            )
        })
    }

    pub(super) fn new(
        id: Option<String>,
        child: impl Into<crate::namespace_launch::ApplicationProcess>,
    ) -> Self {
        let child = child.into();
        let pid = child.process_id();
        Self {
            buffered_event: None,
            shell_launch: None,
            developer_launch: None,
            id,
            launch_transaction: None,
            catalog_launch: false,
            native_catalog: None,
            process_identity: pid
                .and_then(crate::launch_origin::read_process)
                .map(|p| p.identity),
            diagnostic: pid.and_then(crate::diagnostics::application::registration),
            child,
        }
    }

    pub(super) fn for_launch(
        id: Option<String>,
        transaction: TransactionId,
        child: impl Into<crate::namespace_launch::ApplicationProcess>,
    ) -> Self {
        let mut child = Self::new(id, child);
        child.launch_transaction = Some(transaction);
        child
    }

    pub(super) fn poll(
        &mut self,
        launches: &mut crate::launch_authentication::LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Option<crate::namespace_launch::ApplicationProcessEvent> {
        if let Some(event) = self.buffered_event.take() {
            return Some(event);
        }
        let event = self.child.poll(launches, wake, now);
        if let Some(event) = &event
            && let Some(ticket) = self.developer_launch.take()
        {
            use crate::namespace_launch::ApplicationProcessEvent;
            use sophia_protocol::developer_launch::DeveloperLaunchStatus as Status;
            match event {
                ApplicationProcessEvent::Started { process, .. } => {
                    ticket.finish(Status::Started, Some(*process));
                }
                ApplicationProcessEvent::Exited(_) | ApplicationProcessEvent::Failed(_) => {
                    ticket.finish(Status::Failed, None);
                }
            }
        }
        self.process_identity = self.child.launch_identity().or(self.process_identity);
        if let Some(crate::namespace_launch::ApplicationProcessEvent::Started { process, .. }) =
            &event
        {
            self.diagnostic = crate::diagnostics::application::registration(*process);
        }
        event
    }

    /// Startup's first-frame wait must advance an asynchronous launch without
    /// consuming its first lifecycle event before the owner loop observes it.
    pub(super) fn pump(
        &mut self,
        launches: &mut crate::launch_authentication::LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) {
        if self.buffered_event.is_none() {
            self.buffered_event = self.poll(launches, wake, now);
        }
    }

    pub(super) fn terminate(&mut self, grouped: bool) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ticket) = self.developer_launch.take() {
            ticket.finish(
                sophia_protocol::developer_launch::DeveloperLaunchStatus::Unavailable,
                None,
            );
        }
        if let Some(child) = self.child.host_mut() {
            terminate_session_child(child, grouped, self.diagnostic)
        } else {
            self.child.cancel_namespace();
            Ok(())
        }
    }
}

impl<C: Into<crate::namespace_launch::ApplicationProcess>>
    From<crate::application_catalog::NativeCatalogChild<C>> for ManagedSessionChild
{
    fn from(value: crate::application_catalog::NativeCatalogChild<C>) -> Self {
        let mut managed = Self::for_launch(None, value.launch.transaction, value.child);
        managed.catalog_launch = true;
        managed.native_catalog = Some(value.launch);
        managed
    }
}

/// Shared process construction for catalog launches. The caller must consume
/// exact admission before this effect and retain the returned child immediately.
/// `identity` is the catalog identity the command was verified from.
pub(super) fn spawn_catalog_child(
    command: crate::application_catalog::ApplicationLaunchCommand,
    identity: &str,
    config: &PersistentXtermSessionConfig,
    xauthority: &std::path::Path,
    transaction: TransactionId,
    applications: &mut ApplicationLaunchTurn<'_>,
) -> std::io::Result<ManagedSessionChild> {
    let child = applications.spawn(
        config,
        xauthority,
        identity,
        &command,
        crate::diagnostics::application::LaunchContext {
            source: crate::diagnostics::application::LaunchSource::Catalog,
            transaction: Some(transaction.raw()),
        },
    )?;
    let mut managed = ManagedSessionChild::for_launch(None, transaction, child);
    managed.catalog_launch = true;
    Ok(managed)
}

pub(super) const fn managed_child_exit_is_nonfatal(
    normal_session: bool,
    launch_transaction: Option<TransactionId>,
) -> bool {
    normal_session || launch_transaction.is_some()
}

pub(super) fn terminate_session_child(
    child: &mut Child,
    grouped: bool,
    diagnostic: Option<crate::diagnostics::application::LaunchTicket>,
) -> Result<(), Box<dyn std::error::Error>> {
    let status = child.try_wait()?;
    if let Some(status) = status {
        crate::diagnostics::application::exited(diagnostic, status);
    }
    let leader_exited = status.is_some();
    if grouped {
        let pid = rustix::process::Pid::from_raw(child.id() as i32)
            .ok_or("session child PID is invalid")?;
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::TERM);
        if leader_exited {
            // A launcher can exit before helpers in its process group. The
            // group remains addressable by its original PGID even after the
            // leader is reaped, so explicitly drain those helpers as well.
            std::thread::sleep(Duration::from_millis(25));
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait()? {
                crate::diagnostics::application::exited(diagnostic, status);
                // The process-group leader can exit before terminal helpers
                // that inherited the X connection. Drain the whole group or
                // frontend shutdown can wait forever on an orphaned client.
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    } else {
        if leader_exited {
            return Ok(());
        }
        child.kill()?;
    }
    let status = child.wait()?;
    crate::diagnostics::application::exited(diagnostic, status);
    Ok(())
}

impl SessionProcessGuard {
    pub(super) fn new(
        child: Option<crate::namespace_launch::ApplicationProcess>,
        secondary_children: Vec<ManagedSessionChild>,
        socket_path: std::path::PathBuf,
        grouped: bool,
    ) -> Self {
        Self {
            diagnostic: child
                .as_ref()
                .and_then(|child| child.process_id())
                .and_then(crate::diagnostics::application::registration),
            child: child.map(|child| ManagedSessionChild::new(None, child)),
            secondary_children,
            socket_path: Some(socket_path),
            grouped,
        }
    }

    pub(super) fn children_mut(
        &mut self,
    ) -> (
        Option<&mut ManagedSessionChild>,
        &mut Vec<ManagedSessionChild>,
    ) {
        (self.child.as_mut(), &mut self.secondary_children)
    }

    pub(super) fn add_secondary_child(
        &mut self,
        id: Option<String>,
        child: impl Into<crate::namespace_launch::ApplicationProcess>,
    ) {
        self.secondary_children
            .push(ManagedSessionChild::new(id, child));
    }

    pub(super) fn terminate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(mut child) = self.child.take() {
            child.terminate(self.grouped)?;
        }
        for mut child in self.secondary_children.drain(..) {
            child.terminate(self.grouped)?;
        }
        if let Some(socket_path) = self.socket_path.as_ref() {
            match std::fs::remove_file(socket_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

impl Drop for SessionProcessGuard {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}
