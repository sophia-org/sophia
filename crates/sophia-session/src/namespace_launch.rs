//! Bounded Session-owned launch scheduling for confined recipe clients.
//! Selection and service approval precede this owner; no peer supplies grants.
use crate::launch_authentication::{
    LaunchCoordinator, LaunchPolicy, Phase, RegistrationOwner,
    spawn::SpawnLease,
    startup::{Recipe, Startup},
};
use crate::namespace_views::{ApprovedEndpoint, NamespaceRecipeOwner, PublishedRecipe};
use sophia_config::namespace_policy::{LoadedNamespaceRecipe, NamespaceRecipeProfile};
use sophia_factotum::launch::Audiences;
use sophia_protocol::{
    NamespaceId,
    namespace_view::{EndpointProtocol, NAMESPACE_DIRECTORY_ENV},
};
use sophia_runtime::{
    NamespaceRegistry, ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionPath,
    SupervisedProcessKind,
    authenticated_files::FileAuthenticationFactory,
    namespace_publication::{CapturedX11Socket, NamespaceListener, NamespaceViewLease},
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Instant,
};

const MAX_CLIENTS: usize = 64;
pub const NAMESPACE_CLIENT_UNAME: &[u8] = b"namespace-client";

/// Trusted policy resolution, including its containment driver and live service
/// references. An arbitrary executable is allowed; its recipe determines reach.
pub struct ClientLaunchRequest {
    pub launch: ProcessLaunchSpec,
    pub recipe: LoadedNamespaceRecipe,
    pub namespace: NamespaceId,
    pub principal: u64,
    pub approved: BTreeMap<String, ApprovedEndpoint>,
    pub x11: Vec<CapturedX11Socket>,
}

