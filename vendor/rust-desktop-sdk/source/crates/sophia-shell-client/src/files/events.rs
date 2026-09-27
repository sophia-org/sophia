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
use sophia_shell_protocol::{ShellCatalogActionRecord, ShellContentRecord, TransactionId};

use super::{FEEDS, FetchPhase, FileWire, MAX_FETCH_RESTARTS, O_RDONLY, ObjectFetch, feed_index};
use crate::ShellClientError;
use crate::wire::Inbound;

/// The whole record at the front of `bytes`, if one is complete.
fn complete_record(bytes: &[u8]) -> Option<usize> {
    let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    (size >= SHELL_FILE_HEADER_BYTES && bytes.len() >= size).then_some(size)
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
        self.handle_event(record, inbox)?;
        self.progress += 1;
        Ok(true)
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

    fn handle_event(
        &mut self,
        record: Vec<u8>,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        let header = decode_shell_file_record(&record, ShellFileClass::Event)?.header;
        let sequence = header.sequence;
        if sequence <= self.ack_ready {
            return Err(ShellClientError::Protocol("event sequence did not rise"));
        }
        match header.kind {
            ShellFileKind::Submitted => {
                let submitted = decode_shell_file_submitted(&record)?;
                self.on_submitted(submitted.submission_id)?;
            }
            ShellFileKind::ObjectPublished => {
                let published = decode_shell_file_object_published(&record)?;
                let index = feed_index(published.object)
                    .ok_or(ShellClientError::Protocol("announced object kind"))?;
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
            ShellFileKind::Refused => {
                let refused = decode_shell_file_refused(&record)?;
                inbox.push_back(Inbound::Content(
                    TransactionId::INVALID,
                    ShellContentRecord::AdmissionRefused(refused),
                ));
            }
            ShellFileKind::AllocationResult => {
                let value = decode_shell_file_allocation_result(&record)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
            }
            ShellFileKind::ResourceStatus => {
                let value = decode_shell_file_resource_status(&record)?;
                let ShellContentRecord::ResourceStatus(status) = &value.record else {
                    return Err(ShellClientError::Protocol("resource status shape"));
                };
                self.observe_resource_status(status.resource, status.status)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
            }
            ShellFileKind::ResourceReleased => {
                let value = decode_shell_file_resource_released(&record)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
            }
            ShellFileKind::CandidateOutcome
            | ShellFileKind::FramePermit
            | ShellFileKind::Action => {
                let value = decode_shell_file_transaction(&record, header.kind)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
            }
            ShellFileKind::CatalogActivationOutcome => {
                let value = decode_shell_file_catalog_action(&record, header.kind)?;
                let ShellCatalogActionRecord::ActivationOutcome(outcome) = value.record else {
                    return Err(ShellClientError::Protocol("catalog outcome shape"));
                };
                inbox.push_back(Inbound::CatalogOutcome(value.transaction, outcome));
            }
            ShellFileKind::IndicatorActivationOutcome => {
                let value = decode_shell_file_indicator_activation_outcome(&record)?;
                inbox.push_back(Inbound::IndicatorOutcome(value.transaction, value.outcome));
            }
            _ => return Err(ShellClientError::Protocol("unexpected event kind")),
        }
        self.ack_ready = sequence;
        Ok(())
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
            let clunk = self.pipeline.clunk(fid)?;
            self.forgettable.insert(clunk);
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
                    let clunk = self.pipeline.clunk(fid)?;
                    self.forgettable.insert(clunk);
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
        let clunk = self.pipeline.clunk(fid)?;
        self.forgettable.insert(clunk);
        let fetch = self.object_fetch.take().expect("a fetch is in flight");
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
                (
                    value.catalog.catalog.generation,
                    Inbound::Catalog(value.transaction, value.catalog),
                )
            }
            ShellFileKind::Indicators => {
                let value = decode_shell_file_indicators(&fetch.buf)?;
                (
                    value.snapshot.generation,
                    Inbound::Indicators(value.transaction, value.snapshot),
                )
            }
            _ => return Err(ShellClientError::Protocol("announced object kind")),
        };
        if generation != fetch.generation {
            return Err(ShellClientError::Protocol(
                "snapshot object generation differs from its announcement",
            ));
        }
        match item {
            Inbound::Catalog(..) => inbox.retain(|queued| !matches!(queued, Inbound::Catalog(..))),
            Inbound::Indicators(..) => {
                inbox.retain(|queued| !matches!(queued, Inbound::Indicators(..)))
            }
            _ => {}
        }
        inbox.push_back(item);
        self.holds[feed_index(fetch.feed).expect("fetched feeds are known")] = None;
        Ok(())
    }
}
