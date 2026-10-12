//! Nonblocking launch transaction shared by role startup and replacement.
use super::spawn::{ProtectedSpawn, SpawnLease};
use super::{LaunchCoordinator, LaunchPolicy, Phase, Registration, files::EndpointBudget};
use sophia_runtime::authenticated_files::FileAuthenticationFactory;
use sophia_runtime::{ProcessSupervisor, SupervisorCommand};
use std::time::{Duration, Instant};

pub(crate) struct Recipe {
    pub policy: LaunchPolicy,
    pub uname: Vec<u8>,
    pub aname: Vec<u8>,
}

/// Keep the lease until the supervisor has dropped or reaped its child.
pub(crate) struct Started {
    pub supervisor: ProcessSupervisor,
    pub registration: Registration,
    pub factory: Box<dyn FileAuthenticationFactory>,
    pub lease: SpawnLease,
    pub diagnostic: Option<crate::diagnostics::application::PreparedLaunch>,
}

pub(crate) struct Startup {
    supervisor: Option<ProcessSupervisor>,
    spawning: Option<ProtectedSpawn>,
    lease: Option<SpawnLease>,
    registration: Option<Registration>,
    endpoint: EndpointBudget,
    grant: Option<sophia_runtime::EndpointGrantLease>,
    host_namespace_client: bool,
    recipe: Recipe,
    not_before: Instant,
    deadline: Instant,
    failure: Option<String>,
    finished: bool,
    diagnostic: Option<crate::diagnostics::application::PreparedLaunch>,
}

