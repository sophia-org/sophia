//! The single-lane outbound queue and the submission state machine that
//! drives one candidate record through `transaction`+`submit`: walk, open,
//! write the record, write `submit`, then settle on the journal's own
//! `Submitted` event (handled in `events.rs`), which may arrive before or
//! after the `submit` reply. A unit stays at the front of the outbox until
//! its custody is settled, so a disconnect can still classify it.
//!
//! `submit` answers: `Rwrite` with `Submitted` is custody; `EAGAIN` has
//! transferred nothing and the same submit goes again later, the staged
//! record untouched; `EALREADY` is custody only when `Submitted` was already
//! observed, otherwise the outcome is unknown and the connection fails
//! closed; `ESTALE` is revocation; any other error number is the session's
//! definitive refusal, recorded as-is, and the lane moves on.
use std::time::{Duration, Instant};

use sophia_9p_client::pipeline::{PipelineError, Reply};
use sophia_9p_records::{Errno, Tag};
use sophia_shell_protocol::shell_files::{
    ShellFileClass, ShellFileKind, ShellFileSubmit, decode_shell_file_record,
    encode_shell_file_submit,
};

use super::{
    Current, FileWire, O_RDWR, Opening, QueuedKind, RETRY_FIRST, RETRY_MAX, SubmissionPhase,
    SubmitReply,
};
use crate::ShellClientError;
use crate::custody::Custody;
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
            && super::open_fid_for(&self.uploads, resource).is_none()
            && self
                .uploads
                .iter()
                .flatten()
                .any(|upload| upload.resource == resource)
        {
            return Ok(false);
        }
        // A record needs the `transaction` node, which the export keeps busy
        // until the previous submission's `Submitted` is acknowledged. The
        // export serves a connection's requests in order, so the covering
        // ack must be queued before this unit's walk and open: wait until
        // intake has reached that event and an ack through it is queued.
        if matches!(kind, QueuedKind::Record)
            && (self.early_submitted.is_some() || self.acked < self.submitted_sequence)
        {
            return Ok(false);
        }
        let ticket = output.front_ticket().expect("the front unit has a ticket");
        let current = match kind {
            QueuedKind::Record => {
                let bytes = output
                    .front()
                    .expect("pending mirrors the outbox one-for-one");
                let header = decode_shell_file_record(bytes, ShellFileClass::Candidate)?.header;
                let (submission_id, kind) = (header.submission_id, header.kind);
                let phase = if let Some(fid) = self.transaction_fid {
                    let tag = self.pipeline.write(fid, 0, bytes)?;
                    SubmissionPhase::WriteRecord { tag, fid }
                } else {
                    let name: &[u8] = if self.reusable_transactions {
                        b"transaction-reuse"
                    } else {
                        b"transaction"
                    };
                    SubmissionPhase::Opening(Opening::start(
                        &mut self.pipeline,
                        self.root,
                        &[name],
                        O_RDWR,
                    )?)
                };
                Current::Submission {
                    bytes: output.take_front_bytes(),
                    submission_id,
                    kind,
                    ticket,
                    submit: Vec::new(),
                    phase,
                    submitted: false,
                    accepted: false,
                }
            }
            QueuedKind::SlotWrite { resource, offset } => Current::SlotWrite {
                resource,
                offset,
                remaining: output.take_front_bytes(),
                acknowledged: 0,
                tag: None,
                ticket,
                not_before: None,
                wrote_any: false,
            },
        };
        self.current = Some(current);
        self.pending.pop_front();
        self.custody.push((ticket, Custody::InFlight));
        Ok(true)
    }

    /// Settles the current unit: records its custody and releases it from
    /// the front of the outbox.
    pub(super) fn settle_current(&mut self, custody: Custody) {
        let ticket = match self.current.take() {
            Some(Current::Submission { ticket, .. } | Current::SlotWrite { ticket, .. }) => ticket,
            None => return,
        };
        self.custody.push((ticket, custody));
        self.retire += 1;
    }

    /// The current submission's `Submitted` event arrived. Custody is
    /// settled at once; the unit itself completes when `submit` has also
    /// answered.
    pub(super) fn on_submitted(
        &mut self,
        submission: u64,
        candidate_kind: ShellFileKind,
    ) -> Result<(), ShellClientError> {
        if self.early_submitted == Some((submission, candidate_kind)) {
            // Already settled from the buffered events (see
            // `settle_buffered_submitted`); this is that same event.
            self.early_submitted = None;
            return Ok(());
        }
        let Some(Current::Submission {
            submission_id,
            kind,
            ticket,
            phase,
            submitted,
            ..
        }) = &mut self.current
        else {
            return Err(ShellClientError::Protocol("unexpected Submitted event"));
        };
        let issued = matches!(
            phase,
            SubmissionPhase::WritePair { .. }
                | SubmissionPhase::WriteSubmit { .. }
                | SubmissionPhase::AwaitSubmitted
        );
        if *submission_id != submission || *kind != candidate_kind || *submitted || !issued {
            return Err(ShellClientError::Protocol("unexpected Submitted event"));
        }
        *submitted = true;
        let (ticket, done) = (*ticket, *phase == SubmissionPhase::AwaitSubmitted);
        self.custody.push((ticket, Custody::Submitted));
        if done {
            self.settle_current(Custody::Submitted);
        }
        Ok(())
    }

    /// Resends an `EAGAIN`-refused submit once an event has been consumed
    /// since, or its backoff has passed. Never in the pass that saw the
    /// refusal: that pass recorded the current progress count.
    pub(super) fn drive_submit_retry(&mut self) -> Result<bool, ShellClientError> {
        let Some(Current::Submission {
            submit,
            phase:
                SubmissionPhase::RetryWait {
                    fid,
                    not_before,
                    progress,
                    pass,
                },
            ..
        }) = &self.current
        else {
            return Ok(false);
        };
        // Only a pending retry without new event progress needs a clock read.
        if self.pass == *pass
            || (self.progress == *progress
                && self.dispatch_time.unwrap_or_else(Instant::now) < *not_before)
        {
            return Ok(false);
        }
        let fid = *fid;
        let tag = self.pipeline.write(self.submit_fid, 0, submit)?;
        if let Some(Current::Submission { phase, .. }) = &mut self.current {
            *phase = SubmissionPhase::WriteSubmit { tag, fid };
        }
        Ok(true)
    }

    /// Marks the current submission Submitted if its `Submitted` event is
    /// among the buffered events that pass normal intake's checks, and lets
    /// intake accept that event once when it gets there.
    fn settle_buffered_submitted(&mut self) {
        let Some(Current::Submission {
            submission_id,
            kind,
            submitted: false,
            phase:
                SubmissionPhase::WritePair { .. }
                | SubmissionPhase::WriteSubmit { .. }
                | SubmissionPhase::AwaitSubmitted,
            ..
        }) = &self.current
        else {
            return;
        };
        let target = (*submission_id, *kind);
        if self.buffered_submissions().contains(&target)
            && self.on_submitted(target.0, target.1).is_ok()
        {
            self.early_submitted = Some(target);
        }
    }

    /// The instant a pending `EAGAIN` retry becomes due without event
    /// progress, if one is pending.
    pub(crate) fn retry_deadline(&self) -> Option<Instant> {
        match &self.current {
            Some(Current::Submission {
                phase: SubmissionPhase::RetryWait { not_before, .. },
                ..
            }) => Some(*not_before),
            Some(Current::SlotWrite {
                not_before: Some((not_before, _)),
                ..
            }) => Some(*not_before),
            _ => None,
        }
    }

    /// The next backoff, doubling up to `RETRY_MAX`.
    pub(super) fn next_backoff(&mut self) -> Duration {
        let backoff = self.backoff;
        self.backoff = (backoff * 2).min(RETRY_MAX);
        backoff
    }

    /// The existing record write is whole, so it is also its final fragment.
    /// A capacity refusal queues nothing and keeps the sequential path valid.
    pub(super) fn queue_submit_with_record(&mut self) -> Result<bool, ShellClientError> {
        let Some(Current::Submission {
            bytes,
            submission_id,
            submit,
            phase,
            ..
        }) = &mut self.current
        else {
            return Ok(false);
        };
        let SubmissionPhase::WriteRecord { tag, fid } = *phase else {
            return Ok(false);
        };
        if submit.is_empty() {
            *submit = encode_shell_file_submit(ShellFileSubmit {
                connection_epoch: self.epoch,
                submission_id: *submission_id,
                candidate_bytes: bytes.len() as u32,
            })?;
        }
        let submit_tag = match self.pipeline.write(self.submit_fid, 0, submit) {
            Ok(tag) => tag,
            Err(PipelineError::Limit(_)) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        *phase = SubmissionPhase::WritePair {
            record_tag: tag,
            submit_tag,
            fid,
            record_ok: None,
            reply: None,
        };
        Ok(true)
    }

    /// Reconcile only after draining replies: a later read in the same batch
    /// can carry Submitted and must survive a contradictory record result.
    pub(super) fn finish_write_pair(&mut self) -> Result<bool, ShellClientError> {
        let Some(Current::Submission {
            phase:
                SubmissionPhase::WritePair {
                    submit_tag,
                    fid,
                    record_ok: Some(ok),
                    reply: Some(reply),
                    ..
                },
            ..
        }) = &self.current
        else {
            return Ok(false);
        };
        let (tag, fid, ok, reply) = (*submit_tag, *fid, *ok, *reply);
        if !ok {
            self.settle_buffered_submitted();
            let Some(Current::Submission {
                submitted, submit, ..
            }) = &self.current
            else {
                unreachable!("a paired write cannot retire before reconciliation");
            };
            let custody = if *submitted
                || matches!(reply, SubmitReply::Write(n) if n as usize == submit.len())
            {
                Custody::Submitted
            } else {
                match reply {
                    SubmitReply::Error(errno) if errno != Errno::EALREADY => Custody::DroppedUnsent,
                    _ => Custody::Unknown,
                }
            };
            self.settle_current(custody);
            return Err(ShellClientError::Protocol(
                "transaction record write refused",
            ));
        }
        if let Some(Current::Submission { phase, .. }) = &mut self.current {
            *phase = SubmissionPhase::WriteSubmit { tag, fid };
        }
        let reply = match reply {
            SubmitReply::Write(count) => Reply::Write(count),
            SubmitReply::Error(errno) => Reply::Error(errno),
            SubmitReply::Invalid => {
                return Err(ShellClientError::Protocol("unexpected submit reply"));
            }
        };
        self.on_submission_reply(tag, &reply)
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
            submit,
            phase,
            accepted,
            ..
        }) = &mut self.current
        else {
            return Ok(false);
        };
        let is_submit = match phase {
            SubmissionPhase::WritePair { submit_tag, .. } => tag == *submit_tag,
            SubmissionPhase::WriteSubmit {
                tag: submit_tag, ..
            } => tag == *submit_tag,
            _ => false,
        };
        if is_submit && matches!(reply, Reply::Write(n) if *n as usize == submit.len()) {
            *accepted = true;
        }
        if let SubmissionPhase::WritePair {
            record_tag,
            submit_tag,
            record_ok,
            reply: submit_reply,
            ..
        } = phase
        {
            if tag == *record_tag {
                *record_ok = Some(matches!(reply, Reply::Write(n) if *n as usize == bytes.len()));
            } else if tag == *submit_tag {
                *submit_reply = Some(match reply {
                    Reply::Write(n) => SubmitReply::Write(*n),
                    Reply::Error(errno) => SubmitReply::Error(*errno),
                    _ => SubmitReply::Invalid,
                });
            } else {
                return Ok(false);
            }
            return Ok(true);
        }
        if let SubmissionPhase::Opening(opening) = phase {
            let result = opening.receive(&mut self.pipeline, tag, reply)?;
            if result == Some(true) {
                let fid = opening.fid;
                if self.reusable_transactions {
                    self.transaction_fid = Some(fid);
                }
                let tag = self.pipeline.write(fid, 0, bytes)?;
                *phase = SubmissionPhase::WriteRecord { tag, fid };
            }
            return Ok(result.is_some());
        }
        match *phase {
            SubmissionPhase::WriteRecord { tag: t, fid } if t == tag => match reply {
                Reply::Write(count) if *count as usize == bytes.len() => {
                    if submit.is_empty() {
                        *submit = encode_shell_file_submit(ShellFileSubmit {
                            connection_epoch: self.epoch,
                            submission_id: *submission_id,
                            candidate_bytes: bytes.len() as u32,
                        })?;
                    }
                    let submit_tag = self.pipeline.write(self.submit_fid, 0, submit)?;
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
            SubmissionPhase::WriteSubmit { tag: t, fid } if t == tag => {
                if matches!(reply, Reply::Error(_)) {
                    // The same drain may have buffered this submission's
                    // Submitted ahead of the error; settle from it first.
                    self.settle_buffered_submitted();
                }
                let Some(Current::Submission {
                    submitted, submit, ..
                }) = &self.current
                else {
                    return Ok(true);
                };
                let seen = *submitted;
                let exact = submit.len();
                match reply {
                    Reply::Write(count) if *count as usize == exact => {}
                    Reply::Write(_) => {
                        return Err(ShellClientError::Protocol("short submit write"));
                    }
                    Reply::Error(errno) if *errno == Errno::EALREADY && seen => {}
                    Reply::Error(errno) if *errno == Errno::EAGAIN => {
                        if seen {
                            return Err(ShellClientError::Protocol("EAGAIN after Submitted"));
                        }
                        let (progress, pass) = (self.progress, self.pass);
                        let not_before =
                            self.dispatch_time.unwrap_or_else(Instant::now) + self.next_backoff();
                        if let Some(Current::Submission { phase, .. }) = &mut self.current {
                            *phase = SubmissionPhase::RetryWait {
                                fid,
                                not_before,
                                progress,
                                pass,
                            };
                        }
                        return Ok(true);
                    }
                    Reply::Error(errno) if *errno == Errno::EALREADY => {
                        // A replay without observed custody: the outcome is
                        // unknown, and the connection fails closed.
                        self.settle_current(Custody::Unknown);
                        return Err(ShellClientError::Protocol("EALREADY without Submitted"));
                    }
                    Reply::Error(errno) if *errno == Errno::ESTALE => {
                        // Revocation; terminal classification settles the unit.
                        self.peer_closed = true;
                        return Ok(true);
                    }
                    Reply::Error(_) if seen => {
                        // Custody was already observed; a refusal now
                        // contradicts it. Custody stays settled as
                        // Submitted, and the connection fails closed.
                        return Err(ShellClientError::Protocol("submit refused after Submitted"));
                    }
                    Reply::Error(errno) => {
                        // A definitive refusal: nothing was journaled.
                        let errno = errno.0;
                        if let Some(Current::Submission {
                            kind: ShellFileKind::ResourceBegin,
                            bytes,
                            ..
                        }) = &self.current
                        {
                            let begin = sophia_shell_protocol::shell_files::decode_shell_file_resource_begin(bytes)?;
                            if let sophia_shell_protocol::ShellContentRecord::ResourceBegin(value) =
                                begin.record
                            {
                                let slot = &mut self.uploads[begin.slot as usize];
                                // No admission event will release this local claim.
                                // A queued payload then settles DroppedUnsent.
                                if slot.is_some_and(|upload| {
                                    upload.resource == value.resource
                                        && matches!(upload.state, super::UploadState::Pending)
                                }) {
                                    *slot = None;
                                }
                            }
                        }
                        self.forget(fid)?;
                        self.transaction_fid = None;
                        self.backoff = RETRY_FIRST;
                        self.settle_current(Custody::Refused(errno));
                        return Ok(true);
                    }
                    _ => return Err(ShellClientError::Protocol("unexpected submit reply")),
                }
                // `submit` accepted: clunk the transaction (a submitted
                // candidate is immutable; clunk cannot undo it) and settle
                // once `Submitted` has been observed too.
                if !self.reusable_transactions {
                    self.forget(fid)?;
                }
                self.backoff = RETRY_FIRST;
                if let Some(Current::Submission { phase, .. }) = &mut self.current {
                    *phase = SubmissionPhase::AwaitSubmitted;
                }
                if seen {
                    self.settle_current(Custody::Submitted);
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