struct Slot(Arc<AtomicUsize>);
impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One per Session. Preparing, running and reaping clients all spend the same
/// bound, including workers whose caller has abandoned its result.
pub struct NamespaceLaunchService {
    registry: Arc<Mutex<NamespaceRegistry>>,
    parent: PathBuf,
    slots: Arc<AtomicUsize>,
}
impl NamespaceLaunchService {
    pub fn new(registry: Arc<Mutex<NamespaceRegistry>>, parent: PathBuf) -> Self {
        Self {
            registry,
            parent,
            slots: Arc::new(AtomicUsize::new(0)),
        }
    }
    pub fn occupied(&self) -> usize {
        self.slots.load(Ordering::Acquire)
    }
    pub fn start(
        &self,
        request: ClientLaunchRequest,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Result<PendingNamespaceClient, String> {
        if !launches.uses_namespaces(&self.registry) {
            return Err("namespace launch and authentication owners differ".into());
        }
        if request.recipe.policy().profile == NamespaceRecipeProfile::SharedX {
            return Err("shared-host recipes require the host launch path".into());
        }
        let domain = request
            .launch
            .protection_domain
            .as_ref()
            .ok_or("recipe requires containment")?;
        if domain.roles().len() != 1
            || !domain
                .roles()
                .contains(&ProtectionDomainRole::NamespaceClient)
        {
            return Err("recipe client requires exclusive namespace-client custody".into());
        }
        let audiences: Vec<_> = request
            .approved
            .values()
            .map(|e| e.audience)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let audiences = Audiences::new(&audiences).map_err(|_| "invalid recipe audience set")?;
        let primary = request
            .approved
            .iter()
            .find(|(_, e)| e.service.protocol == EndpointProtocol::NineP2000L)
            .ok_or("recipe has no authenticated endpoint")?
            .1
            .audience;
        self.slots
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_CLIENTS).then_some(n + 1)
            })
            .map_err(|_| "namespace client capacity exhausted")?;
        let slot = Slot(self.slots.clone());
        let policy = LaunchPolicy {
            principal: request.principal,
            namespace: request.namespace.raw(),
            audience: primary,
            role: ProtectionDomainRole::NamespaceClient,
        };
        let registration = launches
            .reserve_owned(policy, audiences, now)
            .map_err(|e| format!("recipe registration: {e:?}"))?;
        let generation = registration.registration().launch_generation();
        let registry = self.registry.clone();
        let parent = self.parent.clone();
        let (sender, prepared) = mpsc::sync_channel(1);
        let (reaper, lifetime) = SpawnLease::channel();
        std::thread::Builder::new()
            .name("sophia-recipe-prepare".into())
            .spawn(move || {
                let result = prepare(request, registry, parent, generation, slot);
                let _ = sender.send(result);
                wake.notify();
                if let Ok(resources) = lifetime.recv() {
                    drop(resources);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(PendingNamespaceClient {
            registration: Some(registration),
            prepared: Some(prepared),
            reaper: Some(reaper),
            resources: None,
            startup: None,
            policy,
            finished: false,
        })
    }
}

struct Prepared {
    supervisor: Option<ProcessSupervisor>,
    names: NamespaceRecipeOwner,
    published: PublishedRecipe,
    approved: BTreeMap<String, ApprovedEndpoint>,
    slot: Slot,
}
fn prepare(
    mut request: ClientLaunchRequest,
    registry: Arc<Mutex<NamespaceRegistry>>,
    parent: PathBuf,
    generation: u64,
    slot: Slot,
) -> Result<Prepared, String> {
    let mut names = NamespaceRecipeOwner::new(
        &parent,
        registry,
        request.namespace,
        request.principal,
        generation,
    )
    .map_err(|e| e.to_string())?;
    let published = names
        .publish_selected_with_x11(1, &request.recipe, &request.approved, &request.x11)
        .map_err(|e| e.to_string())?;
    let directory = published.namespace.lease.directory();
    request
        .launch
        .environment
        .retain(|(key, _)| key != NAMESPACE_DIRECTORY_ENV);
    request
        .launch
        .environment
        .push((NAMESPACE_DIRECTORY_ENV.into(), directory.as_os_str().into()));
    let domain = request
        .launch
        .protection_domain
        .take()
        .ok_or("missing recipe domain")?
        .path(ProtectionPath::read_only(directory))
        .map_err(|e| e.to_string())?;
    request.launch.protection_domain = Some(domain);
    Ok(Prepared {
        supervisor: Some(ProcessSupervisor::new(
            SupervisedProcessKind::NamespaceClient,
            request.launch,
        )),
        names,
        published,
        approved: request.approved,
        slot,
    })
}

pub struct ClientEndpoint {
    pub listener: NamespaceListener,
    pub view: NamespaceViewLease,
    pub factory: Box<dyn FileAuthenticationFactory>,
}

pub struct PendingNamespaceClient {
    registration: Option<RegistrationOwner>,
    prepared: Option<mpsc::Receiver<Result<Prepared, String>>>,
    reaper: Option<SpawnLease>,
    resources: Option<Prepared>,
    startup: Option<Startup>,
    policy: LaunchPolicy,
    finished: bool,
}
impl PendingNamespaceClient {
    /// Call after the shared coordinator's owner turn. Pending work never waits
    /// for filesystem access, Factotum or Bubblewrap on this thread.
    pub fn poll(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Result<Option<RunningNamespaceClient>, String> {
        if self.finished {
            return Err("namespace launch already consumed".into());
        }
        let result = self.advance(launches, wake, now);
        if result.is_err() {
            self.retire();
            self.finished = true;
        }
        result
    }
    fn advance(
        &mut self,
        launches: &mut LaunchCoordinator,
        wake: sophia_wake::Notifier,
        now: Instant,
    ) -> Result<Option<RunningNamespaceClient>, String> {
        let registration = self
            .registration
            .as_ref()
            .ok_or("namespace launch cancelled")?
            .registration();
        if matches!(launches.phase(registration), None | Some(Phase::Retiring)) {
            return Err("namespace launch authority retired".into());
        }
        if self.resources.is_none() {
            match self
                .prepared
                .as_ref()
                .expect("preparation receiver")
                .try_recv()
            {
                Ok(result) => self.resources = Some(result?),
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("recipe preparation worker stopped".into());
                }
            }
        }
        let resources = self.resources.as_mut().expect("prepared namespace");
        if self.startup.is_none() {
            let primary = resources
                .published
                .namespace
                .listeners
                .first()
                .ok_or("recipe exposes no 9P endpoint")?;
            let grant = resources
                .published
                .grant(&primary.endpoint.endpoint)
                .ok_or("primary endpoint grant absent")?
                .clone();
            let audience = resources
                .approved
                .values()
                .find(|e| e.service == primary.endpoint.endpoint.service)
                .ok_or("primary service absent")?
                .audience;
            let policy = LaunchPolicy {
                audience,
                ..self.policy
            };
            self.startup = Some(Startup::registered_endpoint(
                resources.supervisor.take().expect("prepared supervisor"),
                launches.endpoint(),
                Recipe {
                    policy,
                    uname: NAMESPACE_CLIENT_UNAME.to_vec(),
                    aname: primary
                        .endpoint
                        .endpoint
                        .service
                        .interface
                        .as_bytes()
                        .to_vec(),
                },
                registration,
                grant,
                now,
            )?);
        }
        let Some(started) = self
            .startup
            .as_mut()
            .expect("namespace startup")
            .poll(launches, wake, now)?
        else {
            return Ok(None);
        };
        let mut resources = self.resources.take().expect("prepared resources");
        // Install retirement custody before any fallible endpoint binding.
        let mut running = RunningNamespaceClient {
            registration: self.registration.take(),
            supervisor: Some(started.supervisor),
            names: Some(resources.names),
            slot: Some(resources.slot),
            lease: Some(started.lease),
            reaper: self.reaper.take(),
            endpoints: Vec::new(),
        };
        let mut primary_factory = Some(started.factory);
        for listener in std::mem::take(&mut resources.published.namespace.listeners) {
            let factory = if let Some(factory) = primary_factory.take() {
                factory
            } else {
                let grant = resources
                    .published
                    .grant(&listener.endpoint.endpoint)
                    .ok_or("endpoint grant absent")?
                    .clone();
                let audience = resources
                    .approved
                    .values()
                    .find(|e| e.service == listener.endpoint.endpoint.service)
                    .ok_or("endpoint service absent")?
                    .audience;
                let authority = launches
                    .worker_authority_for_audience(
                        registration,
                        audience,
                        running.supervisor.as_mut().expect("started supervisor"),
                        now,
                    )
                    .map_err(|e| format!("recipe endpoint custody: {e:?}"))?;
                Box::new(
                    launches
                        .endpoint()
                        .bind_grant(
                            authority,
                            grant,
                            &listener.endpoint.endpoint,
                            NAMESPACE_CLIENT_UNAME.to_vec(),
                            listener
                                .endpoint
                                .endpoint
                                .service
                                .interface
                                .as_bytes()
                                .to_vec(),
                        )
                        .map_err(|e| format!("recipe endpoint binding: {e:?}"))?,
                )
            };
            running.endpoints.push(ClientEndpoint {
                listener,
                factory,
                view: resources.published.namespace.lease.clone(),
            });
        }
        self.finished = true;
        Ok(Some(running))
    }

    fn retire(&mut self) {
        self.registration.take();
        if let Some(resources) = self.resources.as_mut() {
            resources.names.revoke_grants();
        }
        if let Some(reaper) = self.reaper.take() {
            reaper.retire(RetiredPending {
                startup: self.startup.take(),
                _resources: self.resources.take(),
                _prepared: self.prepared.take(),
            });
        }
    }
}

impl Drop for PendingNamespaceClient {
    fn drop(&mut self) {
        self.retire();
    }
}

struct RetiredPending {
    startup: Option<Startup>,
    _resources: Option<Prepared>,
    _prepared: Option<mpsc::Receiver<Result<Prepared, String>>>,
}
impl Drop for RetiredPending {
    fn drop(&mut self) {
        if let Some(startup) = self.startup.take() {
            startup.retire();
        }
    }
}

/// Keeps the view, grant owners and spawn thread alive through the child's
/// lifetime. Dispatch must retain each endpoint view while serving its streams.
pub struct RunningNamespaceClient {
    registration: Option<RegistrationOwner>,
    supervisor: Option<ProcessSupervisor>,
    names: Option<NamespaceRecipeOwner>,
    slot: Option<Slot>,
    lease: Option<SpawnLease>,
    reaper: Option<SpawnLease>,
    endpoints: Vec<ClientEndpoint>,
}
impl RunningNamespaceClient {
    pub fn take_endpoints(&mut self) -> Vec<ClientEndpoint> {
        std::mem::take(&mut self.endpoints)
    }
    pub fn peer_id(&self) -> Option<u32> {
        self.supervisor
            .as_ref()
            .and_then(ProcessSupervisor::peer_id)
    }
    pub fn poll(
        &mut self,
        launches: &LaunchCoordinator,
    ) -> Result<Option<std::process::ExitStatus>, String> {
        let registration = self
            .registration
            .as_ref()
            .ok_or("namespace client retired")?
            .registration();
        if launches.phase(registration) != Some(Phase::Active) {
            return Err("namespace client authority retired".into());
        }
        let supervisor = self
            .supervisor
            .as_mut()
            .ok_or("namespace client process absent")?;
        supervisor.poll().map_err(|e| e.to_string())?;
        let status = supervisor.take_exit_status();
        if status.is_some() {
            self.registration.take();
            self.names.as_mut().expect("live view").revoke_grants();
        }
        Ok(status)
    }
}
impl Drop for RunningNamespaceClient {
    fn drop(&mut self) {
        self.registration.take();
        if let Some(names) = self.names.as_mut() {
            names.revoke_grants();
        }
        if let Some(reaper) = self.reaper.take() {
            reaper.retire((
                self.supervisor.take(),
                self.names.take(),
                self.slot.take(),
                self.lease.take(),
            ));
        }
    }
}

#[path = "../tests/support/namespace_client_launch.rs"]
mod tests;
