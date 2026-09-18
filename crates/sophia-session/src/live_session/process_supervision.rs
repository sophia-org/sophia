use super::*;

pub(super) struct SessionProcessGuard {
    pub(super) child: Option<Child>,
    pub(super) secondary_children: Vec<ManagedSessionChild>,
    pub(super) socket_path: Option<std::path::PathBuf>,
    pub(super) grouped: bool,
}

pub(super) struct ManagedSessionChild {
    pub(super) id: Option<String>,
    pub(super) launch_transaction: Option<TransactionId>,
    pub(super) catalog_launch: bool,
    pub(super) native_catalog: Option<std::sync::Arc<crate::session_actions::NativeCatalogLaunch>>,
    pub(super) child: Child,
    pub(super) process_identity: Option<crate::launch_origin::ProcessIdentity>,
}

impl ManagedSessionChild {
    pub(super) fn matches_admission(&self, launches: &SessionLaunchQueue) -> bool {
        self.launch_transaction.is_some_and(|transaction| {
            launches.matches_child_launch(
                transaction,
                self.catalog_launch,
                self.native_catalog.as_deref(),
            )
        })
    }

    pub(super) fn new(id: Option<String>, child: Child) -> Self {
        Self {
            id,
            launch_transaction: None,
            catalog_launch: false,
            native_catalog: None,
            process_identity: crate::launch_origin::read_process(child.id()).map(|p| p.identity),
            child,
        }
    }

    pub(super) fn for_launch(id: Option<String>, transaction: TransactionId, child: Child) -> Self {
        Self {
            id,
            launch_transaction: Some(transaction),
            catalog_launch: false,
            native_catalog: None,
            process_identity: crate::launch_origin::read_process(child.id()).map(|p| p.identity),
            child,
        }
    }
}

impl From<crate::application_catalog::NativeCatalogChild> for ManagedSessionChild {
    fn from(value: crate::application_catalog::NativeCatalogChild) -> Self {
        let mut managed = Self::for_launch(None, value.launch.transaction, value.child);
        managed.catalog_launch = true;
        managed.native_catalog = Some(value.launch);
        managed
    }
}

/// Shared process construction for catalog launches. The caller must consume
/// exact admission before this effect and retain the returned child immediately.
pub(super) fn spawn_catalog_child(
    command: crate::application_catalog::ApplicationLaunchCommand,
    config: &PersistentXtermSessionConfig,
    xauthority: &std::path::Path,
    transaction: TransactionId,
) -> std::io::Result<ManagedSessionChild> {
    let child = crate::application_catalog::spawn_catalog_process(
        &command,
        crate::application_catalog::CatalogProcessEnvironment {
            display: &config.display,
            xauthority,
            control_socket: config.control_socket.as_deref(),
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
) -> Result<(), Box<dyn std::error::Error>> {
    let leader_exited = child.try_wait()?.is_some();
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
            if child.try_wait()?.is_some() {
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
    child.wait()?;
    Ok(())
}

impl SessionProcessGuard {
    pub(super) fn new(
        child: Option<Child>,
        secondary_children: Vec<ManagedSessionChild>,
        socket_path: std::path::PathBuf,
        grouped: bool,
    ) -> Self {
        Self {
            child,
            secondary_children,
            socket_path: Some(socket_path),
            grouped,
        }
    }

    pub(super) fn children_mut(&mut self) -> (Option<&mut Child>, &mut Vec<ManagedSessionChild>) {
        (self.child.as_mut(), &mut self.secondary_children)
    }

    pub(super) fn add_secondary_child(&mut self, id: Option<String>, child: Child) {
        self.secondary_children
            .push(ManagedSessionChild::new(id, child));
    }

    pub(super) fn terminate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(mut child) = self.child.take() {
            terminate_session_child(&mut child, self.grouped)?;
        }
        for mut child in self.secondary_children.drain(..) {
            terminate_session_child(&mut child.child, self.grouped)?;
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
