//! Transport-lifetime bookkeeping for authenticated admission.
//!
//! This is not a verifier. Only the trusted admission owner may supply a
//! verified identity after checking Factotum's verdict and live launch custody.
//! Wire dispatch does not enable this mechanism merely by constructing it.

use crate::ConnectionId;

pub const MAX_ATTEMPTS: u8 = 8;
pub const ATTEMPT_MSEC: u64 = 5_000;
pub const FIRST_ATTACH_MSEC: u64 = 10_000;
pub const TRANSCRIPT_BYTES: usize = 4096;
pub const UNATTACHED_FIDS: usize = 4;
pub const UNATTACHED_REQUESTS: usize = 4;

/// Opaque owner-selected identities. None comes from uname, aname or n_uname.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub principal: u64,
    pub namespace: u64,
    pub launch: u64,
}

/// Exact destination of a trusted result; numeric afid reuse is insufficient.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attempt {
    pub server: u64,
    pub connection: ConnectionId,
    pub session: u64,
    pub audience: u64,
    pub wire_generation: u64,
    pub attempt_generation: u64,
    pub afid: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Busy,
    Exhausted,
    Expired,
    Stale,
    Unverified,
    IdentityChanged,
    Oversize,
    ClockReversed,
    Closed,
}

struct Current {
    attempt: Attempt,
    deadline: u64,
    bytes: usize,
    verified: Option<Identity>,
}

/// One connection's state, kept outside its Tversion-reset fid table.
/// Global/endpoint quotas and the custody check belong to the admission owner.
pub struct Authentication {
    server: u64,
    connection: ConnectionId,
    session: u64,
    audience: u64,
    first_deadline: u64,
    last_now: u64,
    wire_generation: u64,
    starts: u8,
    current: Option<Current>,
    pinned: Option<Identity>,
    closed: bool,
}

impl Authentication {
    pub fn new(
        server: u64,
        connection: ConnectionId,
        session: u64,
        audience: u64,
        now: u64,
    ) -> Result<Self, Refusal> {
        Ok(Self {
            server,
            connection,
            session,
            audience,
            first_deadline: now
                .checked_add(FIRST_ATTACH_MSEC)
                .ok_or(Refusal::Exhausted)?,
            last_now: now,
            wire_generation: 1,
            starts: 0,
            current: None,
            pinned: None,
            closed: false,
        })
    }

    pub fn pinned(&self) -> Option<Identity> {
        self.pinned
    }
    pub fn starts(&self) -> u8 {
        self.starts
    }

    fn tick(&mut self, now: u64) -> Result<(), Refusal> {
        if self.closed {
            return Err(Refusal::Closed);
        }
        if now < self.last_now {
            self.close();
            return Err(Refusal::ClockReversed);
        }
        self.last_now = now;
        if self.current.as_ref().is_some_and(|c| now >= c.deadline) {
            self.current = None;
        }
        if self.pinned.is_none() && now >= self.first_deadline {
            return Err(Refusal::Expired);
        }
        Ok(())
    }

    /// Called after reserving owner quota and an afid/reply slot. An accepted
    /// start spends the lifetime budget even if later cancelled or refused.
    pub fn begin(&mut self, afid: u32, now: u64) -> Result<Attempt, Refusal> {
        self.tick(now)?;
        if self.current.is_some() {
            return Err(Refusal::Busy);
        }
        if self.starts == MAX_ATTEMPTS {
            return Err(Refusal::Exhausted);
        }
        let mut deadline = now.checked_add(ATTEMPT_MSEC).ok_or(Refusal::Exhausted)?;
        if self.pinned.is_none() {
            deadline = deadline.min(self.first_deadline);
        }
        self.starts += 1;
        let attempt = Attempt {
            server: self.server,
            connection: self.connection,
            session: self.session,
            audience: self.audience,
            wire_generation: self.wire_generation,
            attempt_generation: u64::from(self.starts),
            afid,
        };
        self.current = Some(Current {
            attempt,
            deadline,
            bytes: 0,
            verified: None,
        });
        Ok(attempt)
    }

    /// Charges both directions, including relay framing, before forwarding.
    /// Overflow cancels this attempt without disturbing a different attempt.
    pub fn charge(&mut self, attempt: Attempt, bytes: usize, now: u64) -> Result<(), Refusal> {
        self.tick(now)?;
        let current = self
            .current
            .as_mut()
            .filter(|c| c.attempt == attempt)
            .ok_or(Refusal::Stale)?;
        let total = current
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= TRANSCRIPT_BYTES);
        let Some(total) = total else {
            self.current = None;
            return Err(Refusal::Oversize);
        };
        current.bytes = total;
        Ok(())
    }

    /// A supplied trusted verdict qualifies state-machine behavior only.
    /// The real owner must correlate its nonce, reservation and live custody
    /// before calling this; no wire request may invoke it directly.
    pub fn verified(
        &mut self,
        attempt: Attempt,
        identity: Identity,
        now: u64,
    ) -> Result<(), Refusal> {
        self.tick(now)?;
        let current = self
            .current
            .as_mut()
            .filter(|c| c.attempt == attempt)
            .ok_or(Refusal::Stale)?;
        if self.pinned.is_some_and(|p| p != identity) {
            return Err(Refusal::IdentityChanged);
        }
        if current.verified.is_some() {
            return Err(Refusal::Stale);
        }
        current.verified = Some(identity);
        Ok(())
    }

    /// Called only after the root fid, output capacity and live policy/custody
    /// are reserved/checked. On success the caller publishes the root without
    /// another fallible step. Refusals never pin a partial admission.
    pub fn commit_attach(&mut self, attempt: Attempt, now: u64) -> Result<Identity, Refusal> {
        self.tick(now)?;
        let current = self
            .current
            .as_ref()
            .filter(|c| c.attempt == attempt)
            .ok_or(Refusal::Stale)?;
        let identity = current.verified.ok_or(Refusal::Unverified)?;
        if self.pinned.is_some_and(|p| p != identity) {
            return Err(Refusal::IdentityChanged);
        }
        self.pinned = Some(identity);
        Ok(identity)
    }

    pub fn cancel(&mut self, attempt: Attempt) {
        if self.current.as_ref().is_some_and(|c| c.attempt == attempt) {
            self.current = None;
        }
    }

    /// Discards attempts/verdicts, not the transport's identity or budget.
    pub fn reset(&mut self, now: u64) -> Result<(), Refusal> {
        self.tick(now)?;
        self.current = None;
        let Some(next) = self.wire_generation.checked_add(1) else {
            self.close();
            return Err(Refusal::Exhausted);
        };
        self.wire_generation = next;
        Ok(())
    }

    /// Also used for launch revocation. A closed transport cannot be revived.
    pub fn close(&mut self) {
        self.current = None;
        self.closed = true;
    }
}
