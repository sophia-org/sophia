//! Ordered, bounded output journal assembly. Transport controls are validated
//! behind an application-held head, but acknowledgements never skip that head.
use sophia_output_protocol::{output_files::*, *};
use std::collections::VecDeque;

pub const EVENT_BYTES: usize = 32 + 24;
pub const JOURNAL_BYTES: usize = 64 * EVENT_BYTES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    ObjectPublished(OutputFilePublication),
    Outcome(TransactionId, OutputV1Outcome),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Negotiated(OutputV1ServerWelcome),
    Refused(OutputFileRefusal),
    Submitted(OutputFileSubmitted),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Overflow,
    Deadline,
    ClockRegression,
    Empty,
}

struct Entry {
    header: OutputFileHeader,
    bytes: usize,
    event: Option<Event>,
}

pub struct Journal {
    epoch: u64,
    limits: OutputFileLimits,
    offer: OutputV1ClientHello,
    selected: Option<u64>,
    refusal: Option<(u64, OutputFileRefusal)>,
    verified_publication: Option<u64>,
    entries: VecDeque<Entry>,
    fragment: Vec<u8>,
    retained_bytes: usize,
    offset: u64,
    sequence: u64,
    consumed: u64,
    now: u64,
    deadline: Option<u64>,
    fault: Option<Error>,
}

impl Journal {
    pub fn new(
        epoch: u64,
        limits: OutputFileLimits,
        offer: OutputV1ClientHello,
        now: u64,
    ) -> Result<Self, Error> {
        if epoch == 0 || limits.validate().is_err() {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            limits,
            offer,
            selected: None,
            refusal: None,
            verified_publication: None,
            entries: VecDeque::with_capacity(64),
            fragment: Vec::with_capacity(EVENT_BYTES),
            retained_bytes: 0,
            offset: 0,
            sequence: 0,
            consumed: 0,
            now,
            deadline: None,
            fault: None,
        })
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn consumed(&self) -> u64 {
        self.consumed
    }
    pub fn selected(&self) -> Option<u64> {
        self.selected
    }
    pub fn fragment_deadline(&self) -> Option<u64> {
        self.deadline
    }
    pub fn capacity(&self) -> usize {
        if self.fault.is_some() {
            0
        } else {
            JOURNAL_BYTES - self.retained_bytes - self.fragment.len()
        }
    }
    pub fn fault(&self) -> Option<Error> {
        self.fault
    }

    /// The callback validates transport controls against the current issued
    /// candidate. Run this before interpreting submit replies in the same I/O
    /// batch. Successful earlier controls survive a later malformed record;
    /// nothing beyond the malformed record is scanned or delivered.
    pub fn append(
        &mut self,
        bytes: &[u8],
        now: u64,
        mut control: impl FnMut(OutputFileHeader, Control) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if let Some(error) = self.fault {
            return Err(error);
        }
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        self.now = now;
        let result = self.append_inner(bytes, now, &mut control);
        if let Err(error) = result {
            self.fault = Some(error);
        }
        result
    }

