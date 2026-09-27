//! The bounded, nonblocking `poll_io` driving loop: one round writes queued
//! requests and reads replies via `Pipeline::poll`, dispatches whatever
//! completed, then lets every other state machine (outbound submission/slot
//! writes, object fetch, event processing, the `events` read and its acks)
//! take its next step, repeating while something keeps changing.
use std::collections::VecDeque;

use sophia_9p_client::pipeline::{PipelineError, Reply};
use sophia_9p_records::Tag;
use sophia_shell_protocol::shell_files::{ShellFileAck, encode_shell_file_ack};

use super::{FileWire, MAX_ROUNDS, is_disconnect};
use crate::wire::Inbound;
use crate::{ShellClientError, outbox::ClientOutbox};

impl FileWire {
    pub(crate) fn poll_io(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        if let Some(error) = &self.fatal {
            return Err(error.clone());
        }
        let result = self.drive(output, inbox);
        if let Err(error) = &result {
            self.fatal = Some(error.clone());
        }
        result
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
            self.drain_replies(inbox)?;
            if eof || self.pipeline.is_poisoned() {
                self.peer_closed = true;
                return Ok(());
            }
            let mut progressed = false;
            progressed |= self.start_next_outbound(output)?;
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
        let mut progressed = false;
        while let Some((tag, reply)) = self.pipeline.take_reply() {
            progressed = true;
            if self.forgettable.remove(&tag) {
                continue;
            }
            if Some(tag) == self.read_tag {
                self.on_read_reply(tag, reply)?;
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
                self.event_buf.extend_from_slice(&data);
                Ok(())
            }
            _ => Err(ShellClientError::Protocol("unexpected events reply")),
        }
    }

    fn on_ack_reply(&mut self, reply: &Reply) -> Result<(), ShellClientError> {
        self.ack_tag = None;
        match reply {
            Reply::Write(_) => Ok(()),
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
        if inbox.len() >= crate::MAX_QUEUED_FRAMES {
            return Ok(false);
        }
        let tag = self
            .pipeline
            .read(self.events_fid, self.read_offset, u32::MAX)?;
        self.read_tag = Some(tag);
        Ok(true)
    }

    /// Acknowledges cumulatively through the highest sequence whose typed
    /// unit is retained; never acks past what `handle_event` has committed.
    fn maybe_send_ack(&mut self) -> Result<bool, ShellClientError> {
        if self.ack_tag.is_some() || self.ack_ready <= self.acked {
            return Ok(false);
        }
        let sequence = self.ack_ready;
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
