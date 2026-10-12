//! Session retains revocation ownership when handing a launch to a role worker.
use super::*;
use sophia_runtime::LaunchCustody;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Current authority for one registered launch, not a cached Factotum verdict.
/// No constructor accepts peer-supplied identity or process handles.
#[derive(Clone)]
pub struct WorkerAuthority {
    session: u64,
    delegation: Delegation,
    audience: u64,
    active: Arc<AtomicBool>,
    ingress: Ingress,
    custody: LaunchCustody,
}

impl WorkerAuthority {
    pub(super) fn identity(&self) -> Identity {
        self.delegation.identity
    }
    pub(super) fn session(&self) -> u64 {
        self.session
    }
    pub(super) fn audience(&self) -> u64 {
        self.audience
    }
    pub(super) fn ingress(&self) -> Ingress {
        self.ingress.clone()
    }
    pub(super) fn live(&self) -> Result<bool, Error> {
        Ok(self.active.load(Ordering::Acquire)
            && self.ingress.is_open()
            && self.custody.is_live()?
            && self.active.load(Ordering::Acquire)
            && self.ingress.is_open())
    }
    fn authorize(&self, verdict: Verdict) -> Result<Identity, Error> {
        if verdict.attempt.session != self.session
            || verdict.identity != self.delegation.identity
            || verdict.attempt.audience != self.audience
            || !self.live()?
        {
            return Err(Error::Stale);
        }
        Ok(self.delegation.identity)
    }
}

/// The same final check runs on the Session owner or on its bounded worker.
/// Both paths join current registration, Factotum connection and own-spawn
/// custody. Only the owner path may initiate remote retirement here.
pub enum Authority<'a> {
    Owner {
        book: &'a mut LaunchBook,
        supervisor: &'a mut ProcessSupervisor,
    },
    Worker(&'a WorkerAuthority),
}
impl Authority<'_> {
    pub(super) fn authorize(&mut self, verdict: Verdict, now: Instant) -> Result<Identity, Error> {
        match self {
            Self::Owner { book, supervisor } => book.authorize(verdict, supervisor, now),
            Self::Worker(authority) => authority.authorize(verdict),
        }
    }
}

impl LaunchBook {
    /// Publish only after Factotum activation and this reservation's protected
    /// spawn. Revocation and connection loss invalidate every clone immediately,
    /// independently of worker command queues or remote Retire progress.
    pub fn worker_authority(
        &mut self,
        id: LaunchId,
        supervisor: &mut ProcessSupervisor,
        now: Instant,
    ) -> Result<WorkerAuthority, Error> {
        let audience = self.record(id)?.delegation.audience;
        self.worker_authority_for_audience(id, audience, supervisor, now)
    }

    /// Narrow each endpoint worker to one approved audience. A worker cannot
    /// use a verdict from another endpoint even within the same launch set.
    pub fn worker_authority_for_audience(
        &mut self,
        id: LaunchId,
        audience: u64,
        supervisor: &mut ProcessSupervisor,
        now: Instant,
    ) -> Result<WorkerAuthority, Error> {
        if !self.ready() {
            return Err(Error::Closed);
        }
        let session = self.session;
        let ingress = self.ingress.clone();
        let record = self.record(id)?;
        if record.phase != Phase::Active {
            return Err(Error::NotReady);
        }
        if !record.audiences.contains(audience) {
            return Err(Error::Invalid);
        }
        let custody = match supervisor.launch_custody(&record.reservation, record.role)? {
            Some(custody) => custody,
            None => {
                self.revoke(id, now)?;
                return Err(Error::Stale);
            }
        };
        Ok(WorkerAuthority {
            session,
            delegation: record.delegation,
            audience,
            active: record.worker_active.clone(),
            ingress,
            custody,
        })
    }
}
