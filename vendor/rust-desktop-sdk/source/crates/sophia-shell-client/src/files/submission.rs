//! The single-lane outbound queue and the submission state machine that
//! drives one candidate record through `transaction`+`submit`: walk, open,
//! write the record, write `submit`, then wait for the journal's own
//! `Submitted` event (handled in `events.rs`) before the next queued unit
//! starts. Mirrors `tests/support/shell_file_peer.rs`'s `submit`/`clear`.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::Tag;
use sophia_shell_protocol::shell_files::{
    ShellFileClass, ShellFileSubmit, decode_shell_file_record, encode_shell_file_submit,
};

use super::{Current, FileWire, O_RDWR, QueuedKind, SubmissionPhase, open_fid_for};
use crate::ShellClientError;
use crate::outbox::ClientOutbox;

impl FileWire {
    /// Begins the next queued unit once nothing else is in flight. A queued
    /// slot write whose resource is not yet open stays at the front,
    /// blocking later units too (submissions are sequential per attach, and
    /// upload order matters for the same resource).
    pub(super) fn start_next_outbound(
        &mut self,
        output: &mut ClientOutbox,
    ) -> Result<bool, ShellClientError> {
        if self.current.is_some() {
            return Ok(false);
        }
        let Some(kind) = self.pending.front().copied() else {
            return Ok(false);
        };
        if let QueuedKind::SlotWrite { resource, .. } = kind
            && open_fid_for(&self.uploads, resource).is_none()
        {
            return Ok(false);
        }
        let bytes = output
            .front()
            .expect("pending mirrors the outbox one-for-one")
            .to_vec();
        output.written(bytes.len());
        self.pending.pop_front();
        match kind {
            QueuedKind::Record => {
                let submission_id = decode_shell_file_record(&bytes, ShellFileClass::Candidate)?
                    .header
                    .submission_id;
                let (tag, fid) = self.pipeline.walk(self.root, &[b"transaction"])?;
                self.current = Some(Current::Submission {
                    bytes,
                    submission_id,
                    phase: SubmissionPhase::Walk { tag, fid },
                });
                Ok(true)
            }
            QueuedKind::SlotWrite { resource, offset } => {
                self.current = Some(Current::SlotWrite {
                    resource,
                    offset,
                    remaining: bytes,
                    tag: None,
                });
                Ok(true)
            }
        }
    }

    /// Advances the current submission's own phase once its outstanding
    /// tag's reply arrives. Returns `false` (unconsumed) for any other tag,
    /// including one belonging to a slot open, a slot write or the events
    /// read, which their own handlers try next.
    pub(super) fn on_submission_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
    ) -> Result<bool, ShellClientError> {
        let Some(Current::Submission {
            bytes,
            submission_id,
            phase,
        }) = &mut self.current
        else {
            return Ok(false);
        };
        match *phase {
            SubmissionPhase::Walk { tag: t, fid } if t == tag => match reply {
                Reply::Walk(qids) if qids.len() == 1 => {
                    let open_tag = self.pipeline.lopen(fid, O_RDWR)?;
                    *phase = SubmissionPhase::Open { tag: open_tag, fid };
                    Ok(true)
                }
                _ => Err(ShellClientError::Protocol("transaction walk refused")),
            },
            SubmissionPhase::Open { tag: t, fid } if t == tag => match reply {
                Reply::Lopen { .. } => {
                    let write_tag = self.pipeline.write(fid, 0, bytes)?;
                    *phase = SubmissionPhase::WriteRecord {
                        tag: write_tag,
                        fid,
                    };
                    Ok(true)
                }
                _ => Err(ShellClientError::Protocol("transaction open refused")),
            },
            SubmissionPhase::WriteRecord { tag: t, fid } if t == tag => match reply {
                Reply::Write(count) if *count as usize == bytes.len() => {
                    let submit_bytes = encode_shell_file_submit(ShellFileSubmit {
                        connection_epoch: self.epoch,
                        submission_id: *submission_id,
                        candidate_bytes: bytes.len() as u32,
                    })?;
                    let submit_tag = self.pipeline.write(self.submit_fid, 0, &submit_bytes)?;
                    *phase = SubmissionPhase::WriteSubmit {
                        tag: submit_tag,
                        fid,
                    };
                    Ok(true)
                }
                _ => Err(ShellClientError::Protocol(
                    "transaction record write refused",
                )),
            },
            SubmissionPhase::WriteSubmit { tag: t, fid } if t == tag => match reply {
                Reply::Write(_) => {
                    let clunk_tag = self.pipeline.clunk(fid)?;
                    self.forgettable.insert(clunk_tag);
                    *phase = SubmissionPhase::AwaitSubmitted;
                    Ok(true)
                }
                _ => Err(ShellClientError::Protocol("submit refused")),
            },
            _ => Ok(false),
        }
    }
}
