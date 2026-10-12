//! Session authentication and quota ownership for runtime file transports.
use super::{
    Authority, WorkerAuthority,
    role::{RoleAuthentication, RoleScope},
};
use sophia_9p::{
    ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt, export::AttachContext,
};
use sophia_factotum::launch::queue::Ingress;
use sophia_protocol::{ClientAdmissionContext, ClientAuthenticationMethod, NamespaceId};
use sophia_runtime::authenticated_files::{FileAuthentication, FileAuthenticationFactory};
use sophia_runtime::{AdmissionLease, EndpointGrantLease, NamespaceRegistry};
use std::{
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    time::{Duration, Instant},
};

const GLOBAL_TRANSPORTS: usize = 32;
const ENDPOINT_TRANSPORTS: usize = 8;

struct Global {
    used: AtomicUsize,
    next: AtomicU64,
    namespaces: Arc<Mutex<NamespaceRegistry>>,
    retiring: SyncSender<Box<dyn FileAuthentication>>,
}
struct Retiring {
    incoming: Receiver<Box<dyn FileAuthentication>>,
    owners: Vec<Box<dyn FileAuthentication>>,
}
/// One instance per Session, shared by every authenticated role endpoint.
#[derive(Clone)]
pub struct TransportBudget(Arc<Global>, Arc<Mutex<Retiring>>);
#[derive(Clone)]
pub struct EndpointBudget {
    global: Arc<Global>,
    used: Arc<AtomicUsize>,
}
struct Permit {
    endpoint: EndpointBudget,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.endpoint.used.fetch_sub(1, Ordering::AcqRel);
        self.endpoint.global.used.fetch_sub(1, Ordering::AcqRel);
    }
}
impl TransportBudget {
    pub fn new(namespaces: Arc<Mutex<NamespaceRegistry>>) -> Self {
        let (retiring, incoming) = sync_channel(GLOBAL_TRANSPORTS);
        Self(
            Arc::new(Global {
                used: AtomicUsize::new(0),
                next: AtomicU64::new(0),
                namespaces,
                retiring,
            }),
            Arc::new(Mutex::new(Retiring {
                incoming,
                owners: Vec::new(),
            })),
        )
    }
    pub fn endpoint(&self) -> EndpointBudget {
        EndpointBudget {
            global: self.0.clone(),
            used: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Closed owners keep their permits until remote cancellation completes.
    /// The receiver is owned separately from permits, avoiding a retention
    /// cycle when Session drops its last budget owner.
    pub fn poll_retiring(&self, now: Instant) {
        let Ok(mut retiring) = self.1.try_lock() else {
            return;
        };
        while retiring.owners.len() < GLOBAL_TRANSPORTS {
            let Ok(owner) = retiring.incoming.try_recv() else {
                break;
            };
            retiring.owners.push(owner);
        }
        retiring.owners.retain_mut(|owner| {
            let _ = owner.poll(now);
            !owner.drained()
        });
    }
}
impl EndpointBudget {
    fn reserve(&self) -> Result<(u64, Permit), Errno> {
        self.global
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < GLOBAL_TRANSPORTS).then_some(n + 1)
            })
            .map_err(|_| Errno::EAGAIN)?;
        if self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < ENDPOINT_TRANSPORTS).then_some(n + 1)
            })
            .is_err()
        {
            self.global.used.fetch_sub(1, Ordering::AcqRel);
            return Err(Errno::EAGAIN);
        }
        let permit = Permit {
            endpoint: self.clone(),
        };
        let id = self
            .global
            .next
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_err(|_| Errno::ENOSPC)?
            + 1;
        Ok((id, permit))
    }

    /// Reuse this endpoint budget across launch replacements. Old conversations
    /// retain their slots while cancellation is pending on the shared connection.
    pub fn bind(
        &self,
        worker: WorkerAuthority,
        uname: Vec<u8>,
        aname: Vec<u8>,
    ) -> Result<RoleFileFactory, Errno> {
        if uname.len() > 255 || aname.len() > 255 {
            return Err(Errno::EINVAL);
        }
        Ok(RoleFileFactory {
            endpoint: self.clone(),
            worker,
            uname,
            aname,
            grant: None,
        })
    }

    /// Recipe endpoints retain the exact grant selected during publication.
    /// A matching name, service or principal cannot substitute another scope.
    pub fn bind_grant(
        &self,
        worker: WorkerAuthority,
        grant: EndpointGrantLease,
        endpoint: &sophia_protocol::namespace_view::GrantedEndpoint,
        uname: Vec<u8>,
        aname: Vec<u8>,
    ) -> Result<RoleFileFactory, Errno> {
        let identity = worker.identity();
        let method = ClientAuthenticationMethod::FactotumLaunch {
            principal: identity.principal,
            launch_generation: identity.launch,
        };
        if grant.endpoint() != endpoint
            || !grant.permits(
                NamespaceId::from_raw(identity.namespace),
                method,
                worker.audience(),
            )
        {
            return Err(Errno::EACCES);
        }
        let registry = self
            .global
            .namespaces
            .try_lock()
            .map_err(|error| match error {
                TryLockError::WouldBlock => Errno::EAGAIN,
                TryLockError::Poisoned(_) => Errno::EIO,
            })?;
        if !registry.endpoint_grant_is_current(&grant) {
            return Err(Errno::ESTALE);
        }
        let mut factory = self.bind(worker, uname, aname)?;
        factory.grant = Some(grant);
        Ok(factory)
    }
}

