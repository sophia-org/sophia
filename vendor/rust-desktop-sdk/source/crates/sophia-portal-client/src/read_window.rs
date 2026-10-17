//! Bounded frame range scheduling, separate from transport and grant authority.
use crate::protocol::IMAGE_MAX_BYTES;
pub const READ_WINDOW: usize = 4;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    pub token: u64,
    pub offset: u64,
    pub count: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Closed,
    Exhausted,
}
#[derive(Clone, Copy, Default)]
struct Slot {
    token: u64,
    offset: u64,
    count: u32,
    pending: bool,
}
pub struct ReadWindow {
    slots: [Slot; READ_WINDOW],
    length: u64,
    next: u64,
    received: u64,
    serial: u64,
    chunk: u32,
    error: Option<Error>,
}
impl ReadWindow {
    /// Count respects reply framing, open iounit and the caller's chunk budget.
    /// No allocation or I/O. Use only with one retained frame fid.
    pub fn new(length: u64, msize: u32, iounit: u32, chunk: u32) -> Result<Self, Error> {
        if length == 0 || length > IMAGE_MAX_BYTES || !(512..=65536).contains(&msize) || chunk == 0
        {
            return Err(Error::Invalid);
        }
        let chunk = chunk
            .min(msize - 11)
            .min(if iounit == 0 { u32::MAX } else { iounit });
        Ok(Self {
            slots: [Slot::default(); READ_WINDOW],
            length,
            next: 0,
            received: 0,
            serial: 0,
            chunk,
            error: None,
        })
    }
    /// Reserves one range. If transport admission is busy, retain this request
    /// and submit it once; do not ask for a replacement or reuse its token.
    pub fn reserve(&mut self) -> Result<Option<Request>, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|s| !s.pending && (s.count != 0 || self.next < self.length))
        else {
            return Ok(None);
        };
        let Some(serial) = self.serial.checked_add(1) else {
            self.error = Some(Error::Exhausted);
            return Err(Error::Exhausted);
        };
        if slot.count == 0 {
            slot.offset = self.next;
            slot.count = self.chunk.min((self.length - self.next) as u32);
            self.next += u64::from(slot.count);
        }
        self.serial = serial;
        slot.token = serial;
        slot.pending = true;
        Ok(Some(Request {
            token: serial,
            offset: slot.offset,
            count: slot.count,
        }))
    }
    /// Validate a matched Rread before placing its borrowed bytes at the
    /// reserved offset. Short reads schedule only the unread suffix, never a
    /// replay. Empty/past-end/duplicate replies fail this window permanently.
    pub fn accept(&mut self, token: u64, length: usize) -> Result<u64, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|s| s.pending && s.token == token)
        else {
            self.error = Some(Error::Invalid);
            return Err(Error::Invalid);
        };
        if length == 0 || length > slot.count as usize {
            self.error = Some(Error::Invalid);
            return Err(Error::Invalid);
        }
        let offset = slot.offset;
        slot.offset += length as u64;
        slot.count -= length as u32;
        slot.pending = false;
        self.received += length as u64;
        Ok(offset)
    }
    pub fn received(&self) -> u64 {
        self.received
    }
    /// Only the expected payload has arrived. Caller still verifies EOF, the
    /// retained metadata identity and clunk before declaring a complete image.
    pub fn payload_complete(&self) -> bool {
        self.error.is_none() && self.received == self.length
    }
    /// Call on wire errors, revocation, timeout or cancellation; do not replay.
    pub fn close(&mut self) {
        self.error.get_or_insert(Error::Closed);
    }
}
