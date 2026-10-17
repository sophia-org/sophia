use crate::{
    Error,
    protocol::{self, Event, EventRecord, Request, ResponseRow},
};
pub enum Candidate<'a> {
    Negotiate {
        minimum: u16,
        maximum: u16,
    },
    Response {
        request: &'a Request<'a>,
        rows: &'a [ResponseRow<'a>],
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Custody {
    Queued,
    Issued,
    WriteAccepted,
    Submitted,
    Refused,
    DroppedUnsent,
    Unknown,
}
/// One immutable candidate and control. A driver calls issued only after submit
/// enters the transport. Staging is not custody; EAGAIN never retries itself.
pub struct Submission {
    bytes: [u8; 448],
    length: usize,
    control: [u8; 24],
    epoch: u64,
    id: u64,
    kind: u16,
    now: u64,
    deadline: u64,
    custody: Custody,
    inflight: bool,
    receipt: Option<u64>,
    error: Option<Error>,
}
impl Submission {
    pub fn new(
        epoch: u64,
        id: u64,
        candidate: Candidate<'_>,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if deadline <= now {
            return Err(Error::Deadline);
        }
        let mut bytes = [0; 448];
        let mut control = [0; 24];
        let length = match candidate {
            Candidate::Negotiate { minimum, maximum } => {
                protocol::encode_negotiate(epoch, id, minimum, maximum, &mut bytes)
            }
            Candidate::Response { request, rows } => {
                protocol::encode_response(epoch, id, request, rows, &mut bytes)
            }
        }
        .map_err(|_| Error::Invalid)?;
        protocol::encode_submit(epoch, id, length as u32, &mut control)
            .map_err(|_| Error::Invalid)?;
        let kind = u16::from_le_bytes([bytes[6], bytes[7]]);
        Ok(Self {
            bytes,
            length,
            control,
            epoch,
            id,
            kind,
            now,
            deadline,
            custody: Custody::Queued,
            inflight: false,
            receipt: None,
            error: None,
        })
    }
    fn fail(&mut self, e: Error) -> Error {
        if self.error.is_none() {
            self.error = Some(e);
            self.custody = match self.custody {
                Custody::Queued => Custody::DroppedUnsent,
                Custody::Issued | Custody::WriteAccepted => Custody::Unknown,
                v => v,
            };
        }
        self.error.unwrap()
    }
    pub fn advance(&mut self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = self.error {
            return Err(e);
        }
        self.now = now;
        if (self.custody != Custody::Submitted || self.inflight) && now >= self.deadline {
            return Err(self.fail(Error::Deadline));
        }
        Ok(())
    }
    pub fn custody(&self) -> Custody {
        self.custody
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
    pub fn control(&self) -> &[u8; 24] {
        &self.control
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.error.is_none() && (self.custody != Custody::Submitted || self.inflight))
            .then_some(self.deadline)
    }
    pub fn issued(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.custody != Custody::Queued || self.inflight {
            return Err(Error::Busy);
        }
        self.custody = Custody::Issued;
        self.inflight = true;
        Ok(())
    }
    pub fn submit_reply(&mut self, reply: Result<u32, u32>, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if !self.inflight {
            return Err(Error::Busy);
        }
        self.inflight = false;
        match reply {
            Ok(24) => {
                if self.receipt.is_none() {
                    self.custody = Custody::WriteAccepted;
                }
                Ok(())
            }
            Err(11) if self.receipt.is_none() => {
                self.custody = Custody::Queued;
                Ok(())
            }
            Err(e) if self.receipt.is_none() => {
                self.custody = Custody::Refused;
                Err(self.fail(Error::Remote(e)))
            }
            _ => Err(self.fail(Error::Protocol)),
        }
    }
    pub fn receipt(&mut self, event: EventRecord<'_>, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if event.epoch != self.epoch
            || event.sequence == 0
            || self.receipt.is_some()
            || !matches!(self.custody, Custody::Issued | Custody::WriteAccepted)
            || event.event
                != (Event::Submitted {
                    submission: self.id,
                    candidate_kind: self.kind,
                })
        {
            return Err(self.fail(Error::Protocol));
        }
        self.receipt = Some(event.sequence);
        self.custody = Custody::Submitted;
        Ok(())
    }
    /// Supply the successfully acknowledged watermark, never local consumption.
    pub fn releasable(&self, acknowledged: u64) -> bool {
        self.error.is_none() && !self.inflight && self.receipt.is_some_and(|r| acknowledged >= r)
    }
}
