//! One Session owner for the shared Factotum generation and all role launches.
use super::files::{EndpointBudget, TransportBudget};
use super::{Error, LaunchBook, LaunchId, LaunchPolicy, Phase, WorkerAuthority};
use crate::session_unlock_supervisor::LaunchService;
use sophia_factotum::launch::Audiences;
use sophia_factotum::launch::queue::Ingress;
use sophia_runtime::{LaunchChannel, NamespaceRegistry, ProcessSupervisor};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// A registration belongs to exactly one shared Factotum generation. Neither
/// a late owner action nor a retained registration may cross that boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Registration {
    connection: u64,
    launch: LaunchId,
}
impl Registration {
    pub fn launch_generation(self) -> u64 {
        self.launch.generation()
    }
}

/// Session-thread custody for dynamically launched clients. Drop fences local
/// workers immediately; the coordinator reclaims the remote record next turn.
/// It cannot cross threads and race a subsequent activation on the owner.
pub(crate) struct RegistrationOwner {
    registration: Registration,
    _lifetime: Arc<()>,
    active: Arc<AtomicBool>,
    _owner_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl RegistrationOwner {
    pub fn registration(&self) -> Registration {
        self.registration
    }
}
impl Drop for RegistrationOwner {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

struct Connection {
    generation: u64,
    ingress: Ingress,
    book: LaunchBook,
}

pub struct LaunchCoordinator {
    service: LaunchService,
    current: Option<Connection>,
    seen: u64,
    // Supervisors outlive Factotum replacement and refuse reused generations.
    // Preserve burned numbers even if setup failed before inserting a record.
    next_launch: u64,
    transports: TransportBudget,
    owned: Vec<(Registration, Weak<()>)>,
}

impl LaunchCoordinator {
    pub fn relay_deadline(&self) -> Option<Instant> {
        self.current.as_ref()?.book.relay_deadline()
    }

    pub fn subscribe_relays<'a>(
        &'a self,
        start: usize,
        visit: impl FnMut(rustix::event::PollFd<'a>),
    ) -> smallvec::SmallVec<[super::RelayWait<'a>; sophia_factotum::launch::MAX_DELEGATIONS]> {
        self.current
            .as_ref()
            .map_or_else(smallvec::SmallVec::new, |connection| {
                connection.book.subscribe_relays(start, visit)
            })
    }

    pub fn new(service: LaunchService, namespaces: Arc<Mutex<NamespaceRegistry>>) -> Self {
        Self {
            service,
            current: None,
            seen: 0,
            next_launch: 1,
            transports: TransportBudget::new(namespaces),
            owned: Vec::new(),
        }
    }

    /// Retain this endpoint budget across provider and Factotum replacements.
    /// Pending cleanup on an older connection still spends Session capacity.
    pub fn endpoint(&self) -> EndpointBudget {
        self.transports.endpoint()
    }

    pub(crate) fn uses_namespaces(&self, namespaces: &Arc<Mutex<NamespaceRegistry>>) -> bool {
        self.transports.uses_namespaces(namespaces)
    }

    /// Poll once per owner turn, even without a lock or pending PAM request.
    /// Failure revokes the shared generation; only a newly published connection
    /// can recover. A contended publication mutex is not connection loss.
    pub fn poll(&mut self, now: Instant) -> Result<(), Error> {
        self.retire_dropped(now);
        self.transports.poll_retiring(now);
        if self.current.as_ref().is_some_and(|c| !c.ingress.is_open()) {
            self.current = None;
        }
        if let Some(connection) = self.service.current()
            && connection.generation > self.seen
        {
            self.current = None;
            self.seen = connection.generation;
            let book = match LaunchBook::with_next_launch(
                connection.generation,
                connection.ingress.clone(),
                now,
                now + Duration::from_secs(5),
                self.next_launch,
            ) {
                Ok(book) => book,
                Err(error) => {
                    connection.ingress.invalidate();
                    return Err(error);
                }
            };
            self.current = Some(Connection {
                generation: connection.generation,
                ingress: connection.ingress,
                book,
            });
        }
        if let Some(connection) = self.current.as_mut()
            && let Err(error) = connection.book.poll(now)
        {
            self.current = None;
            return Err(error);
        }
        Ok(())
    }