pub struct RoleFileFactory {
    endpoint: EndpointBudget,
    worker: WorkerAuthority,
    uname: Vec<u8>,
    aname: Vec<u8>,
    grant: Option<EndpointGrantLease>,
}
impl FileAuthenticationFactory for RoleFileFactory {
    fn retire(
        &mut self,
        mut owner: Box<dyn FileAuthentication>,
    ) -> Result<(), Box<dyn FileAuthentication>> {
        owner.close(Instant::now());
        if owner.drained() {
            return Ok(());
        }
        self.endpoint
            .global
            .retiring
            .try_send(owner)
            .map_err(|error| match error {
                TrySendError::Full(owner) | TrySendError::Disconnected(owner) => owner,
            })
    }

    fn create(
        &mut self,
        connection: ConnectionId,
        now: Instant,
        wake: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        if !self.worker.live().map_err(|_| Errno::ESTALE)?
            || self.grant.as_ref().is_some_and(|grant| !grant.is_live())
        {
            return Err(Errno::ESTALE);
        }
        let (server, permit) = self.endpoint.reserve()?;
        let ingress = self.worker.ingress().with_wake(move || wake.wake());
        let auth = RoleAuthentication::new(
            RoleScope {
                server,
                connection,
                session: self.worker.session(),
                audience: self.worker.audience(),
                uname: self.uname.clone(),
                aname: self.aname.clone(),
            },
            ingress.clone(),
            now,
        )?;
        Ok(Box::new(RoleFileAuthentication {
            auth,
            worker: self.worker.clone(),
            connection,
            ingress,
            closed: false,
            admission: None,
            grant: self.grant.clone(),
            _permit: permit,
        }))
    }
}

