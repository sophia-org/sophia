//! Inbound event dispatch: turning one whole `events` record into the same
//! typed `Inbound` the socket wire produces, and fetching the snapshot object
//! an `ObjectPublished` announces.
//!
//! Acknowledgement never passes an announcement whose object has not been
//! fetched and verified. Each feed keeps a hold at the earliest such
//! announcement's bound until its newest announcement is fetched: an older
//! announcement already superseded in the buffered events is not fetched at
//! all (opening pins the current object, which would not match it), and a
//! fetched object whose qid differs from its announcement is newer still, so
//! the hold stays until that newer announcement is handled. Events are
//! handled strictly in order; a fetch in flight holds later events back.
use std::collections::VecDeque;

use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Tag};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{
    SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG, SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
    SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION, SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG,
    SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS, ShellCatalogActionRecord, ShellContentRecord,
    TransactionId,
};

use super::{FEEDS, FetchPhase, FileWire, MAX_FETCH_RESTARTS, O_RDONLY, ObjectFetch, feed_index};
use crate::ShellClientError;
use crate::wire::Inbound;

/// The largest event record the journal can hold, and so the most event
/// bytes the client ever buffers.
pub(super) const MAX_EVENT_BYTES: usize = SHELL_FILE_MAX_JOURNAL_BYTES as usize;

/// One decoded event, before it is applied.
enum Event {
    Submitted(u64, ShellFileKind),
    Published(ShellFileObjectPublished, usize),
    ResourceStatus(sophia_shell_protocol::ContentResourceId, u16, Inbound),
    Inbound(Inbound),
}

/// The whole record at the front of `bytes`, if one is complete.
fn complete_record(bytes: &[u8]) -> Option<usize> {
    let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    (size >= SHELL_FILE_HEADER_BYTES && bytes.len() >= size).then_some(size)
}

/// Refuses a record length no event can have, so a malformed length can
/// never leave bytes accumulating behind it.
fn check_record_length(bytes: &[u8]) -> Result<(), ShellClientError> {
    let Some(prefix) = bytes.get(..4) else {
        return Ok(());
    };
    let size = u32::from_le_bytes(prefix.try_into().unwrap()) as usize;
    if !(SHELL_FILE_HEADER_BYTES..=MAX_EVENT_BYTES).contains(&size) {
        return Err(ShellClientError::Protocol("event record length"));
    }
    Ok(())
}

impl FileWire {
    /// The ack bound: every handled event, but never past a held
    /// announcement.
    pub(super) fn ack_limit(&self) -> u64 {
        self.holds
            .iter()
            .flatten()
            .fold(self.ack_ready, |limit, hold| limit.min(*hold))
    }

