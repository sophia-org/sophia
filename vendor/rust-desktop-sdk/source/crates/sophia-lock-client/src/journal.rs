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
    Image(ImageControl),
    Negotiated(LockNegotiated),
    Refused(LockRefusal),
    Submitted(LockSubmitted),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageControl {
    Admitted(LockImageMetadata),
    Released(LockSourceReleased),
    Ready(LockChannelReady),
    Closed(LockChannelClosed),
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
    image_enabled: bool,
    minimum_revision: u16,
    maximum_revision: u16,
    selected_revision: Option<u16>,
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
            image_enabled: false,
            minimum_revision: offer.minimum_revision,
            maximum_revision: offer.maximum_revision,
            selected_revision: None,
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
    pub(crate) fn enable_image(&mut self) {
        self.image_enabled = true;
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
    /// The validated wire selection; never inferred from granted capabilities.
    pub fn selected_revision(&self) -> Option<u16> {
        self.selected_revision
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
            let revision = self.selected_revision.unwrap_or(1);
            let record = decode_lock_file_record_at_revision(
                &self.bytes[self.start..self.start + n],
                LockFileClass::Event,
                revision,
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
                    let (selected, v) = LockNegotiated::decode_selected(b)?;
                    if self.welcome.is_some()
                        || selected < self.minimum_revision
                        || selected > self.maximum_revision
                        || v.granted_capabilities & !self.capabilities != 0
                        || v.granted_chords as usize != self.chords
                        || (v.granted_chords != 0
                            && v.granted_capabilities & LOCK_FILE_CAPABILITY_CHORDS == 0)
                    {
                        return Err(Error::Invalid);
                    }
                    control(h, Control::Negotiated(v))?;
                    self.welcome = Some(v);
                    self.selected_revision = Some(selected);
                    None
                }
                LockFileKind::Refused if self.welcome.is_none() => {
                    let v = LockRefusal::decode(b)?;
                    control(h, Control::Refused(v))?;
                    self.refusal = Some((h.sequence, v));
                    None
                }
                LockFileKind::Submitted => {
                    control(
                        h,
                        Control::Submitted(LockSubmitted::decode_at_revision(b, revision)?),
                    )?;
                    None
                }
                _ if self.welcome.is_none() => return Err(Error::Invalid),
                LockFileKind::ImageAdmitted
                | LockFileKind::SourceReleased
                | LockFileKind::ChannelReady
                | LockFileKind::ChannelClosed => {
                    if !self.image_enabled
                        || revision != 2
                        || self.welcome.unwrap().granted_capabilities
                            & LOCK_FILE_CAPABILITY_SEALED_MEMORY
                            == 0
                    {
                        return Err(Error::Invalid);
                    }
                    let image = match h.kind {
                        LockFileKind::ImageAdmitted => {
                            ImageControl::Admitted(LockImageMetadata::decode(b)?)
                        }
                        LockFileKind::SourceReleased => {
                            ImageControl::Released(LockSourceReleased::decode(b)?)
                        }
                        LockFileKind::ChannelReady => {
                            ImageControl::Ready(LockChannelReady::decode(b)?)
                        }
                        LockFileKind::ChannelClosed => {
                            ImageControl::Closed(LockChannelClosed::decode(b)?)
                        }
                        _ => unreachable!(),
                    };
                    control(h, Control::Image(image))?;
                    None
                }
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
