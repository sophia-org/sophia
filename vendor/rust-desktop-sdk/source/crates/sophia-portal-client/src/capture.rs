//! One capture request, without transport or transfer authority. The driver owns
//! serial sequencing, matches wire replies and closes on terminal errors. Never
//! replay an issued request; local cancellation cannot cancel server work.
use crate::protocol::{
    CAPTURE_BYTES, CaptureRequest, CaptureState, decode_capture_status, encode_capture,
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
    bytes: [u8; CAPTURE_BYTES],
    request: CaptureRequest,
    now: u64,
    deadline: u64,
    poll_at: u64,
    status_inflight: bool,
    custody: Custody,
    outcome: Option<CaptureState>,
    error: Option<Error>,
}
impl Pending {
    /// One caller-monotonic deadline, at most ten seconds away. The driver must
    /// allocate increasing serials for the recipient owner, including reconnects.
    /// Construction neither authenticates nor performs I/O; storage is inline.
    pub fn new(request: CaptureRequest, now: u64, deadline: u64) -> Result<Self, Error> {
        if deadline <= now || deadline - now > 10_000 {
            return Err(Error::Deadline);
        }
        let bytes = encode_capture(request).map_err(|_| Error::Invalid)?;
        Ok(Self {
            bytes,
            request,
            now,
            deadline,
            poll_at: now,
            status_inflight: false,
            custody: Custody::Queued,
            outcome: None,
            error: None,
        })
    }
    /// Submit these bytes as one write at offset zero on write-only `capture`.
    /// Refuse an msize or nonzero iounit too small for this complete record.
    pub fn bytes(&self) -> &[u8; CAPTURE_BYTES] {
        &self.bytes
    }
    pub fn custody(&self) -> Custody {
        self.custody
    }
    pub fn outcome(&self) -> Option<CaptureState> {
        self.outcome
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn next_wake(&self) -> Option<u64> {
        if self.error.is_some() || self.custody == Custody::Settled {
            return None;
        }
        Some(
            if self.custody == Custody::Accepted && !self.status_inflight {
                self.poll_at.min(self.deadline)
            } else {
                self.deadline
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
        if self.custody != Custody::Settled && now >= self.deadline {
            return Err(self.fail(Error::Deadline));
        }
        Ok(())
    }
    /// Call only after the pipeline admits the complete write, not on EAGAIN.
    pub fn issued(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.custody != Custody::Queued {
            return Err(Error::Busy);
        }
        self.custody = Custody::Issued;
        Ok(())
    }
    /// Exact Rwrite means admission only; complete Rlerror means refusal. A
    /// short or lost reply leaves custody uncertain and never permits replay.
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
            Ok(n) if n as usize == CAPTURE_BYTES => {
                self.custody = Custody::Accepted;
                self.poll_at = now;
                Ok(())
            }
            _ => Err(self.fail(Error::Protocol)),
        }
    }
    /// Start a fresh immutable capture-status snapshot when due. Its EOF and
    /// clunk share the original deadline; partial progress never extends it.
    pub fn start_status(&mut self, now: u64) -> Result<u64, Error> {
        self.advance(now)?;
        if self.custody != Custody::Accepted || self.status_inflight || now < self.poll_at {
            return Err(Error::Busy);
        }
        self.status_inflight = true;
        Ok(self.deadline)
    }
    /// Supply only an EOF/clunk-verified snapshot. Ready is historical status,
    /// not a frame grant; frame reads still check their retained object lease.
    pub fn status(&mut self, bytes: &[u8], now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.custody != Custody::Accepted || !self.status_inflight {
            return Err(self.fail(Error::Protocol));
        }
        let status = decode_capture_status(bytes).map_err(|_| self.fail(Error::Protocol))?;
        if status.serial != self.request.serial || status.output != self.request.output {
            return Err(self.fail(Error::Protocol));
        }
        self.status_inflight = false;
        match status.state {
            CaptureState::Queued | CaptureState::Capturing => {
                self.poll_at = now.saturating_add(1);
            }
            CaptureState::Idle => return Err(self.fail(Error::Protocol)),
            outcome => {
                self.outcome = Some(outcome);
                self.custody = Custody::Settled;
            }
        }
        Ok(())
    }
}
