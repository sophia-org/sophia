//! Ordered, bounded WM journal assembly. Transport controls are validated
//! behind an application-held head, but acknowledgements never skip that head.
use sophia_wm_protocol::{wm_files::*, *};
use std::collections::VecDeque;

// Largest Cycle: header, prefix, sixteen output IDs and the largest cause.
pub const EVENT_BYTES: usize = 32 + 48 + 16 * 8 + 64;
pub const JOURNAL_BYTES: usize = 64 * EVENT_BYTES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    ProfilePrepare(PolicyProfileCommand),
    ProfileActivate(PolicyProfileCommand),
    ProfileRollback(PolicyProfileCommand),
    ConfigurationOutcome(WmFileConfigurationOutcome),
    Cycle(WmFileCycle),
    ProjectionOutcome(WmFileProjectionOutcome),
    SessionOperationOutcome(WmFileSessionOperationOutcome),
    PresentationReceipt(WmFilePresentationReceipt),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Negotiated(u64),
    Submitted(WmFileSubmitted),
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
    header: WmFileHeader,
    bytes: usize,
    event: Option<Event>,
}

pub struct Journal {
    epoch: u64,
    limits: WmFileLimits,
    offer: WmFileNegotiationOffer,
    selected: Option<u64>,
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
        limits: WmFileLimits,
        offer: WmFileNegotiationOffer,
        now: u64,
    ) -> Result<Self, Error> {
        if epoch == 0 || offer.required_capabilities & offer.optional_capabilities != 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            limits,
            offer,
            selected: None,
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
        mut control: impl FnMut(WmFileHeader, Control) -> Result<(), Error>,
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
        control: &mut impl FnMut(WmFileHeader, Control) -> Result<(), Error>,
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
            self.deadline = Some(now.saturating_add(u64::from(WM_FILE_SEND_TIMEOUT_MILLIS)));
        }
        self.fragment.extend_from_slice(bytes);
        let mut parsed = 0;
        while self.fragment.len() - parsed >= 4 {
            let n =
                u32::from_le_bytes(self.fragment[parsed..parsed + 4].try_into().unwrap()) as usize;
            if !(WM_FILE_HEADER_BYTES..=EVENT_BYTES).contains(&n) {
                return Err(Error::Invalid);
            }
            if n > self.fragment.len() - parsed {
                break;
            }
            if self.entries.len() == 64 {
                return Err(Error::Overflow);
            }
            let bytes = &self.fragment[parsed..parsed + n];
            let header = decode_wm_file_record(bytes, WmFileClass::Event)
                .map_err(|_| Error::Invalid)?
                .header;
            if header.connection_epoch != self.epoch || header.sequence <= self.sequence {
                return Err(Error::Invalid);
            }
            let event = match header.kind {
                WmFileKind::Negotiated => {
                    let caps = decode_wm_file_negotiated(bytes).map_err(|_| Error::Invalid)?;
                    if self.selected.is_some() || !valid_selection(caps, self.limits, self.offer) {
                        return Err(Error::Invalid);
                    }
                    control(header, Control::Negotiated(caps))?;
                    self.selected = Some(caps);
                    None
                }
                WmFileKind::Submitted => {
                    let value = decode_wm_file_submitted(bytes).map_err(|_| Error::Invalid)?;
                    control(header, Control::Submitted(value))?;
                    None
                }
                _ => {
                    let caps = self.selected.ok_or(Error::Invalid)?;
                    Some(decode_event(bytes, header.kind, caps).map_err(|_| Error::Invalid)?)
                }
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
            self.deadline = Some(now.saturating_add(u64::from(WM_FILE_SEND_TIMEOUT_MILLIS)));
        }
        self.skip_controls();
        Ok(())
    }

    pub fn head(&self) -> Option<(WmFileHeader, &Event)> {
        if self.fault.is_some() {
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

fn valid_selection(caps: u64, limits: WmFileLimits, offer: WmFileNegotiationOffer) -> bool {
    if caps & !(offer.required_capabilities | offer.optional_capabilities) != 0
        || caps & !limits.capability_ceiling != 0
        || caps & offer.required_capabilities != offer.required_capabilities
        || (limits.profile_required && caps & SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION == 0)
    {
        return false;
    }
    let dependencies = [
        (
            SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
            SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES,
        ),
        (
            SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT,
            SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
        ),
        (
            SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE,
            SOPHIA_WM_CAPABILITY_ACTIONS | SOPHIA_WM_CAPABILITY_CONFIGURATION,
        ),
        (
            SOPHIA_WM_CAPABILITY_CHORD_ACTIONS,
            SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE
                | SOPHIA_WM_CAPABILITY_ACTIONS
                | SOPHIA_WM_CAPABILITY_CONFIGURATION,
        ),
        (
            SOPHIA_WM_CAPABILITY_HELD_CAPTURE,
            SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        ),
    ];
    dependencies
        .into_iter()
        .all(|(bit, required)| caps & bit == 0 || caps & required == required)
}

fn decode_event(bytes: &[u8], kind: WmFileKind, caps: u64) -> Result<Event, WmFilePayloadError> {
    Ok(match kind {
        WmFileKind::ProfilePrepare => {
            Event::ProfilePrepare(decode_wm_file_profile_command(bytes, kind, caps)?)
        }
        WmFileKind::ProfileActivate => {
            Event::ProfileActivate(decode_wm_file_profile_command(bytes, kind, caps)?)
        }
        WmFileKind::ProfileRollback => {
            Event::ProfileRollback(decode_wm_file_profile_command(bytes, kind, caps)?)
        }
        WmFileKind::ConfigurationOutcome => {
            Event::ConfigurationOutcome(decode_wm_file_configuration_outcome(bytes, caps)?)
        }
        WmFileKind::Cycle => Event::Cycle(decode_wm_file_cycle(bytes, caps)?),
        WmFileKind::ProjectionOutcome => {
            Event::ProjectionOutcome(decode_wm_file_projection_outcome(bytes, caps)?)
        }
        WmFileKind::SessionOperationOutcome => {
            Event::SessionOperationOutcome(decode_wm_file_session_operation_outcome(bytes, caps)?)
        }
        WmFileKind::PresentationReceipt => {
            Event::PresentationReceipt(decode_wm_file_presentation_receipt(bytes, caps)?)
        }
        _ => return Err(WmFilePayloadError::Value),
    })
}
