//! Inbound event dispatch: turning one whole `events` record into the same
//! typed `Inbound` the socket wire produces. `ObjectPublished` for `outputs`
//! needs its own further fetch (open, read, clunk) before it becomes one;
//! that fetch is a small state machine of its own, driven the same way the
//! submission and upload machinery are.
use std::collections::VecDeque;

use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::Tag;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{ShellContentRecord, TransactionId};

use super::{Current, FetchPhase, FileWire, O_RDONLY, ObjectFetch, SubmissionPhase};
use crate::ShellClientError;
use crate::wire::Inbound;

impl FileWire {
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
        if self.event_buf.len() < 4 {
            return Ok(false);
        }
        let size = u32::from_le_bytes(self.event_buf[..4].try_into().unwrap()) as usize;
        if size < SHELL_FILE_HEADER_BYTES || self.event_buf.len() < size {
            return Ok(false);
        }
        let header =
            decode_shell_file_record(&self.event_buf[..size], ShellFileClass::Event)?.header;
        let needs_inbox_room = !matches!(header.kind, ShellFileKind::Submitted);
        if needs_inbox_room && inbox.len() >= crate::MAX_QUEUED_FRAMES {
            return Ok(false);
        }
        let record: Vec<u8> = self.event_buf.drain(..size).collect();
        self.handle_event(record, inbox)
    }

    fn handle_event(
        &mut self,
        record: Vec<u8>,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<bool, ShellClientError> {
        let header = decode_shell_file_record(&record, ShellFileClass::Event)?.header;
        let sequence = header.sequence;
        match header.kind {
            ShellFileKind::Submitted => {
                let submitted = decode_shell_file_submitted(&record)?;
                let matches = matches!(
                    &self.current,
                    Some(Current::Submission {
                        submission_id,
                        phase: SubmissionPhase::AwaitSubmitted,
                        ..
                    }) if *submission_id == submitted.submission_id
                );
                if !matches {
                    return Err(ShellClientError::Protocol("unexpected Submitted event"));
                }
                self.current = None;
                self.ack_ready = self.ack_ready.max(sequence);
            }
            ShellFileKind::ObjectPublished => {
                let published = decode_shell_file_object_published(&record)?;
                if published.object == ShellFileKind::Outputs {
                    self.object_fetch = Some(ObjectFetch {
                        sequence,
                        phase: FetchPhase::NotStarted,
                    });
                } else {
                    // Limits is immutable and already fetched at connect; no
                    // other object kind is implemented server-side yet.
                    self.ack_ready = self.ack_ready.max(sequence);
                }
            }
            ShellFileKind::Refused => {
                let refused = decode_shell_file_refused(&record)?;
                inbox.push_back(Inbound::Content(
                    TransactionId::INVALID,
                    ShellContentRecord::AdmissionRefused(refused),
                ));
                self.ack_ready = self.ack_ready.max(sequence);
            }
            ShellFileKind::AllocationResult => {
                let value = decode_shell_file_allocation_result(&record)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
                self.ack_ready = self.ack_ready.max(sequence);
            }
            ShellFileKind::ResourceStatus => {
                let value = decode_shell_file_resource_status(&record)?;
                let ShellContentRecord::ResourceStatus(status) = &value.record else {
                    return Err(ShellClientError::Protocol("resource status shape"));
                };
                self.observe_resource_status(status.resource, status.status)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
                self.ack_ready = self.ack_ready.max(sequence);
            }
            ShellFileKind::ResourceReleased => {
                let value = decode_shell_file_resource_released(&record)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
                self.ack_ready = self.ack_ready.max(sequence);
            }
            ShellFileKind::CandidateOutcome
            | ShellFileKind::FramePermit
            | ShellFileKind::Action => {
                let value = decode_shell_file_transaction(&record, header.kind)?;
                inbox.push_back(Inbound::Content(value.transaction, value.record));
                self.ack_ready = self.ack_ready.max(sequence);
            }
            _ => return Err(ShellClientError::Protocol("unexpected event kind")),
        }
        Ok(true)
    }

    pub(super) fn drive_object_fetch(&mut self) -> Result<bool, ShellClientError> {
        let Some(fetch) = &mut self.object_fetch else {
            return Ok(false);
        };
        if fetch.phase != FetchPhase::NotStarted {
            return Ok(false);
        }
        let (tag, fid) = self.pipeline.walk(self.root, &[b"outputs"])?;
        fetch.phase = FetchPhase::Walk { tag, fid };
        Ok(true)
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
        let matches = match fetch.phase {
            FetchPhase::Walk { tag: t, .. }
            | FetchPhase::Open { tag: t, .. }
            | FetchPhase::Read { tag: t, .. } => t == tag,
            FetchPhase::NotStarted => false,
        };
        if !matches {
            return Ok(false);
        }
        match fetch.phase {
            FetchPhase::Walk { fid, .. } => match reply {
                Reply::Walk(qids) if qids.len() == 1 => {
                    let open_tag = self.pipeline.lopen(fid, O_RDONLY)?;
                    self.object_fetch.as_mut().unwrap().phase =
                        FetchPhase::Open { tag: open_tag, fid };
                }
                _ => return Err(ShellClientError::Protocol("outputs walk refused")),
            },
            FetchPhase::Open { fid, .. } => match reply {
                Reply::Lopen { .. } => {
                    let read_tag = self.pipeline.read(fid, 0, u32::MAX)?;
                    self.object_fetch.as_mut().unwrap().phase =
                        FetchPhase::Read { tag: read_tag, fid };
                }
                _ => return Err(ShellClientError::Protocol("outputs open refused")),
            },
            FetchPhase::Read { fid, .. } => match reply {
                Reply::Read(data) => {
                    let clunk_tag = self.pipeline.clunk(fid)?;
                    self.forgettable.insert(clunk_tag);
                    let decoded = decode_shell_file_outputs(data)?;
                    let sequence = self.object_fetch.take().unwrap().sequence;
                    self.ack_ready = self.ack_ready.max(sequence);
                    inbox.push_back(Inbound::Content(decoded.transaction, decoded.record));
                }
                _ => return Err(ShellClientError::Protocol("outputs read refused")),
            },
            FetchPhase::NotStarted => unreachable!("checked above"),
        }
        Ok(true)
    }
}