struct RoleFileAuthentication {
    auth: RoleAuthentication,
    worker: WorkerAuthority,
    connection: ConnectionId,
    ingress: Ingress,
    closed: bool,
    admission: Option<AdmissionLease>,
    grant: Option<EndpointGrantLease>,
    _permit: Permit,
}
impl Drop for RoleFileAuthentication {
    fn drop(&mut self) {
        self.auth.close(Instant::now());
        // An owner abandoned during construction, panic or shutdown must not
        // release a slot while its remote attempt can still consume authority.
        if !self.auth.drained() {
            self.ingress.invalidate();
        }
    }
}
impl FileAuthentication for RoleFileAuthentication {
    fn claims(&self, c: &AttachContext<'_>) -> Result<(), Errno> {
        self.auth.claims(c.connection, c.uname, c.aname, c.n_uname)
    }
    fn begin(&mut self, afid: Fid, now: Instant) -> Result<Attempt, Errno> {
        if self.grant.as_ref().is_some_and(|grant| !grant.is_live()) {
            return Err(Errno::ESTALE);
        }
        self.auth.begin(afid.0, now)
    }
    fn read(
        &mut self,
        attempt: Attempt,
        offset: u64,
        count: u32,
        now: Instant,
    ) -> Result<ReadOutcome, Errno> {
        if self.grant.as_ref().is_some_and(|grant| !grant.is_live()) {
            return Err(Errno::ESTALE);
        }
        self.auth.read(
            attempt,
            offset,
            count,
            now,
            &mut Authority::Worker(&self.worker),
        )
    }
    fn commit(&mut self, attempt: Attempt, now: Instant) -> Result<(), Errno> {
        let mut authority = Authority::Worker(&self.worker);
        let identity = self.auth.prepare_identity(attempt, now, &mut authority)?;
        let method = ClientAuthenticationMethod::FactotumLaunch {
            principal: identity.principal,
            launch_generation: identity.launch,
        };
        if let Some(admission) = &self.admission {
            let context = admission.context();
            if !admission.is_live() {
                return Err(Errno::ESTALE);
            }
            if context.namespace.id.raw() != identity.namespace
                || context.auth_provenance.method != method
            {
                return Err(Errno::EACCES);
            }
            return self
                .auth
                .commit_attach(attempt, now, &mut authority)
                .map(drop);
        }
        // Reserve the registry result before pinning authentication. Neither
        // registry publication nor the already-prepared role root can fail
        // afterwards. Contention refuses this attach; it never blocks a worker.
        let mut namespaces =
            self._permit
                .endpoint
                .global
                .namespaces
                .try_lock()
                .map_err(|error| match error {
                    TryLockError::WouldBlock => Errno::EAGAIN,
                    TryLockError::Poisoned(_) => Errno::EIO,
                })?;
        let prepared = match &self.grant {
            Some(grant) => {
                namespaces.prepare_granted_admission(grant, method, self.worker.audience())
            }
            None => namespaces.prepare_admission(NamespaceId::from_raw(identity.namespace), method),
        }
        .map_err(|_| Errno::EACCES)?;
        self.auth.commit_attach(attempt, now, &mut authority)?;
        self.admission = Some(prepared.publish_lease());
        Ok(())
    }
    fn check(&mut self, connection: ConnectionId, now: Instant) -> Result<(), Errno> {
        if connection != self.connection {
            return Err(Errno::EACCES);
        }
        if self.closed {
            return Err(Errno::ESTALE);
        }
        if !self.admission.as_ref().ok_or(Errno::EACCES)?.is_live() {
            return Err(Errno::ESTALE);
        }
        self.auth
            .check_admitted(now, &mut Authority::Worker(&self.worker))
            .map(drop)
    }
    fn release(&mut self, attempt: Attempt, now: Instant) {
        self.auth.release(attempt, now);
    }
    fn reset(&mut self, now: Instant) -> Result<(), Errno> {
        self.auth.reset(now)
    }
    fn close(&mut self, now: Instant) {
        self.closed = true;
        self.admission = None;
        self.auth.close(now);
    }
    fn poll(&mut self, now: Instant) -> Result<(), Errno> {
        let result = self.auth.poll(now);
        if !self.closed
            && (result.is_err()
                || !self.worker.live().unwrap_or(false)
                || self.grant.as_ref().is_some_and(|grant| !grant.is_live())
                || self
                    .admission
                    .as_ref()
                    .is_some_and(|admission| !admission.is_live()))
        {
            self.close(now);
            return Err(Errno::ESTALE);
        }
        result
    }
    fn drained(&self) -> bool {
        self.auth.drained()
    }
    fn admitted(&self) -> bool {
        self.admission.as_ref().is_some_and(AdmissionLease::is_live)
    }
    fn admission_context(&self) -> Option<ClientAdmissionContext> {
        self.admission.as_ref().map(AdmissionLease::context)
    }
    fn wait(&self, _now: Instant) -> Duration {
        // Factotum answers also wake the core. This bounded idle cadence checks
        // launch exit/revocation and deadlines when neither peer sends traffic.
        Duration::from_millis(20)
    }
}
