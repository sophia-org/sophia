//! One role transport's afid conversation. The export keeps this outside its
//! wire-reset fid table, supplies exact claims and reserves the root first.
//! This owner never turns a wire-provided success flag into authentication.
use super::LaunchBook;
use sophia_9p::{
    ConnectionId, Errno, ReadOutcome,
    authentication::{Attempt, Authentication, Identity},
};
use sophia_factotum::launch::{
    Verdict,
    queue::{Error, Ingress, Ticket},
    relay::encode_challenge,
    wire::{Command, Response},
};
use sophia_runtime::ProcessSupervisor;
use std::time::{Duration, Instant};

mod lock;

pub struct RoleScope {
    pub server: u64,
    pub connection: ConnectionId,
    pub session: u64,
    pub audience: u64,
    pub uname: Vec<u8>,
    pub aname: Vec<u8>,
}
struct Conversation {
    attempt: Attempt,
    deadline: Instant,
    ticket: Option<Ticket>,
    challenge: Option<[u8; 40]>,
    verdict: Option<Verdict>,
    qualified: bool,
}
struct Cancellation {
    attempt: Attempt,
    deadline: Instant,
    ticket: Option<Ticket>,
}

/// The transport owner must call poll even without peer traffic, close a
/// transport whose owner returns ESTALE, and retain its slot until drained().
/// Endpoint/global transport admission is deliberately outside this object.
pub struct RoleAuthentication {
    scope: RoleScope,
    state: Authentication,
    ingress: Ingress,
    origin: Instant,
    last_now: Instant,
    conversation: Option<Conversation>,
    cancellation: Option<Cancellation>,
    admitted: Option<Verdict>,
    closed: bool,
}
impl RoleAuthentication {
    pub fn new(scope: RoleScope, ingress: Ingress, now: Instant) -> Result<Self, Errno> {
        if scope.server == 0
            || scope.session == 0
            || scope.audience == 0
            || scope.uname.len() > 255
            || scope.aname.len() > 255
        {
            return Err(Errno::EINVAL);
        }
        let state = Authentication::new(
            scope.server,
            scope.connection,
            scope.session,
            scope.audience,
            0,
        )
        .map_err(|_| Errno::EINVAL)?;
        Ok(Self {
            scope,
            state,
            ingress,
            origin: now,
            last_now: now,
            conversation: None,
            cancellation: None,
            admitted: None,
            closed: false,
        })
    }
    fn clock(&self, now: Instant) -> Result<u64, Errno> {
        now.checked_duration_since(self.origin)
            .and_then(|d| d.as_millis().try_into().ok())
            .ok_or(Errno::ESTALE)
    }
    pub fn claims(
        &self,
        connection: ConnectionId,
        uname: &[u8],
        aname: &[u8],
        n_uname: u32,
    ) -> Result<(), Errno> {
        if connection != self.scope.connection
            || uname != self.scope.uname
            || aname != self.scope.aname
            || n_uname != u32::MAX
        {
            Err(Errno::EACCES)
        } else {
            Ok(())
        }
    }
    pub fn pinned(&self) -> Option<Identity> {
        self.state.pinned()
    }
    pub fn drained(&self) -> bool {
        self.conversation.is_none() && self.cancellation.is_none()
    }

    pub fn begin(&mut self, afid: u32, now: Instant) -> Result<Attempt, Errno> {
        self.poll(now)?;
        if self.cancellation.is_some() {
            return Err(Errno::EAGAIN);
        }
        let attempt = self
            .state
            .begin(afid, self.clock(now)?)
            .map_err(|_| Errno::EACCES)?;
        let mut deadline = now + Duration::from_secs(5);
        if self.state.pinned().is_none() {
            deadline = deadline.min(self.origin + Duration::from_secs(10));
        }
        let command = Command::Begin {
            attempt,
            remaining_msec: deadline.saturating_duration_since(now).as_millis() as u32,
        };
        let ticket = match self.ingress.submit(command, now, deadline) {
            Ok(ticket) => ticket,
            Err(error) => {
                self.state.cancel(attempt);
                return Err(queue_error(error));
            }
        };
        self.conversation = Some(Conversation {
            attempt,
            deadline,
            ticket: Some(ticket),
            challenge: None,
            verdict: None,
            qualified: false,
        });
        Ok(attempt)
    }

