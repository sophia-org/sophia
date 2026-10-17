//! Semantic ordering for the session driver. Journal validates wire sequences;
//! transaction validates custody. Neither check substitutes for this ordering.
use crate::{
    Error,
    protocol::{Event, EventRecord, Limits},
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Negotiating,
    Ready,
    Responding,
    Refused,
}
pub struct Ordering {
    epoch: u64,
    limits: Limits,
    phase: Phase,
    expected: Option<(u64, u16)>,
    handshake_receipt: bool,
    last_request: u64,
    pending: Option<u64>,
    refusal_sequence: Option<u64>,
    refusal_due: Option<u64>,
    now: u64,
    error: Option<Error>,
}
impl Ordering {
    pub fn new(epoch: u64, limits: Limits, negotiation_id: u64, now: u64) -> Result<Self, Error> {
        if epoch == 0 || negotiation_id == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            limits,
            phase: Phase::Negotiating,
            expected: Some((negotiation_id, 256)),
            handshake_receipt: false,
            last_request: 0,
            pending: None,
            refusal_sequence: None,
            refusal_due: None,
            now,
            error: None,
        })
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn pending(&self) -> Option<u64> {
        self.pending
    }
    pub fn refusal_deadline(&self) -> Option<u64> {
        self.refusal_due
    }
    pub fn refusal_acked(&self, ack: u64) -> bool {
        self.refusal_sequence.is_some_and(|seq| ack >= seq)
    }
    fn fail(&mut self, e: Error) -> Error {
        *self.error.get_or_insert(e)
    }
    pub fn advance(&mut self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = self.error {
            return Err(e);
        }
        self.now = now;
        if self.refusal_due.is_some_and(|due| now >= due) {
            return Err(self.fail(Error::Deadline));
        }
        Ok(())
    }
    /// Register only after response validation and admission to the session.
    pub fn response(&mut self, request: u64, submission: u64, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if submission == 0 {
            return Err(Error::Invalid);
        }
        if self.phase != Phase::Ready || self.pending != Some(request) || self.expected.is_some() {
            return Err(Error::Busy);
        }
        self.expected = Some((submission, 257));
        self.phase = Phase::Responding;
        Ok(())
    }
    /// Called after the journal and transaction have validated the event.
    pub fn event(&mut self, record: EventRecord<'_>, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if record.epoch != self.epoch || record.sequence == 0 {
            return Err(self.fail(Error::Protocol));
        }
        match record.event {
            Event::Submitted {
                submission,
                candidate_kind,
            } if self.expected == Some((submission, candidate_kind)) => {
                self.expected = None;
                if self.phase == Phase::Negotiating {
                    self.handshake_receipt = true;
                } else if self.phase == Phase::Responding {
                    self.pending = None;
                    self.phase = Phase::Ready;
                } else {
                    return Err(self.fail(Error::Protocol));
                }
            }
            Event::Negotiated {
                max_surfaces,
                max_label_bytes,
            } if self.phase == Phase::Negotiating
                && self.handshake_receipt
                && max_surfaces == self.limits.max_surfaces
                && max_label_bytes == self.limits.max_label_bytes =>
            {
                self.phase = Phase::Ready;
            }
            Event::Refused if self.phase == Phase::Negotiating && self.handshake_receipt => {
                let due = now
                    .checked_add(u64::from(self.limits.ack_progress_timeout_ms))
                    .ok_or_else(|| self.fail(Error::Overflow))?;
                self.phase = Phase::Refused;
                self.refusal_sequence = Some(record.sequence);
                self.refusal_due = Some(due);
            }
            Event::Request(request)
                if self.phase == Phase::Ready
                    && self.pending.is_none()
                    && request.id > self.last_request =>
            {
                self.last_request = request.id;
                self.pending = Some(request.id);
            }
            _ => return Err(self.fail(Error::Protocol)),
        }
        Ok(())
    }
}
