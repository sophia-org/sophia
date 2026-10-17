use crate::{
    Error,
    protocol::{EventRecord, decode_event, encode_ack},
};
/// Fixed receive storage. Returned labels borrow the retained head until consume.
/// The local timer starts on observed bytes; the server owns publication time.
pub struct Journal {
    epoch: u64,
    now: u64,
    timeout: u64,
    due: Option<u64>,
    offset: u64,
    bytes: [u8; 512],
    start: usize,
    end: usize,
    head: usize,
    decoded: u64,
    consumed: u64,
    acknowledged: u64,
    ack: Option<(u64, [u8; 16])>,
    error: Option<Error>,
}
impl Journal {
    pub fn new(epoch: u64, now: u64, timeout: u32) -> Result<Self, Error> {
        if epoch == 0 || !(1..=2000).contains(&timeout) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            now,
            timeout: u64::from(timeout),
            due: None,
            offset: 0,
            bytes: [0; 512],
            start: 0,
            end: 0,
            head: 0,
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
    pub fn consumed(&self) -> u64 {
        self.consumed
    }
    pub fn acknowledged(&self) -> u64 {
        self.acknowledged
    }
    pub fn read_count(&self) -> u32 {
        if self.error.is_some() || self.head != 0 {
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
        if !matches!(length, 40 | 48 | 56 | 200) {
            return Err(self.fail(Error::Protocol));
        }
        if b.len() < length {
            return Ok(());
        }
        let sequence = match decode_event(&b[..length], self.epoch) {
            Ok(event) => event.sequence,
            Err(_) => return Err(self.fail(Error::Protocol)),
        };
        if self.decoded.checked_add(1) != Some(sequence) {
            return Err(self.fail(Error::Protocol));
        }
        self.decoded = sequence;
        self.head = length;
        Ok(())
    }
    pub fn receive(&mut self, bytes: &[u8], now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.head != 0 {
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
    pub fn event(&self) -> Option<EventRecord<'_>> {
        if self.error.is_some() || self.head == 0 {
            return None;
        }
        decode_event(&self.bytes[self.start..self.start + self.head], self.epoch).ok()
    }
    pub fn consume(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.head == 0 {
            return Err(Error::Busy);
        }
        self.consumed = self.decoded;
        self.start += self.head;
        self.head = 0;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
        self.parse()
    }
    pub fn begin_ack(&mut self, now: u64) -> Result<[u8; 16], Error> {
        self.advance(now)?;
        if self.ack.is_some() || self.consumed == self.acknowledged {
            return Err(Error::Busy);
        }
        let mut bytes = [0; 16];
        encode_ack(self.epoch, self.consumed, &mut bytes)
            .map_err(|_| self.fail(Error::Protocol))?;
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
