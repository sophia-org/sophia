use super::Error;
use crate::protocol::requester::{EventRecord, decode_event, encode_ack};
/// Fixed journal buffer and one retained application event. Consuming an event
/// makes it eligible for ack; only the exact ack reply advances acknowledged.
pub struct Journal {
    epoch: u64,
    now: u64,
    timeout: u64,
    due: Option<u64>,
    offset: u64,
    bytes: [u8; 512],
    start: usize,
    end: usize,
    head: Option<EventRecord>,
    decoded: u64,
    consumed: u64,
    acknowledged: u64,
    ack: Option<(u64, [u8; 16])>,
    error: Option<Error>,
}
impl Journal {
    pub fn new(epoch: u64, now: u64, ack_timeout_ms: u32) -> Result<Self, Error> {
        if epoch == 0 || !(1..=2000).contains(&ack_timeout_ms) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            now,
            timeout: u64::from(ack_timeout_ms),
            due: None,
            offset: 0,
            bytes: [0; 512],
            start: 0,
            end: 0,
            head: None,
            decoded: 0,
            consumed: 0,
            acknowledged: 0,
            ack: None,
            error: None,
        })
    }
    fn fail(&mut self, e: Error) -> Error {
        *self.error.get_or_insert(e)
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    pub fn advance(&mut self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = self.error {
            return Err(e);
        }
        self.now = now;
        if self.due.is_some_and(|d| now >= d) {
            return Err(self.fail(Error::Deadline));
        }
        Ok(())
    }
    fn arm(&mut self) -> Result<(), Error> {
        self.due = Some(
            self.now
                .checked_add(self.timeout)
                .ok_or_else(|| self.fail(Error::Overflow))?,
        );
        Ok(())
    }
    pub fn deadline(&self) -> Option<u64> {
        if self.error.is_none() { self.due } else { None }
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    /// Maximum next Rread payload. Do not read while an application head is held.
    pub fn read_count(&self) -> u32 {
        if self.error.is_some() || self.head.is_some() {
            0
        } else {
            (512 - (self.end - self.start)) as u32
        }
    }
    fn parse(&mut self) -> Result<(), Error> {
        let b = &self.bytes[self.start..self.end];
        if b.len() < 4 {
            return Ok(());
        }
        let length = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
        if !(40..=88).contains(&length) {
            return Err(self.fail(Error::Protocol));
        }
        if b.len() < length {
            return Ok(());
        }
        let event = decode_event(&b[..length]).map_err(|_| self.fail(Error::Protocol))?;
        if event.epoch != self.epoch || self.decoded.checked_add(1) != Some(event.sequence) {
            return Err(self.fail(Error::Protocol));
        }
        self.decoded = event.sequence;
        self.head = Some(event);
        Ok(())
    }
    /// Accept one nonempty reply at offset(). Handles partial and coalesced
    /// records without a per-event allocation or a copy on every consume.
    pub fn receive(&mut self, bytes: &[u8], now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.head.is_some() {
            return Err(Error::Busy);
        }
        if bytes.is_empty() || bytes.len() > self.read_count() as usize {
            return Err(self.fail(Error::Protocol));
        }
        let offset = self
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| self.fail(Error::Overflow))?;
        if self.due.is_none() {
            self.arm()?;
        }
        if self.end + bytes.len() > 512 {
            self.bytes.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        self.bytes[self.end..self.end + bytes.len()].copy_from_slice(bytes);
        self.end += bytes.len();
        self.offset = offset;
        self.parse()
    }
    pub fn event(&self) -> Option<EventRecord> {
        if self.error.is_none() {
            self.head
        } else {
            None
        }
    }
    pub fn consume(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        let head = self.head.take().ok_or(Error::Busy)?;
        self.consumed = head.sequence;
        self.start +=
            u32::from_le_bytes(self.bytes[self.start..self.start + 4].try_into().unwrap()) as usize;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
        self.parse()
    }
    pub fn consumed(&self) -> u64 {
        self.consumed
    }
    pub fn acknowledged(&self) -> u64 {
        self.acknowledged
    }
    /// Freezes this cumulative ack. Later consumption does not change its bytes.
    pub fn begin_ack(&mut self, now: u64) -> Result<[u8; 16], Error> {
        self.advance(now)?;
        if self.ack.is_some() || self.consumed == self.acknowledged {
            return Err(Error::Busy);
        }
        let bytes =
            encode_ack(self.epoch, self.consumed).map_err(|_| self.fail(Error::Protocol))?;
        self.ack = Some((self.consumed, bytes));
        Ok(bytes)
    }
    pub fn ack_bytes(&self) -> Option<&[u8; 16]> {
        self.ack.as_ref().map(|(_, b)| b)
    }
    pub fn ack_reply(&mut self, reply: Result<u32, u32>, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        let (sequence, _) = self.ack.ok_or(Error::Busy)?;
        match reply {
            Ok(16) => (),
            Ok(_) => return Err(self.fail(Error::Protocol)),
            Err(e) => return Err(self.fail(Error::Remote(e))),
        }
        self.acknowledged = sequence;
        self.ack = None;
        if self.start != self.end || self.consumed > sequence {
            self.arm()?;
        } else {
            self.due = None;
        }
        Ok(())
    }
}
