//! One immutable administrative invocation. The session owns request sequencing
//! and wire tags; it must close on errors and never replay this command.
use crate::protocol::{
    Catalog, Command, Info, Invocation, Outcome, Owner, Status, decode_status, encode_invocation,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Custody {
    Queued,
    Issued,
    Accepted,
    Settled,
    Refused,
    DroppedUnsent,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
    ClockRegression,
    Deadline,
    Protocol,
    Refused(u32),
    Closed,
}

pub struct Pending {
    bytes: [u8; 512],
    length: usize,
    connection: u64,
    request: u64,
    generation: u64,
    owner: Owner,
    reload: bool,
    frame: u64,
    now: u64,
    deadline: u64,
    operation_deadline: u64,
    poll_at: u64,
    status_inflight: bool,
    custody: Custody,
    error: Option<Error>,
    outcome: Option<Outcome>,
    detail: [u8; 256],
    detail_length: usize,
}
impl Pending {
    pub fn new(
        info: Info,
        catalog: &Catalog<'_>,
        command: Command<'_>,
        request: u64,
        now: u64,
    ) -> Result<Self, Error> {
        if !catalog.contains(command) {
            return Err(Error::Invalid);
        }
        Self::selected(info, catalog.generation(), command, request, now)
    }
    pub(crate) fn selected(
        info: Info,
        generation: u64,
        command: Command<'_>,
        request: u64,
        now: u64,
    ) -> Result<Self, Error> {
        if info.connection == 0
            || request == 0
            || generation == 0
            || info.command_ms == 0
            || info.command_ms > 10000
            || info.frame_ms == 0
            || info.frame_ms > 2000
        {
            return Err(Error::Invalid);
        }
        let deadline = now
            .checked_add(u64::from(info.command_ms) + u64::from(info.frame_ms))
            .ok_or(Error::Invalid)?;
        let mut bytes = [0; 512];
        let length = encode_invocation(
            Invocation {
                connection: info.connection,
                request,
                generation,
                owner: command.owner,
                name: command.name,
            },
            &mut bytes,
        )
        .map_err(|_| Error::Invalid)?;
        Ok(Self {
            bytes,
            length,
            connection: info.connection,
            request,
            generation,
            owner: command.owner,
            reload: command.name == "reload-profile",
            frame: u64::from(info.frame_ms),
            now,
            deadline,
            operation_deadline: deadline,
            poll_at: now,
            status_inflight: false,
            custody: Custody::Queued,
            error: None,
            outcome: None,
            detail: [0; 256],
            detail_length: 0,
        })
    }
    pub fn custody(&self) -> Custody {
        self.custody
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn request(&self) -> u64 {
        self.request
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
    pub fn outcome(&self) -> Option<(Outcome, &str)> {
        self.outcome.map(|v| {
            (
                v,
                std::str::from_utf8(&self.detail[..self.detail_length]).expect("validated detail"),
            )
        })
    }
    pub fn invalidate_catalog(&self) -> bool {
        self.outcome.is_some_and(|v| {
            self.owner == Owner::Session
                || matches!(v, Outcome::Stale | Outcome::Denied | Outcome::Indeterminate)
        })
    }
    pub fn next_wake(&self) -> Option<u64> {
        if self.error.is_some() || self.custody == Custody::Settled {
            return None;
        }
        Some(
            if self.custody == Custody::Accepted && !self.status_inflight {
                self.poll_at.min(self.deadline)
            } else {
                self.operation_deadline.min(self.deadline)
            },
        )
    }
    fn fail(&mut self, error: Error) -> Error {
        if self.error.is_none() {
            self.error = Some(error);
            self.custody = match self.custody {
                Custody::Queued => Custody::DroppedUnsent,
                Custody::Issued | Custody::Accepted => Custody::Unknown,
                other => other,
            };
            self.status_inflight = false;
        }
        self.error.unwrap()
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    pub fn advance(&mut self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(error) = self.error {
            return Err(error);
        }
        self.now = now;
        if self.custody != Custody::Settled
            && (now >= self.deadline || now >= self.operation_deadline)
        {
            return Err(self.fail(Error::Deadline));
        }
        Ok(())
    }
    /// Mark issued only after the pipeline admits the complete ctl write.
    pub fn issued(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.custody != Custody::Queued {
            return Err(Error::Busy);
        }
        self.operation_deadline = now.saturating_add(self.frame).min(self.deadline);
        self.custody = Custody::Issued;
        Ok(())
    }
    /// A complete Rlerror proves nonacceptance; short/lost replies do not.
    pub fn write_reply(&mut self, reply: Result<u32, u32>, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.custody != Custody::Issued {
            return Err(self.fail(Error::Protocol));
        }
        match reply {
            Err(errno) => {
                self.custody = Custody::Refused;
                Err(self.fail(Error::Refused(errno)))
            }
            Ok(n) if n as usize == self.length => {
                self.custody = Custody::Accepted;
                self.poll_at = now;
                self.operation_deadline = self.deadline;
                Ok(())
            }
            _ => Err(self.fail(Error::Protocol)),
        }
    }
    /// Start a new immutable status snapshot only when due; returns its absolute
    /// deadline. No idle polling and no deadline extension from partial progress.
    pub fn start_status(&mut self, now: u64) -> Result<u64, Error> {
        self.advance(now)?;
        if self.custody != Custody::Accepted || self.status_inflight || now < self.poll_at {
            return Err(Error::Busy);
        }
        self.status_inflight = true;
        self.operation_deadline = now.saturating_add(self.frame).min(self.deadline);
        Ok(self.operation_deadline)
    }
    /// Supply only a complete EOF/clunk-verified status snapshot.
    pub fn status(&mut self, bytes: &[u8], now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.custody != Custody::Accepted || !self.status_inflight {
            return Err(self.fail(Error::Protocol));
        }
        let status = decode_status(bytes).map_err(|_| self.fail(Error::Protocol))?;
        match status {
            Status::Pending {
                connection,
                request,
                generation,
            } if (connection, request, generation)
                == (self.connection, self.request, self.generation) =>
            {
                self.status_inflight = false;
                self.operation_deadline = self.deadline;
                self.poll_at = now.saturating_add(1);
                Ok(())
            }
            Status::Outcome {
                connection,
                request,
                generation,
                outcome,
                detail,
            } if (connection, request, generation)
                == (self.connection, self.request, self.generation) =>
            {
                let allowed = match outcome {
                    Outcome::Committed => self.owner == Owner::Policy,
                    Outcome::Completed => self.owner == Owner::Session,
                    Outcome::Unchanged => self.owner == Owner::Session && self.reload,
                    _ => true,
                };
                if !allowed {
                    return Err(self.fail(Error::Protocol));
                }
                self.detail[..detail.len()].copy_from_slice(detail.as_bytes());
                self.detail_length = detail.len();
                self.outcome = Some(outcome);
                self.custody = Custody::Settled;
                self.status_inflight = false;
                Ok(())
            }
            _ => Err(self.fail(Error::Protocol)),
        }
    }
}