    /// Offset zero rereads the fixed 40-byte challenge. Offset 40 reads the
    /// eight-byte status after the client has relayed the nonce on its channel.
    /// Only whole records are returned; the transcript bound includes rereads.
    pub fn read(
        &mut self,
        attempt: Attempt,
        offset: u64,
        count: u32,
        now: Instant,
        book: &mut LaunchBook,
        supervisor: &mut ProcessSupervisor,
    ) -> Result<ReadOutcome, Errno> {
        self.poll(now)?;
        let bytes = match offset {
            0 if count >= 40 => 40,
            40 if count >= 8 => 8,
            _ => return Err(Errno::EINVAL),
        };
        let current = self
            .conversation
            .as_mut()
            .filter(|c| c.attempt == attempt)
            .ok_or(Errno::ESTALE)?;
        if current.challenge.is_none() {
            return Ok(ReadOutcome::Pending);
        }
        let output = if offset == 0 {
            current.challenge.expect("challenge ready").to_vec()
        } else {
            if current.verdict.is_none() {
                if current.ticket.is_none() {
                    current.ticket = Some(
                        self.ingress
                            .submit(Command::Take { attempt }, now, current.deadline)
                            .map_err(queue_error)?,
                    );
                }
                return Ok(ReadOutcome::Pending);
            }
            self.qualify(attempt, now, book, supervisor)?;
            [1, 0, 0, 0, 0, 0, 0, 0].to_vec()
        };
        if self.state.charge(attempt, bytes, self.clock(now)?).is_err() {
            self.release(attempt, now);
            return Err(Errno::ESTALE);
        }
        Ok(ReadOutcome::Ready(output))
    }

    /// Call only after claims, scope policy, root-fid and reply capacity checks.
    /// The caller publishes its prepared root with no fallible step after this.
    pub fn commit_attach(
        &mut self,
        attempt: Attempt,
        now: Instant,
        book: &mut LaunchBook,
        supervisor: &mut ProcessSupervisor,
    ) -> Result<Identity, Errno> {
        self.poll(now)?;
        let verdict = self.qualify(attempt, now, book, supervisor)?;
        let identity = self
            .state
            .commit_attach(attempt, self.clock(now)?)
            .map_err(|_| Errno::EACCES)?;
        self.admitted = Some(verdict);
        Ok(identity)
    }

    fn qualify(
        &mut self,
        attempt: Attempt,
        now: Instant,
        book: &mut LaunchBook,
        supervisor: &mut ProcessSupervisor,
    ) -> Result<Verdict, Errno> {
        let clock = self.clock(now)?;
        let current = self
            .conversation
            .as_mut()
            .filter(|c| c.attempt == attempt)
            .ok_or(Errno::ESTALE)?;
        let verdict = current.verdict.ok_or(Errno::EACCES)?;
        book.authorize(verdict, supervisor, now)
            .map_err(|_| Errno::ESTALE)?;
        if !current.qualified {
            self.state
                .verified(attempt, verdict.identity, clock)
                .map_err(|_| Errno::EACCES)?;
            current.qualified = true;
        }
        Ok(verdict)
    }

    pub fn check_admitted(
        &mut self,
        now: Instant,
        book: &mut LaunchBook,
        supervisor: &mut ProcessSupervisor,
    ) -> Result<Identity, Errno> {
        self.poll(now)?;
        let verdict = self.admitted.ok_or(Errno::EACCES)?;
        book.authorize(verdict, supervisor, now)
            .map_err(|_| Errno::ESTALE)
    }

