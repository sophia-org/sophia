//! Session-attested launch authentication, separate from 9front's rpc protocols.
//!
//! Only the private Session owner may register delegations or attest which
//! provisioned channel relayed a nonce. This owner does not inspect that channel
//! or prove process custody. Session must recheck live custody before attach.

use sophia_9p::authentication::{ATTEMPT_MSEC, Attempt, Identity};

pub mod client;
pub mod control;
pub mod queue;
pub mod relay;
pub mod wire;

pub const MAX_DELEGATIONS: usize = 64;
pub const MAX_ATTEMPTS: usize = 32;
pub const MAX_LAUNCH_ATTEMPTS: usize = 2;
pub const MAX_PRINCIPAL_ATTEMPTS: usize = 8;

/// Owner-selected facts, never decoded from the launch child's conversation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Delegation {
    pub identity: Identity,
    pub audience: u64,
}

/// No Debug implementation: challenges do not belong in durable logs.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Nonce(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Verdict {
    pub attempt: Attempt,
    pub identity: Identity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Invalid,
    Stale,
    Busy,
    Capacity,
    Reserved,
    Audience,
    RandomUnavailable,
    ClockReversed,
    Closed,
}

struct Registered {
    delegation: Delegation,
    active: bool,
}

struct Pending {
    attempt: Attempt,
    deadline: u64,
    nonce: Option<Nonce>,
    verdict: Option<Identity>,
}

/// One trusted Session control connection's bounded state. A fresh owner is
/// required after that connection closes. Times are monotonic milliseconds.
pub struct LaunchAuthority {
    session: u64,
    last_now: u64,
    last_launch: u64,
    closed: bool,
    delegations: Vec<Registered>,
    attempts: Vec<Pending>,
}

impl LaunchAuthority {
    pub fn new(session: u64, now: u64) -> Result<Self, Refusal> {
        if session == 0 {
            return Err(Refusal::Invalid);
        }
        Ok(Self {
            session,
            last_now: now,
            last_launch: 0,
            closed: false,
            delegations: Vec::with_capacity(MAX_DELEGATIONS),
            attempts: Vec::with_capacity(MAX_ATTEMPTS),
        })
    }

    pub fn expire(&mut self, now: u64) -> Result<(), Refusal> {
        if self.closed {
            return Err(Refusal::Closed);
        }
        if now < self.last_now {
            self.close();
            return Err(Refusal::ClockReversed);
        }
        self.last_now = now;
        // A verified but undelivered result keeps its original deadline.
        self.attempts.retain(|p| now < p.deadline);
        Ok(())
    }

    /// Registers a reservation before spawn. Launch generations increase over
    /// this Session's lifetime and cannot be reused after rollback/retirement.
    pub fn reserve(&mut self, delegation: Delegation, now: u64) -> Result<(), Refusal> {
        self.expire(now)?;
        let identity = delegation.identity;
        if identity.principal == 0 || identity.namespace == 0 || delegation.audience == 0 {
            return Err(Refusal::Invalid);
        }
        if identity.launch <= self.last_launch {
            return Err(Refusal::Stale);
        }
        if self.delegations.len() == MAX_DELEGATIONS {
            return Err(Refusal::Capacity);
        }
        self.last_launch = identity.launch;
        self.delegations.push(Registered {
            delegation,
            active: false,
        });
        Ok(())
    }

    /// Session calls this only for the reservation whose spawn and protection
    /// confirmation succeeded. The child cannot activate its own channel.
    pub fn activate(&mut self, launch: u64, now: u64) -> Result<(), Refusal> {
        self.expire(now)?;
        let entry = self
            .delegations
            .iter_mut()
            .find(|r| r.delegation.identity.launch == launch)
            .ok_or(Refusal::Stale)?;
        if entry.active {
            return Err(Refusal::Stale);
        }
        entry.active = true;
        Ok(())
    }

    pub fn retire(&mut self, launch: u64, now: u64) -> Result<(), Refusal> {
        self.expire(now)?;
        self.delegations
            .retain(|r| r.delegation.identity.launch != launch);
        self.attempts
            .retain(|p| p.verdict.is_none_or(|v| v.launch != launch));
        Ok(())
    }

