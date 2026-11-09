//! Executable custody projection with a supplied ACK-effect callback.
//! Production journal and authorization are not wired into this test seam.
use crate::capture::{Captured, Identity};
use sophia_shell_protocol::shell_files::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Again,
    Invalid,
    Stale,
    Busy,
    Already,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Accepted { applied_ack: u64 },
    Refused(Refusal),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub identity: Identity,
    pub outcome: Outcome,
}

struct Staging {
    bytes: Vec<u8>,
    submission: u64,
    deadline: u64,
}

pub struct Owner {
    epoch: u64,
    writer: u64,
    live: bool,
    token: bool,
    next_sequence: u64,
    watermark: u64,
    staging: Option<Staging>,
    pending: Option<Receipt>,
}
impl Owner {
    pub fn new(epoch: u64, writer: u64) -> Self {
        assert!(
            epoch != 0 && writer != 0,
            "binding identities must be nonzero"
        );
        Self {
            epoch,
            writer,
            live: true,
            token: true,
            next_sequence: 1,
            watermark: 0,
            staging: None,
            pending: None,
        }
    }
    pub fn withdraw(&mut self) {
        self.live = false;
        self.pending = None;
    }
    pub fn expire_writer(&mut self) {
        self.token = false;
    }
    pub fn reopen_writer(&mut self, writer: u64) -> Result<(), Refusal> {
        if !self.live || self.pending.is_some() || writer <= self.writer {
            return Err(Refusal::Stale);
        }
        self.writer = writer;
        self.token = true;
        self.staging = None;
        Ok(())
    }
    pub fn watermark(&self) -> u64 {
        self.watermark
    }
    pub fn staging_deadline(&self) -> Option<u64> {
        self.staging.as_ref().map(|s| s.deadline)
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Reserve the one-deep receipt slot before invoking the ACK control path.
    /// `now` and `deadline` are monotonic test units, not a new production clock.
    pub fn process(
        &mut self,
        request: Captured,
        now: u64,
        deadline: u64,
        room: bool,
        mut apply_ack: impl FnMut(u64) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        if !self.live {
            return Err(Refusal::Stale);
        }
        if self.pending.is_some() {
            return Err(Refusal::Busy);
        }
        let id = request.identity;
        if id.sequence != self.next_sequence || id.sequence == u64::MAX {
            self.withdraw();
            return Err(Refusal::Invalid);
        }
        self.next_sequence += 1;
        // Placeholder reservation owns no journal or terminal capacity.
        self.pending = Some(Receipt {
            identity: id,
            outcome: Outcome::Refused(Refusal::Invalid),
        });
        let outcome = self.process_reserved(request, now, deadline, room, &mut apply_ack);
        if outcome.is_err() && outcome != Err(Refusal::Again) && self.staging.is_some() {
            self.token = false;
        }
        self.pending.as_mut().expect("reserved receipt").outcome = match outcome {
            Ok(()) => Outcome::Accepted {
                applied_ack: id.ack,
            },
            Err(error) => Outcome::Refused(error),
        };
        Ok(())
    }
    fn process_reserved(
        &mut self,
        request: Captured,
        now: u64,
        deadline: u64,
        room: bool,
        apply_ack: &mut impl FnMut(u64) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let id = request.identity;
        if !self.token || id.epoch != self.epoch || id.writer != self.writer {
            return Err(Refusal::Stale);
        }
        if self.staging.as_ref().is_some_and(|s| now >= s.deadline) {
            self.token = false;
            return Err(Refusal::Stale);
        }
        let mut bytes = request.bytes;
        if bytes.len() > SHELL_FILE_MAX_TRANSACTION_BYTES || bytes.len() != id.bytes as usize {
            return Err(Refusal::Invalid);
        }
        let record = decode_shell_file_record_at_revision(
            &bytes,
            ShellFileClass::Candidate,
            SHELL_FRAME_REVISION,
        )
        .map_err(|_| Refusal::Invalid)?;
        if record.header.kind != ShellFileKind::FrameCommit
            || record.header.connection_epoch != id.epoch
            || record.header.submission_id != id.submission
            || record.header.sequence != id.ack
        {
            return Err(Refusal::Invalid);
        }
        if id.ack != 0 {
            apply_ack(id.ack)?;
        }
        if id.submission <= self.watermark {
            return Err(Refusal::Already);
        }
        bytes[24..32].fill(0);
        if let Some(staged) = &self.staging {
            if staged.submission != id.submission || staged.bytes != bytes {
                return Err(Refusal::Busy);
            }
        } else {
            if now >= deadline {
                return Err(Refusal::Stale);
            }
            self.staging = Some(Staging {
                bytes,
                submission: id.submission,
                deadline,
            });
        }
        if !room {
            return Err(Refusal::Again);
        }
        let staged = self.staging.as_ref().expect("staging created");
        if !matches!(
            decode_shell_frame_record(
                &staged.bytes,
                ShellFileClass::Candidate,
                SHELL_FRAME_REVISION,
                SHELL_FILE_MAX_TRANSACTION_BYTES
            ),
            Ok(ShellFrameRecord::Commit(_))
        ) {
            return Err(Refusal::Invalid);
        }
        self.watermark = id.submission;
        self.staging = None;
        Ok(())
    }
    /// Proposed fresh publication gate; a withdrawal preserves accepted watermark.
    pub fn publish(&mut self, authority_live: bool) -> Option<Receipt> {
        if !self.live || !authority_live {
            self.withdraw();
            return None;
        }
        self.pending.take()
    }
}

#[derive(Default)]
pub struct Client {
    pending: Option<Identity>,
    pub known: Option<u64>,
    pub confirmed_ack: u64,
    pub unknown: bool,
    pub closed: bool,
    binding: Option<(u64, u64)>,
    needs_reopen: bool,
    standalone_ack: Option<u64>,
}
impl Client {
    pub fn begin(&mut self, id: Identity) -> Result<(), Refusal> {
        if self.closed || self.unknown {
            return Err(Refusal::Stale);
        }
        if self.pending.is_some() || self.standalone_ack.is_some() {
            return Err(Refusal::Busy);
        }
        if self.needs_reopen || self.binding.is_some_and(|b| b != (id.epoch, id.writer)) {
            return Err(Refusal::Stale);
        }
        self.binding = Some((id.epoch, id.writer));
        self.pending = Some(id);
        Ok(())
    }
    pub fn reopen_writer(&mut self, writer: u64) -> Result<(), Refusal> {
        if self.closed || self.unknown || self.pending.is_some() || !self.needs_reopen {
            return Err(Refusal::Stale);
        }
        let (epoch, old) = self.binding.ok_or(Refusal::Stale)?;
        if writer <= old {
            return Err(Refusal::Stale);
        }
        self.binding = Some((epoch, writer));
        self.needs_reopen = false;
        Ok(())
    }
    pub fn standalone_ack_allowed(&self) -> bool {
        !self.closed && !self.unknown && self.pending.is_none() && self.standalone_ack.is_none()
    }
    /// Mirrors the outstanding standalone 9P ACK, without replacing its handler.
    pub fn begin_ack(&mut self, word: u64) -> Result<(), Refusal> {
        if !self.standalone_ack_allowed() {
            return Err(Refusal::Busy);
        }
        self.standalone_ack = Some(word);
        Ok(())
    }
    pub fn finish_ack(&mut self, word: u64) -> Result<(), Refusal> {
        if self.closed || self.standalone_ack != Some(word) {
            self.disconnect();
            return Err(Refusal::Invalid);
        }
        self.standalone_ack = None;
        self.confirmed_ack = self.confirmed_ack.max(word);
        Ok(())
    }
    pub fn disconnect(&mut self) {
        self.unknown |= self.pending.is_some();
        self.closed = true;
    }
    pub fn observe(&mut self, receipt: Receipt) -> Result<Outcome, Refusal> {
        if self.closed || self.pending != Some(receipt.identity) {
            self.disconnect();
            return Err(Refusal::Invalid);
        }
        if let Outcome::Accepted { applied_ack } = receipt.outcome {
            if applied_ack != receipt.identity.ack {
                self.disconnect();
                return Err(Refusal::Invalid);
            }
            self.known = Some(receipt.identity.submission);
            self.confirmed_ack = self.confirmed_ack.max(applied_ack);
        }
        self.pending = None;
        match receipt.outcome {
            Outcome::Refused(Refusal::Stale) => self.needs_reopen = true,
            Outcome::Refused(Refusal::Again) | Outcome::Accepted { .. } => (),
            _ => self.closed = true,
        }
        Ok(receipt.outcome)
    }
}
