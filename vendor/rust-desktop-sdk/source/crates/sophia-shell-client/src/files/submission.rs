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

use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Tag};
use sophia_shell_protocol::shell_files::{
    ShellFileClass, ShellFileKind, ShellFileSubmit, decode_shell_file_record,
    encode_shell_file_submit,
};

use super::{Current, FileWire, O_RDWR, QueuedKind, RETRY_FIRST, RETRY_MAX, SubmissionPhase};
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
        {
            return Ok(false);
        }
        let bytes = output
            .front()
            .expect("pending mirrors the outbox one-for-one")
            .to_vec();
        let ticket = output.front_ticket().expect("the front unit has a ticket");
        self.pending.pop_front();
        self.custody.push((ticket, Custody::InFlight));
        match kind {
            QueuedKind::Record => {
                let header = decode_shell_file_record(&bytes, ShellFileClass::Candidate)?.header;
                let (submission_id, kind) = (header.submission_id, header.kind);
                let (tag, fid) = self.pipeline.walk(self.root, &[b"transaction"])?;
                self.current = Some(Current::Submission {
                    bytes,
                    submission_id,
                    kind,
                    ticket,
                    submit: Vec::new(),
                    phase: SubmissionPhase::Walk { tag, fid },
                    submitted: false,
                });
            }
            QueuedKind::SlotWrite { resource, offset } => {
                self.current = Some(Current::SlotWrite {
                    resource,
                    offset,
                    remaining: bytes,
                    tag: None,
                    ticket,
                    not_before: None,
                    wrote_any: false,
                });
            }
        }
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
            SubmissionPhase::WriteSubmit { .. } | SubmissionPhase::AwaitSubmitted
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
    pub(super) fn drive_submit_retry(&mut self, now: Instant) -> Result<bool, ShellClientError> {
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
        if self.pass == *pass || (self.progress == *progress && now < *not_before) {
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
            phase: SubmissionPhase::WriteSubmit { .. } | SubmissionPhase::AwaitSubmitted,
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
            ..
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
                    *submit = encode_shell_file_submit(ShellFileSubmit {
                        connection_epoch: self.epoch,
                        submission_id: *submission_id,
                        candidate_bytes: bytes.len() as u32,
                    })?;
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
                        let not_before = Instant::now() + self.next_backoff();
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
                        self.forget(fid)?;
                        self.backoff = RETRY_FIRST;
                        self.settle_current(Custody::Refused(errno));
                        return Ok(true);
                    }
                    _ => return Err(ShellClientError::Protocol("unexpected submit reply")),
                }
                // `submit` accepted: clunk the transaction (a submitted
                // candidate is immutable; clunk cannot undo it) and settle
                // once `Submitted` has been observed too.
                self.forget(fid)?;
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