    fn append_inner(
        &mut self,
        bytes: &[u8],
        now: u64,
        control: &mut impl FnMut(OutputFileHeader, Control) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            return Err(Error::Deadline);
        }
        if bytes.is_empty() {
            return Err(Error::Empty);
        }
        if bytes.len() > self.capacity() {
            return Err(Error::Overflow);
        }
        self.offset = self
            .offset
            .checked_add(bytes.len() as u64)
            .ok_or(Error::Overflow)?;
        if self.fragment.is_empty() {
            self.deadline =
                Some(now.saturating_add(u64::from(self.limits.ack_progress_timeout_millis)));
        }
        self.fragment.extend_from_slice(bytes);
        let mut parsed = 0;
        while self.fragment.len() - parsed >= 4 {
            let n =
                u32::from_le_bytes(self.fragment[parsed..parsed + 4].try_into().unwrap()) as usize;
            if !(OUTPUT_FILE_HEADER_BYTES..=EVENT_BYTES).contains(&n) {
                return Err(Error::Invalid);
            }
            if n > self.fragment.len() - parsed {
                break;
            }
            if self.entries.len() == 64 {
                return Err(Error::Overflow);
            }
            let bytes = &self.fragment[parsed..parsed + n];
            let record = decode_output_file_record(bytes, OutputFileClass::Event)
                .map_err(|_| Error::Invalid)?;
            let header = record.header;
            let body = record.body;
            if header.connection_epoch != self.epoch
                || header.sequence <= self.sequence
                || self.refusal.is_some()
            {
                return Err(Error::Invalid);
            }
            let event = match header.kind {
                OutputFileKind::Negotiated => {
                    let welcome = decode_output_file_negotiated(body, self.epoch)
                        .map_err(|_| Error::Invalid)?;
                    if self.selected.is_some()
                        || welcome.capabilities & !self.offer.capabilities != 0
                        || self.offer.minimum_revision > welcome.selected_revision
                        || self.offer.maximum_revision < welcome.selected_revision
                    {
                        return Err(Error::Invalid);
                    }
                    control(header, Control::Negotiated(welcome))?;
                    self.selected = Some(welcome.capabilities);
                    None
                }
                OutputFileKind::Refused => {
                    if self.selected.is_some() {
                        return Err(Error::Invalid);
                    }
                    let refusal = decode_output_file_refused(body).map_err(|_| Error::Invalid)?;
                    control(header, Control::Refused(refusal))?;
                    self.refusal = Some((header.sequence, refusal));
                    None
                }
                OutputFileKind::Submitted => {
                    let value = decode_output_file_submitted(body).map_err(|_| Error::Invalid)?;
                    control(header, Control::Submitted(value))?;
                    None
                }
                OutputFileKind::ObjectPublished if self.selected.is_some() => {
                    Some(Event::ObjectPublished(
                        decode_output_file_publication(body).map_err(|_| Error::Invalid)?,
                    ))
                }
                OutputFileKind::Outcome if self.selected.is_some() => {
                    let (transaction, outcome) =
                        decode_output_file_outcome(body, self.epoch).map_err(|_| Error::Invalid)?;
                    Some(Event::Outcome(transaction, outcome))
                }
                _ => return Err(Error::Invalid),
            };
            self.entries.push_back(Entry {
                header,
                bytes: n,
                event,
            });
            self.retained_bytes += n;
            self.sequence = header.sequence;
            parsed += n;
        }
        self.fragment.drain(..parsed);
        if self.fragment.is_empty() {
            self.deadline = None;
        } else if parsed != 0 {
            self.deadline =
                Some(now.saturating_add(u64::from(self.limits.ack_progress_timeout_millis)));
        }
        self.skip_controls();
        Ok(())
    }

    /// The driver must settle this sequence's ACK before ending in Refused.
    pub fn refusal(&self) -> Option<(u64, OutputFileRefusal)> {
        self.refusal
    }

    /// A pending publication blocks application delivery and cumulative ACKs.
    /// Its exact Qid and topology epoch must be verified through EOF and clunk.
    pub fn pending_publication(&self) -> Option<(u64, OutputFilePublication)> {
        if self.fault.is_some() {
            return None;
        }
        let entry = self.entries.front()?;
        match entry.event.as_ref()? {
            Event::ObjectPublished(value)
                if self.verified_publication != Some(entry.header.sequence) =>
            {
                Some((entry.header.sequence, *value))
            }
            _ => None,
        }
    }

    /// Called only by the object driver after complete identity validation.
    pub fn publication_verified(
        &mut self,
        sequence: u64,
        value: OutputFilePublication,
    ) -> Result<(), Error> {
        if self.pending_publication() != Some((sequence, value)) {
            return Err(Error::Invalid);
        }
        self.verified_publication = Some(sequence);
        Ok(())
    }

    pub fn head(&self) -> Option<(OutputFileHeader, &Event)> {
        if self.fault.is_some() || self.pending_publication().is_some() {
            return None;
        }
        self.entries
            .front()
            .and_then(|entry| entry.event.as_ref().map(|event| (entry.header, event)))
    }

    /// Consumes only the retained application head, then any contiguous
    /// transport controls. The returned watermark is eligible for an ACK.
    pub fn consume(&mut self) -> Result<u64, Error> {
        if let Some(error) = self.fault {
            return Err(error);
        }
        if self.head().is_none() {
            return Err(Error::Empty);
        }
        self.remove_head();
        self.skip_controls();
        Ok(self.consumed)
    }
    fn remove_head(&mut self) {
        if let Some(entry) = self.entries.pop_front() {
            self.verified_publication = None;
            self.retained_bytes -= entry.bytes;
            self.consumed = entry.header.sequence;
        }
    }
    fn skip_controls(&mut self) {
        while self
            .entries
            .front()
            .is_some_and(|entry| entry.event.is_none())
        {
            self.remove_head();
        }
    }
}
