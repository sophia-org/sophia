//! A retained journal head with fixed storage and caller-driven progress.
use sophia_lock_protocol::{BinaryCodecError, lock_files::*};

pub const JOURNAL_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    ObjectPublished(LockObjectPublished),
    ResourceStatus(LockResourceStatus),
    ResourceReleased(LockResourceReleased),
    CandidateOutcome(LockCandidateOutcome),
    FramePermit(LockFramePermit),
    Entry(LockEntry),
    Chord(LockChord),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Negotiated(LockNegotiated),
    Refused(LockRefusal),
    Submitted(LockSubmitted),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Overflow,
    Empty,
}
impl From<BinaryCodecError> for Error {
    fn from(_: BinaryCodecError) -> Self {
        Self::Invalid
    }
}

pub struct Journal {
    epoch: u64,
    capabilities: u64,
    chords: usize,
    welcome: Option<LockNegotiated>,
    refusal: Option<(u64, LockRefusal)>,
    bytes: [u8; JOURNAL_BYTES],
    start: usize,
    end: usize,
    offset: u64,
    sequence: u64,
    consumed: u64,
    generation: u64,
    head: Option<(LockFileHeader, Event, usize)>,
    verified: bool,
    fault: Option<Error>,
}
impl Journal {
    pub fn new(epoch: u64, offer: &LockNegotiate) -> Result<Self, Error> {
        if epoch == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            capabilities: offer.requested_capabilities,
            chords: offer.chords.len(),
            welcome: None,
            refusal: None,
            bytes: [0; JOURNAL_BYTES],
            start: 0,
            end: 0,
            offset: 0,
            sequence: 0,
            consumed: 0,
            generation: 0,
            head: None,
            verified: false,
            fault: None,
        })
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn consumed(&self) -> u64 {
        self.consumed
    }
    pub fn welcome(&self) -> Option<LockNegotiated> {
        self.welcome
    }
    pub fn refusal(&self) -> Option<(u64, LockRefusal)> {
        self.refusal
    }
    pub fn fault(&self) -> Option<Error> {
        self.fault
    }
    pub fn capacity(&self) -> usize {
        if self.fault.is_some() {
            0
        } else {
            JOURNAL_BYTES - (self.end - self.start)
        }
    }
    /// Append without decoding behind a retained application head. Compact only
    /// when the tail cannot hold the incoming bytes, not after every event.
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if let Some(e) = self.fault {
            return Err(e);
        }
        let result = self.append_inner(bytes);
        if let Err(e) = result {
            self.fault = Some(e);
        }
        result
    }
    fn append_inner(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.is_empty() {
            return Err(Error::Empty);
        }
        if bytes.len() > self.capacity() {
            return Err(Error::Overflow);
        }
        let offset = self
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or(Error::Overflow)?;
        if bytes.len() > JOURNAL_BYTES - self.end {
            self.bytes.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        self.bytes[self.end..self.end + bytes.len()].copy_from_slice(bytes);
        self.end += bytes.len();
        self.offset = offset;
        Ok(())
    }
    /// Process only contiguous internal controls, stopping at the first public
    /// event or publication. The callback validates custody against the issued
    /// candidate. A failure preserves earlier controls and forbids further use.
    pub fn advance(
        &mut self,
        mut control: impl FnMut(LockFileHeader, Control) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if let Some(e) = self.fault {
            return Err(e);
        }
        let result = self.advance_inner(&mut control);
        if let Err(e) = result {
            self.fault = Some(e);
        }
        result
    }
    fn advance_inner(
        &mut self,
        control: &mut impl FnMut(LockFileHeader, Control) -> Result<(), Error>,
    ) -> Result<(), Error> {
        while self.head.is_none() && self.end - self.start >= 4 {
            let n = u32::from_le_bytes(self.bytes[self.start..self.start + 4].try_into().unwrap())
                as usize;
            if !(LOCK_FILE_HEADER_BYTES..=JOURNAL_BYTES).contains(&n) {
                return Err(Error::Invalid);
            }
            if n > self.end - self.start {
                break;
            }
            let record = decode_lock_file_record(
                &self.bytes[self.start..self.start + n],
                LockFileClass::Event,
            )?;
            let h = record.header;
            let b = record.body;
            if h.connection_epoch != self.epoch
                || h.sequence <= self.sequence
                || self.refusal.is_some()
            {
                return Err(Error::Invalid);
            }
            let event = match h.kind {
                LockFileKind::Negotiated => {
                    let v = LockNegotiated::decode(b)?;
                    if self.welcome.is_some()
                        || v.granted_capabilities & !self.capabilities != 0
                        || v.granted_chords as usize != self.chords
                        || (v.granted_chords != 0
                            && v.granted_capabilities & LOCK_FILE_CAPABILITY_CHORDS == 0)
                    {
                        return Err(Error::Invalid);
                    }
                    control(h, Control::Negotiated(v))?;
                    self.welcome = Some(v);
                    None
                }
                LockFileKind::Refused if self.welcome.is_none() => {
                    let v = LockRefusal::decode(b)?;
                    control(h, Control::Refused(v))?;
                    self.refusal = Some((h.sequence, v));
                    None
                }
                LockFileKind::Submitted => {
                    control(h, Control::Submitted(LockSubmitted::decode(b)?))?;
                    None
                }
                _ if self.welcome.is_none() => return Err(Error::Invalid),
                LockFileKind::ObjectPublished => {
                    let v = LockObjectPublished::decode(b)?;
                    if v.object_generation <= self.generation {
                        return Err(Error::Invalid);
                    }
                    Some(Event::ObjectPublished(v))
                }
                LockFileKind::ResourceStatus => {
                    Some(Event::ResourceStatus(LockResourceStatus::decode(b)?))
                }
                LockFileKind::ResourceReleased => {
                    Some(Event::ResourceReleased(LockResourceReleased::decode(b)?))
                }
                LockFileKind::CandidateOutcome => {
                    Some(Event::CandidateOutcome(LockCandidateOutcome::decode(b)?))
                }
                LockFileKind::FramePermit => Some(Event::FramePermit(LockFramePermit::decode(b)?)),
                LockFileKind::Entry => Some(Event::Entry(LockEntry::decode(b)?)),
                LockFileKind::Chord => {
                    let v = LockChord::decode(b)?;
                    if v.chord >= self.welcome.unwrap().granted_chords {
                        return Err(Error::Invalid);
                    }
                    Some(Event::Chord(v))
                }
                _ => return Err(Error::Invalid),
            };
            self.sequence = h.sequence;
            if let Some(event) = event {
                self.head = Some((h, event, n));
            } else {
                self.drop_record(n, h.sequence);
            }
        }
        Ok(())
    }
    pub fn pending_publication(&self) -> Option<(u64, LockObjectPublished)> {
        if self.fault.is_some() || self.verified {
            return None;
        }
        match self.head? {
            (h, Event::ObjectPublished(v), _) => Some((h.sequence, v)),
            _ => None,
        }
    }
    /// The object driver calls this after clunk. A newer Qid supersedes the
    /// announcement: consume it internally without publishing a stale object.
    pub fn publication_resolved(
        &mut self,
        sequence: u64,
        value: LockObjectPublished,
        superseded: bool,
    ) -> Result<(), Error> {
        if self.pending_publication() != Some((sequence, value)) {
            return Err(Error::Invalid);
        }
        if superseded {
            let (_, _, n) = self.head.take().unwrap();
            self.drop_record(n, sequence);
        } else {
            self.generation = value.object_generation;
            self.verified = true;
        }
        Ok(())
    }
    pub fn head(&self) -> Option<(LockFileHeader, &Event)> {
        if self.fault.is_some() || self.pending_publication().is_some() {
            return None;
        }
        self.head.as_ref().map(|(h, e, _)| (*h, e))
    }
    /// The driver calls advance afterwards, with its custody validator.
    pub fn consume(&mut self) -> Result<u64, Error> {
        if let Some(e) = self.fault {
            return Err(e);
        }
        if self.head().is_none() {
            return Err(Error::Empty);
        }
        let (h, _, n) = self.head.take().unwrap();
        self.verified = false;
        self.drop_record(n, h.sequence);
        Ok(self.consumed)
    }
    fn drop_record(&mut self, n: usize, sequence: u64) {
        self.start += n;
        self.consumed = sequence;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
    }
}