impl Startup {
    /// Background retirement only: retain custody and the capacity slot until
    /// an outstanding spawn returns and its child has been reaped.
    pub(crate) fn retire(mut self) {
        if let Some(spawning) = self.spawning.take() {
            spawning.cancel();
            loop {
                match spawning.poll() {
                    Ok(Some(result)) => {
                        super::spawn::retire_process(result.supervisor, result.diagnostic);
                        drop(result.lease);
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                    Err(_) => break,
                }
            }
        }
        if let Some(supervisor) = self.supervisor.take() {
            super::spawn::retire_process(supervisor, self.diagnostic.take());
        }
        self.lease.take();
    }

    pub(crate) fn with_diagnostic(
        mut self,
        diagnostic: Option<crate::diagnostics::application::PreparedLaunch>,
    ) -> Self {
        self.diagnostic = diagnostic;
        self
    }

    pub fn new(
        supervisor: ProcessSupervisor,
        command: SupervisorCommand,
        endpoint: EndpointBudget,
        recipe: Recipe,
        now: Instant,
    ) -> Result<Self, String> {
        let SupervisorCommand::StartProcess { delay, process } = command else {
            return Err("authenticated role launch was not authorized to start".into());
        };
        if process != supervisor.process() {
            return Err("authenticated role launch process mismatch".into());
        }
        let not_before = now
            .checked_add(delay)
            .ok_or("role restart delay overflow")?;
        Ok(Self {
            supervisor: Some(supervisor),
            spawning: None,
            lease: None,
            registration: None,
            endpoint,
            grant: None,
            host_namespace_client: false,
            recipe,
            not_before,
            deadline: not_before + Duration::from_secs(10),
            failure: None,
            finished: false,
            diagnostic: None,
        })
    }

    /// A namespace launch prepares its view after reserving a generation.
    /// Continue through the same spawn/activation transaction as fixed roles,
    /// but bind the primary endpoint to its exact captured grant.
    pub(crate) fn registered_endpoint(
        supervisor: ProcessSupervisor,
        endpoint: EndpointBudget,
        recipe: Recipe,
        registration: Registration,
        grant: sophia_runtime::EndpointGrantLease,
        now: Instant,
    ) -> Result<Self, String> {
        if grant.endpoint().scope.principal != recipe.policy.principal
            || grant.endpoint().scope.namespace.raw() != recipe.policy.namespace
            || !grant.permits(
                grant.endpoint().scope.namespace,
                sophia_protocol::ClientAuthenticationMethod::FactotumLaunch {
                    principal: recipe.policy.principal,
                    launch_generation: registration.launch_generation(),
                },
                recipe.policy.audience,
            )
        {
            return Err("namespace launch grant differs from reservation".into());
        }
        let process = supervisor.process();
        let host_namespace_client = supervisor.launch_spec().protection_domain.is_none();
        let mut startup = Self::new(
            supervisor,
            SupervisorCommand::StartProcess {
                process,
                delay: Duration::ZERO,
            },
            endpoint,
            recipe,
            now,
        )?;
        startup.registration = Some(registration);
        startup.grant = Some(grant);
        startup.host_namespace_client = host_namespace_client;
        Ok(startup)
    }

    /// Revoke before cancelling a spawn. Poll until cancellation returns an
    /// error before starting a replacement: an outstanding worker still counts.
    pub fn cancel(&mut self, launches: &mut LaunchCoordinator, now: Instant, reason: String) {
        self.failure.get_or_insert(reason);
        if let Some(registration) = self.registration.take() {
            let _ = launches.revoke(registration, now);
        }
        if let Some(spawning) = self.spawning.as_ref() {
            spawning.cancel();
        }
    }

    /// The caller polls the shared coordinator once per turn. This transaction
    /// never waits for Factotum, protected peer discovery, or restart backoff.
    pub fn poll(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Result<Option<Started>, String> {
        if self.finished {
            return Err("authenticated role launch already consumed".into());
        }
        let result = self.advance(launches, wake, now);
        if let Err(reason) = result {
            self.cancel(launches, now, reason);
            // Cancellation must reclaim the actual spawn result, not leave an
            // unbounded collection of detached launch workers behind retries.
            if self.spawning.is_some() {
                return Ok(None);
            }
            if let Some(supervisor) = self.supervisor.as_mut() {
                supervisor
                    .request_termination()
                    .map_err(|e| e.to_string())?;
                if !supervisor.poll_termination().map_err(|e| e.to_string())? {
                    return Ok(None);
                }
            }
            if let Some(supervisor) = self.supervisor.take()
                && let (Some(diagnostic), Some(status)) =
                    (&self.diagnostic, supervisor.exit_status())
            {
                diagnostic.exited(status);
            }
            self.diagnostic.take();
            self.lease.take();
            self.finished = true;
            return Err(self.failure.take().expect("failed launch"));
        }
        result
    }

    fn advance(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Result<Option<Started>, String> {
        if let Some(spawning) = self.spawning.as_ref() {
            match spawning.poll() {
                Ok(Some(result)) => {
                    self.spawning = None;
                    self.supervisor = Some(result.supervisor);
                    self.lease = Some(result.lease);
                    self.diagnostic = result.diagnostic;
                    result.result?;
                }
                Ok(None) => {}
                Err(error) => {
                    self.spawning = None;
                    return Err(error);
                }
            }
        }
        if let Some(reason) = self.failure.as_ref() {
            return Err(reason.clone());
        }
        if now >= self.deadline {
            return Err("authenticated role launch deadline".into());
        }
        if now < self.not_before {
            return Ok(None);
        }
        if self.registration.is_none() {
            if !launches.ready() {
                return Ok(None);
            }
            self.registration = Some(
                launches
                    .reserve(self.recipe.policy, now)
                    .map_err(|e| format!("role registration: {e:?}"))?,
            );
        }
        let registration = self.registration.expect("reserved role");
        match launches.phase(registration) {
            Some(Phase::Registering) => Ok(None),
            Some(Phase::Ready) => {
                let channel = launches
                    .take_channel(registration, now)
                    .map_err(|e| format!("role channel: {e:?}"))?;
                self.spawning = Some(ProtectedSpawn::start_using(
                    self.supervisor.take().ok_or("missing role supervisor")?,
                    channel,
                    wake,
                    self.host_namespace_client,
                    self.diagnostic.take(),
                )?);
                Ok(None)
            }
            Some(Phase::Spawning | Phase::Activating) if self.spawning.is_none() => {
                let supervisor = self
                    .supervisor
                    .as_mut()
                    .ok_or("missing role spawn result")?;
                if !launches
                    .activate(registration, supervisor, now)
                    .map_err(|e| format!("role activation: {e:?}"))?
                {
                    return Ok(None);
                }
                let custody = launches
                    .worker_authority(registration, supervisor, now)
                    .map_err(|e| format!("role custody: {e:?}"))?;
                let factory = match &self.grant {
                    Some(grant) => self.endpoint.bind_grant(
                        custody,
                        grant.clone(),
                        grant.endpoint(),
                        self.recipe.uname.clone(),
                        self.recipe.aname.clone(),
                    ),
                    None => self.endpoint.bind(
                        custody,
                        self.recipe.uname.clone(),
                        self.recipe.aname.clone(),
                    ),
                }
                .map_err(|e| format!("role endpoint: {e:?}"))?;
                self.finished = true;
                Ok(Some(Started {
                    supervisor: self.supervisor.take().expect("started role"),
                    registration,
                    factory: Box::new(factory),
                    lease: self.lease.take().expect("protected spawn lease"),
                    diagnostic: self.diagnostic.take(),
                }))
            }
            Some(Phase::Spawning) => Ok(None),
            _ => Err("role launch authority was retired".into()),
        }
    }
}

#[path = "../../tests/support/authenticated_role_startup.rs"]
mod tests;
