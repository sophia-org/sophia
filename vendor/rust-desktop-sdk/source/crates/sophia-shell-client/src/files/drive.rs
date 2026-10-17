//! The bounded, nonblocking `poll_io` driving loop: one round writes queued
//! requests and reads replies via `Pipeline::poll`, dispatches whatever
//! completed, then lets every other state machine (outbound submission/slot
//! writes, object fetch, event processing, the `events` read and its acks)
//! take its next step, repeating while something keeps changing.
use std::collections::VecDeque;
use std::os::fd::{AsFd, BorrowedFd};

use sophia_9p_client::pipeline::{IoBudget, PipelineError, Reply};
use sophia_9p_records::Tag;
use sophia_shell_protocol::shell_files::{
    SHELL_FILE_ACK_BYTES, ShellFileAck, encode_shell_file_ack,
};

use super::{Current, FileWire, MAX_ROUNDS, SubmissionPhase, is_disconnect};
use crate::custody::{Custody, Ledger};
use crate::wire::Inbound;
use crate::{ShellClientError, outbox::ClientOutbox};

impl FileWire {
    pub(crate) fn journal_progress(&self) -> crate::JournalProgress {
        let live = self.fatal.is_none() && !self.peer_closed;
        crate::JournalProgress {
            handled: self.ack_ready,
            ack_limit: self.ack_limit(),
            queued: self.acked,
            acknowledged: self.ack_confirmed,
            ack_pending: live && self.ack_tag.is_some(),
            object_pending: live && self.holds.iter().any(Option::is_some),
        }
    }
    pub(crate) fn set_automatic_journal_ack(
        &mut self,
        enabled: bool,
    ) -> Result<(), ShellClientError> {
        self.io_interest()?;
        self.automatic_ack = enabled;
        self.service_pending = true;
        Ok(())
    }
    pub(crate) fn acknowledge_journal(&mut self) -> Result<bool, ShellClientError> {
        self.io_interest()?;
        self.queue_ack()
    }
    pub(crate) fn close(&mut self, output: &mut ClientOutbox, ledger: &mut Ledger) {
        if self.fatal.is_some() {
            return;
        }
        self.observe_custody_after_close();
        self.peer_closed = true;
        self.fatal = Some(ShellClientError::PeerClosed);
        self.service_pending = false;
        output.release_reservation();
        self.classify_terminal(output);
        self.settle(output, ledger);
    }
    pub(crate) fn as_fd(&self) -> BorrowedFd<'_> {
        self.pipeline.as_fd()
    }

    pub(crate) fn io_interest(
        &self,
    ) -> Result<sophia_9p_client::pipeline::IoInterest, ShellClientError> {
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        if self.peer_closed {
            return Err(ShellClientError::PeerClosed);
        }
        let mut interest = self.pipeline.io_interest()?;
        interest.immediate |= self.service_pending;
        Ok(interest)
    }

    pub(crate) fn poll_io(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
        ledger: &mut Ledger,
        dispatch: Option<(std::time::Instant, usize)>,
    ) -> Result<(), ShellClientError> {
        if let Some((now, _)) = dispatch
            && (self.last_dispatch_time.is_some_and(|last| now < last)
                || now.checked_add(super::RETRY_MAX).is_none())
        {
            return Err(ShellClientError::InvalidArgument("dispatch time"));
        }
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        self.dispatch_time = dispatch.map(|(now, _)| now);
        if let Some(now) = self.dispatch_time {
            self.last_dispatch_time = Some(now);
        }
        self.pass += 1;
        let result = self.drive(
            output,
            inbox,
            dispatch.map(|(_, bytes)| IoBudget::new(bytes)),
        );
        if let Err(error) = &result {
            self.fatal = Some(error.clone());
            // Buffered events are still trusted unless one of them caused
            // the failure: a Submitted among them settles custody.
            self.observe_custody_after_close();
        }
        if result.is_err() || self.peer_closed {
            output.release_reservation();
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
        if matches!(self.current, Some(Current::SlotWrite(_))) {
            let Some(Current::SlotWrite(window)) = self.current.take() else {
                unreachable!()
            };
            for unit in window.units.into_iter().flatten() {
                self.custody.push((unit.ticket, unit.terminal_custody()));
                self.retire += 1;
            }
        }
        if self.retire > 0 || self.current.is_some() || !self.pending.is_empty() {
            let current = match &self.current {
                Some(Current::Submission {
                    submitted: true, ..
                })
                | Some(Current::Submission { accepted: true, .. }) => Some(Custody::Submitted),
                Some(Current::Submission {
                    phase:
                        SubmissionPhase::WritePair { .. }
                        | SubmissionPhase::WriteSubmit { .. }
                        | SubmissionPhase::AwaitSubmitted,
                    ..
                }) => Some(Custody::Unknown),
                Some(Current::Submission { .. }) => Some(Custody::DroppedUnsent),
                Some(Current::SlotWrite(_)) => unreachable!("classified above"),
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
        if self.fatal.is_some() || self.peer_closed {
            None
        } else {
            self.retry_deadline()
        }
    }

    pub(crate) fn wait_for_io(&self, maximum: std::time::Duration) -> Result<(), ShellClientError> {
        if self.service_pending || self.peer_closed {
            return Ok(());
        }
        self.pipeline.wait_for_io(maximum).map_err(Into::into)
    }

    fn drive(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
        mut budget: Option<IoBudget>,
    ) -> Result<(), ShellClientError> {
        self.service_pending = true;
        for round in 0..MAX_ROUNDS {
            let mut eof = false;
            let result = match &mut budget {
                Some(budget) => self.pipeline.poll_with_budget(budget),
                None => self.pipeline.poll(),
            };
            match result {
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
            let paired = self.finish_write_pair()?;
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
            let mut progressed = drained || paired;
            for _ in 0..std::mem::take(&mut self.retire) {
                output.retire_front();
            }
            // Queue the covering ACK before the next writer use. Otherwise
            // gather consumed events until this bounded drive is about to yield.
            if self.current.is_none()
                && matches!(self.pending.front(), Some(super::QueuedKind::Record))
            {
                progressed |= self.maybe_send_ack()?;
            }
            progressed |= self.start_next_outbound(output)?;
            progressed |= self.queue_submit_with_record()?;
            progressed |= self.drive_submit_retry()?;
            progressed |= self.drive_slot_write(output)?;
            progressed |= self.drive_object_fetch()?;
            progressed |= self.drive_image_fetch()?;
            progressed |= self.process_buffered_event(inbox)?;
            progressed |= self.maybe_read_events(inbox)?;
            if !progressed
                || budget.as_ref().is_some_and(IoBudget::exhausted)
                || round + 1 == MAX_ROUNDS
            {
                progressed |= self.maybe_send_ack()?;
            }
            if !progressed {
                self.service_pending = budget.as_ref().is_some_and(IoBudget::exhausted);
                break;
            }
            if budget.as_ref().is_some_and(IoBudget::exhausted) {
                break;
            }
        }
        Ok(())
    }

    fn drain_replies(&mut self, inbox: &mut VecDeque<Inbound>) -> Result<bool, ShellClientError> {
        let mut replies = std::mem::take(&mut self.reply_scratch);
        let result = self.drain_replies_into(inbox, &mut replies);
        // Errors must not retain replies or change their custody. Keep only
        // the bounded outer storage for the next successful drain.
        replies.clear();
        self.reply_scratch = replies;
        result
    }

    fn drain_replies_into(
        &mut self,
        inbox: &mut VecDeque<Inbound>,
        replies: &mut Vec<(Tag, Reply)>,
    ) -> Result<bool, ShellClientError> {
        // Buffer the events this drain carries before judging any other
        // reply: a submit error must see a Submitted that arrived in the same
        // drain, whichever order the two replies came in. Buffering events
        // only appends; handling them stays strictly in order.
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
        for (tag, reply) in replies.drain(..) {
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
            if self.on_image_fetch_reply(tag, &reply)? {
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
            Reply::Write(count) if *count as usize == SHELL_FILE_ACK_BYTES => {
                self.ack_confirmed = self.acked;
                Ok(())
            }
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
        if !self.automatic_ack {
            return Ok(false);
        }
        self.queue_ack()
    }
    fn queue_ack(&mut self) -> Result<bool, ShellClientError> {
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