    pub fn ready(&self) -> bool {
        self.current.as_ref().is_some_and(|c| c.book.ready())
    }

    pub fn reserve(&mut self, policy: LaunchPolicy, now: Instant) -> Result<Registration, Error> {
        self.reserve_with_audiences(
            policy,
            Audiences::new(&[policy.audience]).map_err(|_| Error::Invalid)?,
            now,
        )
    }

    pub fn reserve_with_audiences(
        &mut self,
        policy: LaunchPolicy,
        audiences: Audiences,
        now: Instant,
    ) -> Result<Registration, Error> {
        let connection = self.current.as_mut().ok_or(Error::NotReady)?;
        let launch = connection.book.reserve_with_audiences(
            policy,
            audiences,
            now,
            now + Duration::from_secs(10),
        );
        self.next_launch = connection.book.next;
        let launch = launch?;
        Ok(Registration {
            connection: connection.generation,
            launch,
        })
    }

    pub(crate) fn reserve_owned(
        &mut self,
        policy: LaunchPolicy,
        audiences: Audiences,
        now: Instant,
    ) -> Result<RegistrationOwner, Error> {
        self.retire_dropped(now);
        let registration = self.reserve_with_audiences(policy, audiences, now)?;
        let active = self
            .book(registration)?
            .record(registration.launch)?
            .worker_active
            .clone();
        let lifetime = Arc::new(());
        self.owned.push((registration, Arc::downgrade(&lifetime)));
        Ok(RegistrationOwner {
            registration,
            _lifetime: lifetime,
            active,
            _owner_thread: std::marker::PhantomData,
        })
    }

    fn retire_dropped(&mut self, now: Instant) {
        self.owned.retain(|(registration, owner)| {
            let Some(connection) = self
                .current
                .as_mut()
                .filter(|c| c.generation == registration.connection)
            else {
                return false;
            };
            if owner.strong_count() == 0 {
                let _ = connection.book.revoke(registration.launch, now);
                false
            } else {
                true
            }
        });
    }

    pub fn phase(&self, registration: Registration) -> Option<Phase> {
        self.current
            .as_ref()
            .filter(|c| c.generation == registration.connection && c.ingress.is_open())?
            .book
            .phase(registration.launch)
    }

    pub fn take_channel(
        &mut self,
        registration: Registration,
        now: Instant,
    ) -> Result<LaunchChannel, Error> {
        self.book(registration)?
            .take_channel(registration.launch, now)
    }

    pub fn activate(
        &mut self,
        registration: Registration,
        supervisor: &mut ProcessSupervisor,
        now: Instant,
    ) -> Result<bool, Error> {
        self.book(registration)?
            .activate(registration.launch, supervisor, now)
    }

    pub fn worker_authority(
        &mut self,
        registration: Registration,
        supervisor: &mut ProcessSupervisor,
        now: Instant,
    ) -> Result<WorkerAuthority, Error> {
        self.book(registration)?
            .worker_authority(registration.launch, supervisor, now)
    }

    pub fn worker_authority_for_audience(
        &mut self,
        registration: Registration,
        audience: u64,
        supervisor: &mut ProcessSupervisor,
        now: Instant,
    ) -> Result<WorkerAuthority, Error> {
        self.book(registration)?.worker_authority_for_audience(
            registration.launch,
            audience,
            supervisor,
            now,
        )
    }

    pub fn revoke(&mut self, registration: Registration, now: Instant) -> Result<(), Error> {
        self.book(registration)?.revoke(registration.launch, now)
    }

    fn book(&mut self, registration: Registration) -> Result<&mut LaunchBook, Error> {
        if self
            .owned
            .iter()
            .any(|(id, owner)| *id == registration && owner.strong_count() == 0)
        {
            self.retire_dropped(Instant::now());
            return Err(Error::Stale);
        }
        let connection = self
            .current
            .as_mut()
            .filter(|c| c.generation == registration.connection && c.ingress.is_open())
            .ok_or(Error::Stale)?;
        Ok(&mut connection.book)
    }
}
