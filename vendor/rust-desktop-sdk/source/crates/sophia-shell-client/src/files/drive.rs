//! The bounded, nonblocking `poll_io` driving loop: one round writes queued
//! requests and reads replies via `Pipeline::poll`, dispatches whatever
//! completed, then lets every other state machine (outbound submission/slot
//! writes, object fetch, event processing, the `events` read and its acks)
//! take its next step, repeating while something keeps changing.
use std::collections::VecDeque;

use sophia_9p_client::pipeline::{PipelineError, Reply};
use sophia_9p_records::Tag;
use sophia_shell_protocol::shell_files::{
    SHELL_FILE_ACK_BYTES, ShellFileAck, encode_shell_file_ack,
};

use super::{Current, FileWire, MAX_ROUNDS, SubmissionPhase, is_disconnect};
use crate::custody::{Custody, Ledger};
use crate::wire::Inbound;
use crate::{ShellClientError, outbox::ClientOutbox};

impl FileWire {
    pub(crate) fn poll_io(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
        ledger: &mut Ledger,
    ) -> Result<(), ShellClientError> {
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        self.pass += 1;
        let result = self.drive(output, inbox);
        if let Err(error) = &result {
            self.fatal = Some(error.clone());
            // Buffered events are still trusted unless one of them caused
            // the failure: a Submitted among them settles custody.
            self.observe_custody_after_close();
        }
        if result.is_err() || self.peer_closed {
            self.classify_terminal(output);
        }
        self.settle(output, ledger);
        result
    }

    /// Records custody changes and releases settled front units.
    fn settle(&mut self, output: &mut ClientOutbox, ledger: &mut Ledger) {
        for (ticket, custody) in self.custody.drain(..) {
            ledger.set(ticket, custody);
        }
        for _ in 0..std::mem::take(&mut self.retire) {
            output.retire_front();
        }
    }

    /// The connection is over: settle every unit still held. A submission
    /// whose `Submitted` was observed keeps that; one whose `submit` was on
    /// the wire is unknown; resource bytes partly or possibly written are
    /// unknown; everything else never left the client.
    fn classify_terminal(&mut self, output: &ClientOutbox) {
        if self.retire > 0 || self.current.is_some() || !self.pending.is_empty() {
            let current = match &self.current {
                Some(Current::Submission {
                    submitted: true, ..
                }) => Some(Custody::Submitted),
                Some(Current::Submission {
                    phase: SubmissionPhase::WriteSubmit { .. } | SubmissionPhase::AwaitSubmitted,
                    ..
                }) => Some(Custody::Unknown),
                Some(Current::Submission { .. }) => Some(Custody::DroppedUnsent),
                Some(Current::SlotWrite { tag, wrote_any, .. }) => {
                    Some(if tag.is_some() || *wrote_any {
                        Custody::Unknown
                    } else {
                        Custody::DroppedUnsent
                    })
                }
                None => None,
            };
            // Units already settled this pass sit ahead of the current one.
            let skip = self.retire + usize::from(current.is_some());
            if let Some(custody) = current {
                self.settle_current(custody);
            }
            let unsent: Vec<u64> = output.tickets().skip(skip).collect();
            for ticket in unsent {
                self.custody.push((ticket, Custody::DroppedUnsent));
            }
            self.pending.clear();
        }
    }

    /// The instant a pending `EAGAIN` retry becomes due without event
    /// progress; the caller should service the connection by then.
    pub(crate) fn wake_deadline(&self) -> Option<std::time::Instant> {
        self.retry_deadline()
    }

    fn drive(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        for _ in 0..MAX_ROUNDS {
            let mut eof = false;
            match self.pipeline.poll() {
                Ok(()) => {}
                Err(PipelineError::Io(kind)) if is_disconnect(kind) => {
                    eof = true;
                }
                Err(error) => return Err(error.into()),
            }
            // Recover any reply `poll` resolved before hitting that EOF (see
            // `connect::wait_ok`'s doc comment for why one may still be
            // sitting in the pipeline's completed queue) before treating the
            // peer as gone.
            // A reply handler may queue the next request (an open after a
            // walk); the next round's `poll` writes it, in this same pass.
            let drained = self.drain_replies(inbox)?;
            if eof || self.peer_closed || self.pipeline.is_poisoned() {
                self.peer_closed = true;
                // Whatever complete events already arrived still count
                // (a final `Submitted` among them settles custody), each
                // under the same checks as ever; nothing past a bad one.
                while self.process_buffered_event(inbox)? {}
                // Events the drain could not reach (a fetch holds them back,
                // or the inbox is full) still settle custody.
                self.observe_custody_after_close();
                return Ok(());
            }
            let mut progressed = drained;
            for _ in 0..std::mem::take(&mut self.retire) {
                output.retire_front();
            }
            progressed |= self.start_next_outbound(output)?;
            progressed |= self.drive_submit_retry(std::time::Instant::now())?;
            progressed |= self.drive_slot_write()?;
            progressed |= self.drive_object_fetch()?;
            progressed |= self.process_buffered_event(inbox)?;
            progressed |= self.maybe_read_events(inbox)?;
            progressed |= self.maybe_send_ack()?;
            if !progressed {
                break;
            }
        }
        Ok(())
    }