    /// The admission owner has already reserved its endpoint/transport quota.
    /// `deadline` is the original attempt deadline, clipped to first attach.
    pub fn begin(&mut self, attempt: Attempt, deadline: u64, now: u64) -> Result<Nonce, Refusal> {
        self.expire(now)?;
        if attempt.session != self.session
            || attempt.server == 0
            || attempt.connection.0 == 0
            || attempt.audience == 0
            || attempt.wire_generation == 0
            || attempt.attempt_generation == 0
            || attempt.afid == u32::MAX
            || deadline <= now
            || deadline - now > ATTEMPT_MSEC
        {
            return Err(Refusal::Invalid);
        }
        if self.attempts.iter().any(|p| {
            p.attempt.server == attempt.server && p.attempt.connection == attempt.connection
        }) {
            return Err(Refusal::Busy);
        }
        if self.attempts.len() == MAX_ATTEMPTS {
            return Err(Refusal::Capacity);
        }
        let mut bytes = [0; 32];
        // Never wait for entropy in the owner loop; short reads also refuse.
        let filled = rustix::rand::getrandom(&mut bytes, rustix::rand::GetRandomFlags::NONBLOCK)
            .map_err(|_| Refusal::RandomUnavailable)?;
        if filled != bytes.len() {
            return Err(Refusal::RandomUnavailable);
        }
        let nonce = Nonce(bytes);
        if self.attempts.iter().any(|p| p.nonce == Some(nonce)) {
            return Err(Refusal::RandomUnavailable);
        }
        self.attempts.push(Pending {
            attempt,
            deadline,
            nonce: Some(nonce),
            verdict: None,
        });
        Ok(nonce)
    }

    /// `launch` comes from Session's channel registration, never the child.
    /// Wrong-channel and quota refusals do not spend the valid challenge.
    pub fn relay(&mut self, launch: u64, nonce: Nonce, now: u64) -> Result<(), Refusal> {
        self.expire(now)?;
        let registered = self
            .delegations
            .iter()
            .find(|r| r.delegation.identity.launch == launch)
            .ok_or(Refusal::Stale)?;
        let index = self
            .attempts
            .iter()
            .position(|p| p.nonce == Some(nonce))
            .ok_or(Refusal::Stale)?;
        if registered.delegation.audience != self.attempts[index].attempt.audience {
            return Err(Refusal::Audience);
        }
        if !registered.active {
            return Err(Refusal::Reserved);
        }
        let identity = registered.delegation.identity;
        if self
            .attempts
            .iter()
            .filter(|p| p.verdict.is_some_and(|v| v.launch == launch))
            .count()
            >= MAX_LAUNCH_ATTEMPTS
            || self
                .attempts
                .iter()
                .filter(|p| p.verdict.is_some_and(|v| v.principal == identity.principal))
                .count()
                >= MAX_PRINCIPAL_ATTEMPTS
        {
            return Err(Refusal::Capacity);
        }
        self.attempts[index].nonce = None;
        self.attempts[index].verdict = Some(identity);
        Ok(())
    }

    /// Consumes one correlated result. Session keeps its own reservation until
    /// this reply is consumed or cancellation is acknowledged. It must recheck
    /// live launch custody before installing this verdict into an afid.
    pub fn take_verdict(&mut self, attempt: Attempt, now: u64) -> Result<Option<Verdict>, Refusal> {
        self.expire(now)?;
        let index = self
            .attempts
            .iter()
            .position(|p| p.attempt == attempt)
            .ok_or(Refusal::Stale)?;
        let Some(identity) = self.attempts[index].verdict else {
            return Ok(None);
        };
        if !self
            .delegations
            .iter()
            .any(|r| r.active && r.delegation.identity == identity)
        {
            self.attempts.remove(index);
            return Err(Refusal::Stale);
        }
        self.attempts.remove(index);
        Ok(Some(Verdict { attempt, identity }))
    }

    pub fn cancel(&mut self, attempt: Attempt, now: u64) -> Result<(), Refusal> {
        self.expire(now)?;
        self.attempts.retain(|p| p.attempt != attempt);
        Ok(())
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.attempts.clear();
        self.delegations.clear();
    }
}
