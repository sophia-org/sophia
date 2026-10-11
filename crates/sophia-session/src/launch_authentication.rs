//! Session's launch registration and custody join. Facts come from trusted
//! launch policy and ProcessSupervisor, never from the provisioned child.
//! Registration/activation/retirement are nonblocking Factotum exchanges.

use sophia_9p::authentication::Identity;
use sophia_factotum::launch::{
    Delegation, MAX_DELEGATIONS, Verdict,
    queue::{Error as QueueError, Ingress, Ticket},
    relay::{Relay, RelayServer},
    wire::{Command, Response},
};
use sophia_runtime::{LaunchChannel, LaunchReservation, ProcessSupervisor, ProtectionDomainRole};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

mod custody;
pub use custody::{Authority, WorkerAuthority};
pub mod files;
pub mod role;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchId(u64);
impl LaunchId {
    pub fn generation(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy)]
pub struct LaunchPolicy {
    pub principal: u64,
    pub namespace: u64,
    pub audience: u64,
    pub role: ProtectionDomainRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Registering,
    Ready,
    Spawning,
    Activating,
    Active,
    Retiring,
}

#[derive(Debug)]
pub enum Error {
    Invalid,
    NotReady,
    Stale,
    Capacity,
    Closed,
    Queue(QueueError),
    Io(std::io::Error),
}
impl From<QueueError> for Error {
    fn from(error: QueueError) -> Self {
        Self::Queue(error)
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

struct Record {
    worker_active: Arc<AtomicBool>,
    delegation: Delegation,
    reservation: LaunchReservation,
    role: ProtectionDomainRole,
    phase: Phase,
    deadline: Instant,
    ticket: Option<Ticket>,
    channel: Option<LaunchChannel>,
    relay: Option<RelayServer>,
}

/// One book per shared Factotum connection. A replaced connection needs a new
/// book and new launches; old records cannot migrate to the replacement owner.
pub struct LaunchBook {
    session: u64,
    ingress: Ingress,
    initializing: Option<Ticket>,
    initialized: bool,
    next: u64,
    records: Vec<Record>,
}

impl LaunchBook {
    pub fn new(
        session: u64,
        ingress: Ingress,
        now: Instant,
        deadline: Instant,
    ) -> Result<Self, Error> {
        if session == 0 {
            return Err(Error::Invalid);
        }
        let initializing = Some(ingress.submit(Command::Initialize { session }, now, deadline)?);
        Ok(Self {
            session,
            ingress,
            initializing,
            initialized: false,
            next: 1,
            records: Vec::with_capacity(MAX_DELEGATIONS),
        })
    }

    pub fn ready(&self) -> bool {
        self.initialized && self.ingress.is_open()
    }
    pub fn phase(&self, id: LaunchId) -> Option<Phase> {
        self.records
            .iter()
            .find(|r| r.delegation.identity.launch == id.0)
            .map(|r| r.phase)
    }

    pub fn reserve(
        &mut self,
        policy: LaunchPolicy,
        now: Instant,
        deadline: Instant,
    ) -> Result<LaunchId, Error> {
        if !self.ready() {
            return Err(Error::NotReady);
        }
        if policy.principal == 0
            || policy.namespace == 0
            || policy.audience == 0
            || deadline <= now
            || deadline.duration_since(now) > Duration::from_secs(10)
        {
            return Err(Error::Invalid);
        }
        if self.records.len() >= MAX_DELEGATIONS {
            return Err(Error::Capacity);
        }
        let generation = self.next;
        self.next = self.next.checked_add(1).ok_or(Error::Capacity)?;
        // Burn before any fallible setup. No failed handoff can reuse this ID.
        let delegation = Delegation {
            identity: Identity {
                principal: policy.principal,
                namespace: policy.namespace,
                launch: generation,
            },
            audience: policy.audience,
        };
        let (stream, channel) = LaunchChannel::reserve(generation)?;
        let reservation = channel.reservation();
        let relay = Relay::over(stream, generation, self.ingress.clone())?;
        let ticket = self
            .ingress
            .submit(Command::Reserve(delegation), now, deadline)?;
        self.records.push(Record {
            worker_active: Arc::new(AtomicBool::new(false)),
            delegation,
            reservation,
            role: policy.role,
            phase: Phase::Registering,
            deadline,
            ticket: Some(ticket),
            channel: Some(channel),
            relay: Some(relay),
        });
        Ok(LaunchId(generation))
    }

    /// Only the registered reservation yields a child endpoint, exactly once.
    /// Pass it to the existing protected launch worker, not an Engine wait.
    pub fn take_channel(&mut self, id: LaunchId, now: Instant) -> Result<LaunchChannel, Error> {
        if !self.ready() {
            return Err(Error::Closed);
        }
        let record = self.record(id)?;
        if record.phase != Phase::Ready || now >= record.deadline {
            return Err(Error::NotReady);
        }
        record.phase = Phase::Spawning;
        record.channel.take().ok_or(Error::Stale)
    }

    /// Called after this reservation's own protected spawn completes. Both
    /// requesting activation and publishing its answer recheck live custody.
    pub fn activate(
        &mut self,
        id: LaunchId,
        supervisor: &mut ProcessSupervisor,
        now: Instant,
    ) -> Result<bool, Error> {
        if !self.ready() {
            return Err(Error::Closed);
        }
        let ingress = self.ingress.clone();
        let record = self.record(id)?;
        if !matches!(record.phase, Phase::Spawning | Phase::Activating) || now >= record.deadline {
            return Err(Error::NotReady);
        }
        if !supervisor
            .has_live_launch(&record.reservation, record.role)
            .unwrap_or(false)
        {
            self.revoke(id, now)?;
            return Err(Error::Stale);
        }
        if record.phase == Phase::Spawning {
            record.ticket =
                Some(ingress.submit(Command::Activate { launch: id.0 }, now, record.deadline)?);
            record.phase = Phase::Activating;
            return Ok(false);
        }
        match record.ticket.as_mut().expect("activation ticket").take(now) {
            Ok(None) => Ok(false),
            Ok(Some(Response::Done)) => {
                record.ticket = None;
                record
                    .relay
                    .as_mut()
                    .expect("live relay")
                    .export_mut()
                    .activate(id.0)
                    .map_err(|_| Error::Stale)?;
                record.phase = Phase::Active;
                record.worker_active.store(true, Ordering::Release);
                Ok(true)
            }
            _ => {
                self.revoke(id, now)?;
                Err(Error::Stale)
            }
        }
    }

    /// The afid owner must also validate its exact attempt/deadline and current
    /// policy before pinning identity. This checks the independent launch half.
    pub fn authorize(
        &mut self,
        verdict: Verdict,
        supervisor: &mut ProcessSupervisor,
        now: Instant,
    ) -> Result<Identity, Error> {
        if !self.ready() || verdict.attempt.session != self.session {
            return Err(Error::Closed);
        }
        let id = LaunchId(verdict.identity.launch);
        let record = self.record(id)?;
        if record.phase != Phase::Active
            || record.delegation.identity != verdict.identity
            || record.delegation.audience != verdict.attempt.audience
        {
            return Err(Error::Stale);
        }
        if !supervisor
            .has_live_launch(&record.reservation, record.role)
            .unwrap_or(false)
        {
            self.revoke(id, now)?;
            return Err(Error::Stale);
        }
        Ok(record.delegation.identity)
    }

    /// Local authority disappears before any remote cleanup can block or fail.
    pub fn revoke(&mut self, id: LaunchId, now: Instant) -> Result<(), Error> {
        let record = self.record(id)?;
        if record.phase == Phase::Retiring {
            return Ok(());
        }
        record.phase = Phase::Retiring;
        record.worker_active.store(false, Ordering::Release);
        record.channel = None;
        record.relay = None;
        record.ticket = None;
        record.deadline = now + Duration::from_secs(5);
        Ok(())
    }

    /// Bounded owner turn: at most 64 records and one nonblocking core turn per
    /// relay. Activation publication always goes through activate(supervisor).
    pub fn poll(&mut self, now: Instant) -> Result<(), Error> {
        if !self.ingress.is_open() {
            self.records.clear();
            return Err(Error::Closed);
        }
        if let Some(ticket) = self.initializing.as_mut() {
            match ticket.take(now) {
                Ok(None) => return Ok(()),
                Ok(Some(Response::Done)) => {
                    self.initializing = None;
                    self.initialized = true;
                }
                _ => {
                    self.ingress.invalidate();
                    self.records.clear();
                    return Err(Error::Closed);
                }
            }
        }
        let mut index = 0;
        while index < self.records.len() {
            let id = LaunchId(self.records[index].delegation.identity.launch);
            if !matches!(self.records[index].phase, Phase::Active | Phase::Retiring)
                && now >= self.records[index].deadline
            {
                self.revoke(id, now)?;
            }
            let record = &mut self.records[index];
            if let Some(relay) = record.relay.as_mut()
                && !relay.turn(Some(Duration::ZERO)).unwrap_or(false)
            {
                self.revoke(id, now)?;
                continue;
            }
            let record = &mut self.records[index];
            match record.phase {
                Phase::Registering => match record
                    .ticket
                    .as_mut()
                    .expect("registration ticket")
                    .take(now)
                {
                    Ok(None) => {}
                    Ok(Some(Response::Done)) => {
                        record.ticket = None;
                        record.phase = Phase::Ready;
                    }
                    _ => {
                        self.revoke(id, now)?;
                    }
                },
                Phase::Retiring => {
                    if now >= record.deadline {
                        self.ingress.invalidate();
                        self.records.clear();
                        return Err(Error::Closed);
                    }
                    if record.ticket.is_none() {
                        match self.ingress.submit(
                            Command::Retire { launch: id.0 },
                            now,
                            record.deadline,
                        ) {
                            Ok(ticket) => record.ticket = Some(ticket),
                            Err(QueueError::Capacity) => {}
                            Err(error) => {
                                self.ingress.invalidate();
                                return Err(error.into());
                            }
                        }
                    }
                    if let Some(ticket) = record.ticket.as_mut() {
                        match ticket.take(now) {
                            Ok(None) => {}
                            Ok(Some(Response::Done)) => {
                                self.records.remove(index);
                                continue;
                            }
                            _ => {
                                self.ingress.invalidate();
                                self.records.clear();
                                return Err(Error::Closed);
                            }
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        }
        Ok(())
    }

    fn record(&mut self, id: LaunchId) -> Result<&mut Record, Error> {
        self.records
            .iter_mut()
            .find(|r| r.delegation.identity.launch == id.0)
            .ok_or(Error::Stale)
    }
}

impl Drop for LaunchBook {
    fn drop(&mut self) {
        self.ingress.invalidate();
    }
}
