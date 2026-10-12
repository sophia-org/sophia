//! Application custody spans preparation, dispatch installation and process exit.
//! No raw namespace Child escapes this owner; dropping it fences its grants.
use super::*;
use sophia_portal::recipient_object::PortalObjectRecipient;
use sophia_protocol::namespace_view::ServiceReference;
use sophia_runtime::inspection::InspectionAcceptor;
use std::process::{Child, ExitStatus};

#[derive(Default)]
pub struct NamespaceApplicationServices {
    pub inspection: Option<(ServiceReference, InspectionAcceptor)>,
    pub portal: Option<(ServiceReference, Arc<PortalObjectRecipient>)>,
}

pub enum ApplicationProcessEvent {
    /// Published once after dispatch installation, or after an X application
    /// discards its optional protocol capability. This is process readiness,
    /// not a claim that the application has authenticated or opened a window.
    Started {
        process: u32,
        peer: u32,
    },
    Exited(ExitStatus),
    Failed(String),
}

enum State {
    Host(Child),
    Preparing(Box<PendingNamespaceClient>),
    Running(Box<RunningNamespaceClient>),
    Finished,
}

pub struct ApplicationProcess {
    identity: Option<crate::launch_origin::ProcessIdentity>,
    state: State,
    services: NamespaceApplicationServices,
    terminal: bool,
}
impl From<Child> for ApplicationProcess {
    fn from(child: Child) -> Self {
        Self {
            identity: crate::launch_origin::read_process(child.id()).map(|p| p.identity),
            state: State::Host(child),
            services: Default::default(),
            terminal: false,
        }
    }
}
impl ApplicationProcess {
    pub fn namespace(
        pending: PendingNamespaceClient,
        services: NamespaceApplicationServices,
    ) -> Self {
        Self {
            identity: None,
            state: State::Preparing(Box::new(pending)),
            services,
            terminal: false,
        }
    }
    pub fn process_id(&self) -> Option<u32> {
        match &self.state {
            State::Host(child) => Some(child.id()),
            State::Running(child) => child.process_id(),
            _ => None,
        }
    }
    /// Captured while the fork child was held before exec. This monitor
    /// identity attributes descendants; it is not the authenticated peer.
    pub fn launch_identity(&self) -> Option<crate::launch_origin::ProcessIdentity> {
        self.identity
    }
    pub(crate) fn host_mut(&mut self) -> Option<&mut Child> {
        match &mut self.state {
            State::Host(child) => Some(child),
            _ => None,
        }
    }
    /// Revocation and retirement for a namespace child never reap on the
    /// Session owner thread. Ordinary host process-group cleanup stays with
    /// the existing Session process supervisor.
    pub fn cancel_namespace(&mut self) -> bool {
        if matches!(self.state, State::Host(_)) {
            return false;
        }
        self.state = State::Finished;
        self.services = Default::default();
        self.terminal = true;
        true
    }

    /// The Session polls its shared authentication coordinator before visiting
    /// applications. A failed app is local: it does not terminate Session or
    /// consume another catalog admission. Terminal observations occur once.
    pub fn poll(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Option<ApplicationProcessEvent> {
        if self.terminal {
            return None;
        }
        match self.advance(launches, wake, now) {
            Ok(event) => {
                if matches!(event, Some(ApplicationProcessEvent::Exited(_))) {
                    self.terminal = true;
                    self.services = Default::default();
                }
                event
            }
            Err(reason) => {
                // Keep a host child for its existing process-group cleanup.
                if !matches!(self.state, State::Host(_)) {
                    self.state = State::Finished;
                }
                self.services = Default::default();
                self.terminal = true;
                Some(ApplicationProcessEvent::Failed(reason))
            }
        }
    }

    fn advance(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Result<Option<ApplicationProcessEvent>, String> {
        match &mut self.state {
            State::Host(child) => child
                .try_wait()
                .map(|status| status.map(ApplicationProcessEvent::Exited))
                .map_err(|error| error.to_string()),
            State::Preparing(pending) => {
                let observed = pending.poll(launches, wake.clone(), now);
                self.identity = pending.process_identity;
                if let Some(status) = pending.take_exit_status() {
                    self.state = State::Finished;
                    return Ok(Some(ApplicationProcessEvent::Exited(status)));
                }
                let Some(mut running) = observed? else {
                    return Ok(None);
                };
                if !running.protocol_available() {
                    self.services = Default::default();
                }
                if let Some((reference, owner)) = self.services.inspection.take() {
                    running.serve_inspection(&reference, owner, wake.clone())?;
                }
                if let Some((reference, owner)) = self.services.portal.take() {
                    running.serve_portal(&reference, owner, wake)?;
                }
                if !running.take_endpoints().is_empty() {
                    return Err("application has an undispatched namespace endpoint".into());
                }
                let process = running
                    .process_id()
                    .ok_or("application process identity absent")?;
                let peer = running
                    .peer_id()
                    .ok_or("application peer identity absent")?;
                self.state = State::Running(Box::new(running));
                Ok(Some(ApplicationProcessEvent::Started { process, peer }))
            }
            State::Running(running) => {
                if let Some(status) = running.poll(launches)? {
                    self.state = State::Finished;
                    Ok(Some(ApplicationProcessEvent::Exited(status)))
                } else {
                    Ok(None)
                }
            }
            State::Finished => Ok(None),
        }
    }
}