    fn drain_replies(&mut self, inbox: &mut VecDeque<Inbound>) -> Result<bool, ShellClientError> {
        // Buffer the events this drain carries before judging any other
        // reply: a submit error must see a Submitted that arrived in the same
        // drain, whichever order the two replies came in. Buffering events
        // only appends; handling them stays strictly in order.
        let mut replies = Vec::new();
        let mut read = false;
        while let Some((tag, reply)) = self.pipeline.take_reply() {
            if Some(tag) == self.read_tag {
                self.on_read_reply(tag, reply)?;
                read = true;
            } else {
                replies.push((tag, reply));
            }
        }
        let progressed = read || !replies.is_empty();
        for (tag, reply) in replies {
            if self.forgettable.remove(&tag) {
                continue;
            }
            if Some(tag) == self.ack_tag {
                self.on_ack_reply(&reply)?;
                continue;
            }
            if self.on_submission_reply(tag, &reply)? {
                continue;
            }
            if self.on_slot_open_reply(tag, &reply)? {
                continue;
            }
            if self.on_slot_write_reply(tag, &reply)? {
                continue;
            }
            if self.on_object_fetch_reply(tag, &reply, inbox)? {
                continue;
            }
            return Err(ShellClientError::Protocol("reply to an untracked tag"));
        }
        Ok(progressed)
    }

    fn on_read_reply(&mut self, tag: Tag, reply: Reply) -> Result<(), ShellClientError> {
        debug_assert_eq!(Some(tag), self.read_tag);
        self.read_tag = None;
        match reply {
            Reply::Read(data) => {
                self.read_offset += data.len() as u64;
                self.event_buf
                    .try_reserve_exact(data.len())
                    .map_err(|_| ShellClientError::Protocol("event buffer allocation"))?;
                self.event_buf.extend_from_slice(&data);
                Ok(())
            }
            _ => Err(ShellClientError::Protocol("unexpected events reply")),
        }
    }

    fn on_ack_reply(&mut self, reply: &Reply) -> Result<(), ShellClientError> {
        self.ack_tag = None;
        match reply {
            Reply::Write(count) if *count as usize == SHELL_FILE_ACK_BYTES => Ok(()),
            _ => Err(ShellClientError::Protocol("unexpected ack reply")),
        }
    }

    /// Keeps exactly one `Tread` outstanding on `events`, backing off (not
    /// issuing another) while the inbox has no room to retain what a new
    /// event might decode into.
    fn maybe_read_events(&mut self, inbox: &VecDeque<Inbound>) -> Result<bool, ShellClientError> {
        if self.read_tag.is_some() {
            return Ok(false);
        }
        if inbox.len() >= crate::MAX_QUEUED_FRAMES
            || self.event_buf.len() >= super::events::MAX_EVENT_BYTES
        {
            return Ok(false);
        }
        let tag = self
            .pipeline
            .read(self.events_fid, self.read_offset, u32::MAX)?;
        self.read_tag = Some(tag);
        Ok(true)
    }

    /// Acknowledges cumulatively through the highest sequence whose typed
    /// unit is retained; never past what `handle_event` has committed, nor
    /// past an announcement whose object is not yet fetched.
    fn maybe_send_ack(&mut self) -> Result<bool, ShellClientError> {
        let sequence = self.ack_limit();
        if self.ack_tag.is_some() || sequence <= self.acked {
            return Ok(false);
        }
        let bytes = encode_shell_file_ack(ShellFileAck {
            connection_epoch: self.epoch,
            sequence,
        })?;
        let tag = self.pipeline.write(self.ack_fid, 0, &bytes)?;
        self.ack_tag = Some(tag);
        self.acked = sequence;
        Ok(true)
    }
}