    pub fn release(&mut self, attempt: Attempt, now: Instant) {
        if self
            .conversation
            .as_ref()
            .is_some_and(|c| c.attempt == attempt)
        {
            self.conversation = None;
            self.state.cancel(attempt);
            self.cancellation = Some(Cancellation {
                attempt,
                deadline: now + Duration::from_secs(5),
                ticket: None,
            });
        }
    }
    pub fn reset(&mut self, now: Instant) -> Result<(), Errno> {
        if let Some(c) = &self.conversation {
            self.release(c.attempt, now);
        }
        self.state
            .reset(self.clock(now)?)
            .map_err(|_| Errno::ESTALE)
    }
    pub fn close(&mut self, now: Instant) {
        if let Some(c) = &self.conversation {
            self.release(c.attempt, now);
        }
        self.state.close();
        self.closed = true;
    }

    pub fn poll(&mut self, now: Instant) -> Result<(), Errno> {
        if now < self.last_now {
            self.close(self.last_now);
            return Err(Errno::ESTALE);
        }
        self.last_now = now;
        if !self.ingress.is_open() {
            self.close(now);
            self.cancellation = None;
            return Err(Errno::ESTALE);
        }
        if let Some(cancel) = self.cancellation.as_mut() {
            if now >= cancel.deadline {
                // Never report cleanup complete while an uncertain remote
                // attempt can survive. The shared owner closes the actual
                // transport on its next turn, retiring all its registrations.
                self.ingress.invalidate();
                self.cancellation = None;
                self.close(now);
                return Err(Errno::ESTALE);
            } else {
                if cancel.ticket.is_none() {
                    match self.ingress.submit(
                        Command::Cancel {
                            attempt: cancel.attempt,
                        },
                        now,
                        cancel.deadline,
                    ) {
                        Ok(ticket) => cancel.ticket = Some(ticket),
                        Err(Error::Capacity) => {}
                        Err(_) => return Err(Errno::ESTALE),
                    }
                }
                if let Some(ticket) = cancel.ticket.as_mut() {
                    match ticket.take(now) {
                        Ok(None) => {}
                        Ok(Some(Response::Done)) => self.cancellation = None,
                        _ => return Err(Errno::ESTALE),
                    }
                }
            }
        }
        if self.closed {
            return Err(Errno::ESTALE);
        }
        if self.pinned().is_none() && now >= self.origin + Duration::from_secs(10) {
            self.close(now);
            return Err(Errno::ESTALE);
        }
        if self
            .conversation
            .as_ref()
            .is_some_and(|c| now >= c.deadline)
        {
            let attempt = self.conversation.as_ref().unwrap().attempt;
            self.release(attempt, now);
        }
        let Some(c) = self.conversation.as_mut() else {
            return Ok(());
        };
        let Some(ticket) = c.ticket.as_mut() else {
            return Ok(());
        };
        let response = match ticket.take(now) {
            Ok(None) => return Ok(()),
            Ok(Some(response)) => response,
            Err(error) => {
                let attempt = c.attempt;
                self.release(attempt, now);
                return Err(queue_error(error));
            }
        };
        c.ticket = None;
        match response {
            Response::Challenge(nonce) if c.challenge.is_none() => {
                c.challenge = Some(encode_challenge(nonce));
                // The relay's fixed request and response are also part of the
                // attempt transcript, although they use a separate transport.
                let attempt = c.attempt;
                self.state
                    .charge(attempt, 48, self.clock(now)?)
                    .map_err(|_| Errno::ESTALE)?;
            }
            Response::Pending if c.challenge.is_some() => {}
            Response::Verified(verdict)
                if c.challenge.is_some() && verdict.attempt == c.attempt =>
            {
                c.verdict = Some(verdict);
            }
            _ => {
                let attempt = c.attempt;
                self.release(attempt, now);
                return Err(Errno::EIO);
            }
        }
        Ok(())
    }
}
fn queue_error(error: Error) -> Errno {
    match error {
        Error::Capacity => Errno::EAGAIN,
        Error::Remote(errno) => errno,
        Error::Deadline | Error::Closed => Errno::ESTALE,
    }
}
