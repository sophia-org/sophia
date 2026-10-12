//! Host-administrative 9P requests enter the ordinary recipe process owner.
use super::*;
use sophia_protocol::developer_launch::DeveloperLaunchStatus as Status;
use sophia_runtime::developer_launch::{DeveloperLaunchService, DeveloperLaunchTicket};

#[derive(Default)]
pub(super) struct LiveDeveloperLaunches {
    service: Option<DeveloperLaunchService>,
    next: Option<DeveloperLaunchTicket>,
}
impl LiveDeveloperLaunches {
    pub(super) fn socket_path(&self) -> Option<&std::path::Path> {
        self.service
            .as_ref()
            .map(DeveloperLaunchService::socket_path)
    }
    pub(super) fn start(
        config: &PersistentXtermSessionConfig,
        wake: sophia_wake::Notifier,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if config.control_access != sophia_config::DesktopControlAccess::HostAdmin
            || !config
                .core_config_state
                .active()
                .session
                .namespace_policy
                .recipes
                .values()
                .any(|r| r.developer_launch)
        {
            return Ok(Self::default());
        }
        let runtime =
            std::env::var_os("XDG_RUNTIME_DIR").ok_or("developer launch runtime absent")?;
        let service = DeveloperLaunchService::bind(std::path::Path::new(&runtime))?;
        service.set_owner_wake(wake);
        let owner = Self {
            service: Some(service),
            next: None,
        };
        crate::session_println!(
            "sophia_developer_launch schema=1 status=enabled admission=host-admin socket={}",
            owner.socket_path().expect("enabled service").display()
        );
        Ok(owner)
    }
    pub(super) fn owner_work_pending(&self) -> bool {
        self.next.is_some()
    }
    pub(super) fn service(
        &mut self,
        config: &PersistentXtermSessionConfig,
        applications: &mut ApplicationLaunchTurn<'_>,
        children: &mut Vec<ManagedSessionChild>,
        stopping: bool,
    ) {
        let Some(service) = &self.service else {
            return;
        };
        if stopping || !service.is_running() {
            if let Some(ticket) = self.next.take() {
                ticket.finish(Status::Unavailable, None);
            }
            self.service.take();
            return;
        }
        let Some(ticket) = self.next.take().or_else(|| service.try_request()) else {
            return;
        };
        if ticket.cancelled() {
            return;
        }
        if children.len() >= crate::session_actions::SESSION_ACTION_APPLICATION_CAPACITY
            || children.try_reserve(1).is_err()
        {
            ticket.finish(Status::Unavailable, None);
            return;
        }
        if !ticket.claim() {
            if !ticket.cancelled() {
                self.next = Some(ticket);
            }
            return;
        }
        match applications.spawn_developer(config, &ticket.request) {
            Ok(child) => {
                let mut managed = ManagedSessionChild::new(None, child);
                managed.developer_launch = Some(ticket);
                children.push(managed);
            }
            Err(error) => {
                ticket.finish(
                    if error.kind() == std::io::ErrorKind::PermissionDenied {
                        Status::Denied
                    } else {
                        Status::Failed
                    },
                    None,
                );
                crate::session_eprintln!(
                    "sophia_developer_launch schema=1 status=refused request={} reason={error}",
                    ticket.request.request
                );
            }
        }
    }
}
