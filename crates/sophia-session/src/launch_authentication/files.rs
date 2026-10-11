//! Session authentication and quota ownership for runtime file transports.
use super::{
    Authority, WorkerAuthority,
    role::{RoleAuthentication, RoleScope},
};
use sophia_9p::{
    ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt, export::AttachContext,
};
use sophia_factotum::launch::queue::Ingress;
use sophia_runtime::authenticated_files::{FileAuthentication, FileAuthenticationFactory};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const GLOBAL_TRANSPORTS: usize = 32;
const ENDPOINT_TRANSPORTS: usize = 8;

#[derive(Default)]
struct Global {
    used: AtomicUsize,
    next: AtomicU64,
}
/// One instance per Session, shared by every authenticated role endpoint.
#[derive(Clone, Default)]
pub struct TransportBudget(Arc<Global>);
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
    pub fn endpoint(&self) -> EndpointBudget {
        EndpointBudget {
            global: self.0.clone(),
            used: Arc::new(AtomicUsize::new(0)),
        }
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
        })
    }
}

pub struct RoleFileFactory {
    endpoint: EndpointBudget,
    worker: WorkerAuthority,
    uname: Vec<u8>,
    aname: Vec<u8>,
}
impl FileAuthenticationFactory for RoleFileFactory {
    fn create(
        &mut self,
        connection: ConnectionId,
        now: Instant,
        wake: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        if !self.worker.live().map_err(|_| Errno::ESTALE)? {
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
        self.auth.begin(afid.0, now)
    }
    fn read(
        &mut self,
        attempt: Attempt,
        offset: u64,
        count: u32,
        now: Instant,
    ) -> Result<ReadOutcome, Errno> {
        self.auth.read(
            attempt,
            offset,
            count,
            now,
            &mut Authority::Worker(&self.worker),
        )
    }
    fn commit(&mut self, attempt: Attempt, now: Instant) -> Result<(), Errno> {
        self.auth
            .commit_attach(attempt, now, &mut Authority::Worker(&self.worker))
            .map(drop)
    }
    fn check(&mut self, connection: ConnectionId, now: Instant) -> Result<(), Errno> {
        if connection != self.connection {
            return Err(Errno::EACCES);
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
        self.auth.close(now);
    }
    fn poll(&mut self, now: Instant) -> Result<(), Errno> {
        let result = self.auth.poll(now);
        if !self.closed && !self.worker.live().map_err(|_| Errno::ESTALE)? {
            self.close(now);
            return Err(Errno::ESTALE);
        }
        result
    }
    fn drained(&self) -> bool {
        self.auth.drained()
    }
    fn admitted(&self) -> bool {
        self.auth.pinned().is_some()
    }
    fn wait(&self, _now: Instant) -> Duration {
        // Factotum answers also wake the core. This bounded idle cadence checks
        // launch exit/revocation and deadlines when neither peer sends traffic.
        Duration::from_millis(20)
    }
}
