//! Passive correlation for one request. A matching Allowed is necessary but
//! does not establish an implemented executor or permit an upload by itself.
use super::Error;
use crate::protocol::requester::{Event, Grant, Outcome, TransferRequest};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Pending,
    Denied,
    Allowed,
    EndIssued,
    Completed,
    Revoked,
    Expired,
}
pub struct Transfer {
    expected: Grant,
    state: State,
    closed: bool,
}
impl Transfer {
    pub fn new(request: TransferRequest<'_>, broker_generation: u64) -> Result<Self, Error> {
        if broker_generation == 0
            || request.source == 0
            || request.target == 0
            || request.source == request.target
            || request.mime.is_some_and(|m| m.len() > 255)
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            expected: Grant {
                transfer: request.transfer,
                kind: request.kind,
                source: request.source,
                target: request.target,
                source_generation: request.generation,
                broker_generation,
                deadline_msec: request.deadline_msec,
            },
            state: State::Pending,
            closed: false,
        })
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn disconnected(&self) -> bool {
        self.closed
    }
    pub fn id(&self) -> u64 {
        self.expected.transfer
    }
    /// A disconnected or terminal transfer never exposes a usable grant.
    pub fn grant(&self) -> Option<Grant> {
        (!self.closed && matches!(self.state, State::Allowed | State::EndIssued))
            .then_some(self.expected)
    }
    /// Envelope epoch/sequence and the submission receipt must already have
    /// been checked by the session. Failure leaves the observation unchanged.
    pub fn decision(&mut self, event: Event) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.state != State::Pending {
            return Err(Error::Protocol);
        }
        let state = match event {
            Event::Denied { transfer } if transfer == self.expected.transfer => State::Denied,
            Event::Allowed(grant)
                if grant == self.expected
                    && grant.transfer != 0
                    && grant.source_generation != 0
                    && grant.deadline_msec != 0 =>
            {
                State::Allowed
            }
            _ => return Err(Error::Protocol),
        };
        self.state = state;
        Ok(())
    }
    /// Called when a validated, fully uploaded End enters the transport.
    /// This records possible execution custody, not proof of completion.
    pub fn end_issued(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.state != State::Allowed {
            return Err(Error::Busy);
        }
        self.state = State::EndIssued;
        Ok(())
    }
    pub fn outcome(&mut self, transfer: u64, outcome: Outcome) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if transfer != self.expected.transfer
            || !matches!(self.state, State::Allowed | State::EndIssued)
        {
            return Err(Error::Protocol);
        }
        let state = match outcome {
            Outcome::Completed if self.state == State::EndIssued => State::Completed,
            Outcome::Completed => return Err(Error::Protocol),
            Outcome::Revoked => State::Revoked,
            Outcome::Expired => State::Expired,
        };
        self.state = state;
        Ok(())
    }
    /// Retain the last known observation. Disconnect cannot manufacture a
    /// Revoked/Completed event; EndIssued remains execution-uncertain.
    pub fn close(&mut self) {
        self.closed = true;
    }
}