    /// Parses and dispatches one whole event out of the buffered `events`
    /// bytes, if a complete record is present and (for one that becomes an
    /// `Inbound`) the inbox has room. Events are processed strictly in
    /// arrival order: while an object fetch is in flight, later buffered
    /// events wait, preserving the socket wire's own ordering.
    pub(super) fn process_buffered_event(
        &mut self,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<bool, ShellClientError> {
        if let Err(error) = check_record_length(&self.event_buf) {
            self.event_fault = true;
            return Err(error);
        }
        if self.object_fetch.is_some() {
            return Ok(false);
        }
        let Some(size) = complete_record(&self.event_buf) else {
            return Ok(false);
        };
        let header =
            decode_shell_file_record(&self.event_buf[..size], ShellFileClass::Event)?.header;
        let needs_inbox_room = !matches!(
            header.kind,
            ShellFileKind::Submitted | ShellFileKind::ObjectPublished
        );
        if needs_inbox_room && inbox.len() >= crate::MAX_QUEUED_FRAMES {
            return Ok(false);
        }
        let record: Vec<u8> = self.event_buf.drain(..size).collect();
        if let Err(error) = self.handle_event(record, inbox) {
            self.event_fault = true;
            return Err(error);
        }
        self.progress += 1;
        Ok(true)
    }

    /// The connection is over while an object fetch holds later events back:
    /// look past it only to observe custody. Each buffered record gets the
    /// same length, epoch and rising-sequence checks as ever, and the walk
    /// stops at the first that fails; nothing else is delivered.
    pub(super) fn observe_custody_after_close(&mut self) {
        if self.event_fault {
            return;
        }
        for (submission, kind) in self.buffered_submissions() {
            if self.on_submitted(submission, kind).is_err() {
                return;
            }
        }
    }

    /// Whether the negotiated capabilities disclose `feed` at all.
    fn feed_disclosed(&self, feed: ShellFileKind) -> bool {
        if matches!(
            feed,
            ShellFileKind::Descriptors | ShellFileKind::Tabs | ShellFileKind::Shortcuts
        ) {
            return self.require_descriptor_kind(feed).is_ok();
        }
        let needed = match feed {
            ShellFileKind::Outputs => SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            // The file wire's catalog object is the r8 persistent catalog.
            ShellFileKind::Catalog if self.descriptor => {
                SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            }
            ShellFileKind::Catalog => SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG,
            ShellFileKind::Indicators => SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS,
            _ => return false,
        };
        self.capabilities & needed != 0
    }

    /// Whether a later same-feed announcement is already buffered.
    fn superseded(&self, feed: ShellFileKind) -> bool {
        let mut rest = &self.event_buf[..];
        while let Some(size) = complete_record(rest) {
            let record = &rest[..size];
            if let Ok(parsed) = decode_shell_file_record(record, ShellFileClass::Event)
                && parsed.header.kind == ShellFileKind::ObjectPublished
                && let Ok(published) = decode_shell_file_object_published(record)
                && published.object == feed
            {
                return true;
            }
            rest = &rest[size..];
        }
        false
    }

    /// Decodes one whole event completely: envelope, attach epoch, kind,
    /// typed body, and for an announcement that its feed is disclosed. Both
    /// normal intake and the close scan use it, so neither accepts what the
    /// other would refuse. It changes nothing.
    fn decode_event(&self, record: &[u8]) -> Result<(u64, Event), ShellClientError> {
        let header = decode_shell_file_record(record, ShellFileClass::Event)?.header;
        if header.connection_epoch != self.epoch {
            return Err(ShellClientError::Protocol(
                "event from another connection epoch",
            ));
        }
        if self.descriptor {
            if matches!(
                header.kind,
                ShellFileKind::AllocationResult
                    | ShellFileKind::ResourceStatus
                    | ShellFileKind::ResourceReleased
                    | ShellFileKind::CandidateOutcome
                    | ShellFileKind::FramePermit
                    | ShellFileKind::Action
            ) && self.capabilities & SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE == 0
            {
                return Err(ShellClientError::MissingCapability);
            }
            if header.kind == ShellFileKind::CatalogActivationOutcome
                || (header.kind == ShellFileKind::IndicatorActivationOutcome
                    && self.capabilities & SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION == 0)
            {
                return Err(ShellClientError::MissingCapability);
            }
        }
        let event = match header.kind {
            ShellFileKind::Submitted => {
                let value = decode_shell_file_submitted(record)?;
                Event::Submitted(value.submission_id, value.candidate_kind)
            }
            ShellFileKind::ObjectPublished => {
                let published = decode_shell_file_object_published(record)?;
                let index = feed_index(published.object)
                    .filter(|_| self.feed_disclosed(published.object))
                    .ok_or(ShellClientError::Protocol("announced object kind"))?;
                Event::Published(published, index)
            }
            ShellFileKind::Refused => Event::Inbound(Inbound::Content(
                TransactionId::INVALID,
                ShellContentRecord::AdmissionRefused(decode_shell_file_refused(record)?),
            )),
            ShellFileKind::AllocationResult => {
                let value = decode_shell_file_allocation_result(record)?;
                Event::Inbound(Inbound::Content(value.transaction, value.record))
            }
            ShellFileKind::ResourceStatus => {
                let value = decode_shell_file_resource_status(record)?;
                let ShellContentRecord::ResourceStatus(status) = &value.record else {
                    return Err(ShellClientError::Protocol("resource status shape"));
                };
                Event::ResourceStatus(
                    status.resource,
                    status.status,
                    Inbound::Content(value.transaction, value.record),
                )
            }
            ShellFileKind::ResourceReleased => {
                let value = decode_shell_file_resource_released(record)?;
                Event::Inbound(Inbound::Content(value.transaction, value.record))
            }
            ShellFileKind::CandidateOutcome
            | ShellFileKind::FramePermit
            | ShellFileKind::Action => {
                let value = decode_shell_file_transaction(record, header.kind)?;
                Event::Inbound(Inbound::Content(value.transaction, value.record))
            }
            ShellFileKind::CatalogActivationOutcome => {
                let value = decode_shell_file_catalog_action(record, header.kind)?;
                let ShellCatalogActionRecord::ActivationOutcome(outcome) = value.record else {
                    return Err(ShellClientError::Protocol("catalog outcome shape"));
                };
                Event::Inbound(Inbound::CatalogOutcome(value.transaction, outcome))
            }
            ShellFileKind::IndicatorActivationOutcome => {
                let value = decode_shell_file_indicator_activation_outcome(record)?;
                Event::Inbound(Inbound::IndicatorOutcome(value.transaction, value.outcome))
            }
            kind if shell_file_descriptor_max_bytes(kind).is_some() => {
                self.require_descriptor_kind(kind)?;
                Event::Inbound(Inbound::Descriptor(decode_shell_file_descriptor(
                    record, kind,
                )?))
            }
            _ => return Err(ShellClientError::Protocol("unexpected event kind")),
        };
        Ok((header.sequence, event))
    }

    fn handle_event(
        &mut self,
        record: Vec<u8>,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        let (sequence, event) = self.decode_event(&record)?;
        if sequence <= self.ack_ready {
            return Err(ShellClientError::Protocol("event sequence did not rise"));
        }
        match event {
            Event::Submitted(submission, kind) => {
                self.on_submitted(submission, kind)?;
                self.submitted_sequence = sequence;
            }
            Event::Published(published, index) => {
                let bound = sequence - 1;
                let hold = self.holds[index].get_or_insert(bound);
                *hold = (*hold).min(bound);
                if !self.superseded(published.object) {
                    self.object_fetch = Some(ObjectFetch {
                        feed: published.object,
                        generation: published.generation,
                        qid: published.qid,
                        phase: FetchPhase::NotStarted,
                        buf: Vec::new(),
                        restarts: 0,
                    });
                }
            }
            Event::ResourceStatus(resource, status, item) => {
                self.observe_resource_status(resource, status)?;
                inbox.push_back(item);
            }
            Event::Inbound(item) => inbox.push_back(item),
        }
        self.ack_ready = sequence;
        Ok(())
    }

    /// The buffered events, in order, up to the first that fails the same
    /// checks normal intake applies (full decode, attach epoch, rising
    /// sequence): every `Submitted` among them. Nothing past a failing
    /// record is looked at, and nothing is changed.
    pub(super) fn buffered_submissions(&self) -> Vec<(u64, ShellFileKind)> {
        let mut submitted = Vec::new();
        let mut rest = &self.event_buf[..];
        let mut last = self.ack_ready;
        while check_record_length(rest).is_ok()
            && let Some(size) = complete_record(rest)
        {
            let Ok((sequence, event)) = self.decode_event(&rest[..size]) else {
                break;
            };
            if sequence <= last {
                break;
            }
            last = sequence;
            if let Event::Submitted(submission, kind) = event {
                submitted.push((submission, kind));
            }
            rest = &rest[size..];
        }
        submitted
    }

    pub(super) fn drive_object_fetch(&mut self) -> Result<bool, ShellClientError> {
        let Some(fetch) = &mut self.object_fetch else {
            return Ok(false);
        };
        if fetch.phase != FetchPhase::NotStarted {
            return Ok(false);
        }
        let name: &[u8] = match fetch.feed {
            ShellFileKind::Outputs => b"outputs",
            ShellFileKind::Catalog => b"catalog",
            ShellFileKind::Indicators => b"indicators",
            ShellFileKind::Descriptors => b"descriptors",
            ShellFileKind::Tabs => b"tabs",
            ShellFileKind::Shortcuts => b"shortcuts",
            _ => return Err(ShellClientError::Protocol("announced object kind")),
        };
        fetch.buf.clear();
        let (tag, fid) = self.pipeline.walk(self.root, &[name])?;
        if let Some(fetch) = &mut self.object_fetch {
            fetch.phase = FetchPhase::Walk { tag, fid };
        }
        Ok(true)
    }

    /// Restarts a fetch whose node answered `ESTALE` or `EAGAIN` (the object
    /// moved on, or is not yet current), a bounded number of times.
    fn restart_fetch(
        &mut self,
        fid: Option<sophia_9p_records::Fid>,
    ) -> Result<(), ShellClientError> {
        if let Some(fid) = fid {
            self.forget(fid)?;
        }
        let fetch = self.object_fetch.as_mut().expect("a fetch is in flight");
        if fetch.restarts == MAX_FETCH_RESTARTS {
            return Err(ShellClientError::Protocol("snapshot object unavailable"));
        }
        fetch.restarts += 1;
        fetch.phase = FetchPhase::NotStarted;
        Ok(())
    }

    pub(super) fn on_object_fetch_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<bool, ShellClientError> {
        let Some(fetch) = &mut self.object_fetch else {
            return Ok(false);
        };
        let (phase_tag, fid) = match fetch.phase {
            FetchPhase::Walk { tag, fid }
            | FetchPhase::Open { tag, fid }
            | FetchPhase::Read { tag, fid, .. }
            | FetchPhase::Probe { tag, fid } => (tag, fid),
            FetchPhase::NotStarted => return Ok(false),
        };
        if phase_tag != tag {
            return Ok(false);
        }
        let cap = FEEDS[feed_index(fetch.feed).expect("fetched feeds are known")].1;
        if let Reply::Error(errno) = reply
            && (*errno == Errno::ESTALE || *errno == Errno::EAGAIN)
        {
            let walked = !matches!(fetch.phase, FetchPhase::Walk { .. });
            self.restart_fetch(walked.then_some(fid))?;
            return Ok(true);
        }
        match (fetch.phase, reply) {
            (FetchPhase::Walk { .. }, Reply::Walk(qids)) if qids.len() == 1 => {
                let open = self.pipeline.lopen(fid, O_RDONLY)?;
                self.object_fetch.as_mut().unwrap().phase = FetchPhase::Open { tag: open, fid };
            }
            (FetchPhase::Open { .. }, Reply::Lopen { qid, iounit }) => {
                if qid.path != fetch.qid {
                    // Opening pinned a newer object than this announcement:
                    // deliver nothing and keep the hold until the newer
                    // announcement is handled.
                    self.forget(fid)?;
                    self.object_fetch = None;
                    return Ok(true);
                }
                let count = if *iounit == 0 { u32::MAX } else { *iounit };
                let read = self.pipeline.read(fid, 0, count)?;
                self.object_fetch.as_mut().unwrap().phase = FetchPhase::Read {
                    tag: read,
                    fid,
                    count,
                };
            }
            (FetchPhase::Read { count, .. }, Reply::Read(data)) => {
                if data.is_empty() {
                    return self.finish_fetch(fid, inbox).map(|()| true);
                }
                if fetch.buf.len() + data.len() > cap {
                    return Err(ShellClientError::Protocol("snapshot object over its cap"));
                }
                // Exact growth: capacity never passes the cap.
                fetch
                    .buf
                    .try_reserve_exact(data.len())
                    .map_err(|_| ShellClientError::Protocol("snapshot object allocation"))?;
                if fetch.buf.capacity() > cap {
                    return Err(ShellClientError::Protocol("snapshot object allocation"));
                }
                fetch.buf.extend_from_slice(data);
                let offset = fetch.buf.len() as u64;
                let phase = if fetch.buf.len() == cap {
                    // Exactly the cap: the next read must find the end.
                    FetchPhase::Probe {
                        tag: self.pipeline.read(fid, offset, 1)?,
                        fid,
                    }
                } else {
                    FetchPhase::Read {
                        tag: self.pipeline.read(fid, offset, count)?,
                        fid,
                        count,
                    }
                };
                self.object_fetch.as_mut().unwrap().phase = phase;
            }
            (FetchPhase::Probe { .. }, Reply::Read(data)) if data.is_empty() => {
                self.finish_fetch(fid, inbox)?;
            }
            (FetchPhase::Probe { .. }, Reply::Read(_)) => {
                return Err(ShellClientError::Protocol("snapshot object over its cap"));
            }
            _ => return Err(ShellClientError::Protocol("snapshot object fetch refused")),
        }
        Ok(true)
    }

    /// The whole object is held and the node reported its end: decode it,
    /// require the announced generation, deliver it in place of any older
    /// undelivered one of its feed, and release the feed's hold.
    fn finish_fetch(
        &mut self,
        fid: sophia_9p_records::Fid,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        self.forget(fid)?;
        let fetch = self.object_fetch.take().expect("a fetch is in flight");
        // Drop the older undelivered object of this feed before decoding, so
        // the peak is one decoded object, not two.
        match fetch.feed {
            ShellFileKind::Catalog => {
                inbox.retain(|queued| !matches!(queued, Inbound::Catalog(..) | Inbound::ApplicationCatalog(..)))
            }
            ShellFileKind::Indicators => {
                inbox.retain(|queued| !matches!(queued, Inbound::Indicators(..)))
            }
            ShellFileKind::Outputs => inbox.retain(|queued| {
                !matches!(
                    queued,
                    Inbound::Content(_, ShellContentRecord::OutputFacts(_))
                )
            }),
            kind => inbox.retain(|queued| !matches!(queued, Inbound::Descriptor(value) if shell_file_descriptor_kind(&value.record) == kind)),
        }
        let header = decode_shell_file_record(&fetch.buf, ShellFileClass::Object)?.header;
        if header.kind != fetch.feed || header.connection_epoch != self.epoch {
            return Err(ShellClientError::Protocol(
                "snapshot object from another feed or connection epoch",
            ));
        }
        let (generation, item) = match fetch.feed {
            ShellFileKind::Outputs => {
                let value = decode_shell_file_outputs(&fetch.buf)?;
                let ShellContentRecord::OutputFacts(facts) = &value.record else {
                    return Err(ShellClientError::Protocol("outputs object shape"));
                };
                (
                    facts.facts_generation,
                    Inbound::Content(value.transaction, value.record),
                )
            }
            ShellFileKind::Catalog => {
                let value = decode_shell_file_catalog(&fetch.buf)?;
                if value.catalog.catalog.connection_epoch != self.epoch
                    || (self.descriptor && !value.catalog.identities.is_empty())
                {
                    return Err(ShellClientError::Protocol("catalog identity or profile"));
                }
                if super::budget::catalog_bytes(&value.catalog) > super::budget::CATALOG_BUDGET {
                    return Err(ShellClientError::Protocol(
                        "decoded catalog over its budget",
                    ));
                }
                let generation = value.catalog.catalog.generation;
                let item = if self.descriptor {
                    Inbound::ApplicationCatalog(value.transaction, value.catalog.catalog)
                } else {
                    Inbound::Catalog(value.transaction, value.catalog)
                };
                (generation, item)
            }
            ShellFileKind::Indicators => {
                let value = decode_shell_file_indicators(&fetch.buf)?;
                if super::budget::indicators_bytes(&value.snapshot)
                    > super::budget::INDICATORS_BUDGET
                {
                    return Err(ShellClientError::Protocol(
                        "decoded indicators over its budget",
                    ));
                }
                (
                    value.snapshot.generation,
                    Inbound::Indicators(value.transaction, value.snapshot),
                )
            }
            ShellFileKind::Descriptors | ShellFileKind::Tabs | ShellFileKind::Shortcuts => {
                let value = decode_shell_file_descriptor(&fetch.buf, fetch.feed)?;
                super::budget::check_descriptor(&value.record)?;
                let generation = match &value.record {
                    ShellDescriptorRecord::Descriptors(v) => v.snapshot_generation,
                    ShellDescriptorRecord::Tabs(v) => v.generation,
                    ShellDescriptorRecord::Shortcuts(v) => v.generation,
                    _ => return Err(ShellClientError::Protocol("descriptor object shape")),
                };
                (generation, Inbound::Descriptor(value))
            }
            _ => return Err(ShellClientError::Protocol("announced object kind")),
        };
        if generation != fetch.generation {
            return Err(ShellClientError::Protocol(
                "snapshot object generation differs from its announcement",
            ));
        }
        inbox.push_back(item);
        self.holds[feed_index(fetch.feed).expect("fetched feeds are known")] = None;
        Ok(())
    }
}
