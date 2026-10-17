use super::{Command, Error, RequestId, Session, TransferState, UploadPhase, UploadSlot};
use crate::protocol::requester::Candidate;
use crate::requester::{
    Error as StateError,
    transaction::Transaction,
    upload::{Operation, Upload},
};
use sophia_9p_records::Fid;

impl Session {
    /// Takes the caller's encoded payload for one grant. This consumes the Vec
    /// even on admission error; use begin_buffer to retain rejected storage. Requires
    /// four request slots and six fids for journal/ack/transaction/writer overlap.
    /// The server decides whether this grant's transfer kind has an executor.
    pub fn begin(
        &mut self,
        id: RequestId,
        mut bytes: Vec<u8>,
        now: u64,
        deadline: u64,
    ) -> Result<(), Error> {
        self.begin_buffer(id, &mut bytes, now, deadline)
    }
    /// Takes the Vec only after local admission checks pass, leaving an empty
    /// Vec without allocating or copying. Rejection preserves the original bytes
    /// and capacity. After admission, including a subsequent transport error,
    /// recover storage with take_upload; the buffer is then owned by the session.
    /// An empty payload is valid: use upload_written(id) to query admission.
    pub fn begin_buffer(
        &mut self,
        id: RequestId,
        bytes: &mut Vec<u8>,
        now: u64,
        deadline: u64,
    ) -> Result<(), Error> {
        self.advance(now)?;
        if !self.ready()
            || !self.transaction.done()
            || self.pending_request.is_some()
            || self.upload.is_some()
        {
            return Err(StateError::Busy.into());
        }
        let opened = self.opened.as_ref().ok_or(StateError::Closed)?;
        if opened.pipeline.limits().max_outstanding < 4 || opened.pipeline.limits().max_fids < 6 {
            return Err(StateError::Invalid.into());
        }
        let slot = self
            .transfers
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.id == id))
            .ok_or(StateError::Invalid)?;
        let tracked = self.transfers[slot].as_ref().ok_or(StateError::Invalid)?;
        if tracked.begun
            || tracked.cancel_pending
            || tracked.transfer.state() != TransferState::Allowed
        {
            return Err(StateError::Busy.into());
        }
        let length = bytes.len() as u64;
        let submission = self
            .last_submission
            .checked_add(1)
            .ok_or(StateError::Overflow)?;
        let transaction = Transaction::new(
            self.epoch,
            submission,
            Candidate::Begin {
                transfer: tracked.transfer.id(),
                total_bytes: length,
            },
            now,
            deadline,
            opened.limits.assembly_timeout_ms,
        )?;
        let writer = Upload::take(bytes, opened.limits.max_payload_bytes, now, deadline)?;
        self.transfers[slot]
            .as_mut()
            .ok_or(StateError::Invalid)?
            .begun = true;
        self.upload = Some(UploadSlot {
            id,
            writer,
            phase: UploadPhase::Begin,
            length,
        });
        self.last_submission = submission;
        self.transaction = transaction;
        self.command = Command::Begin;
        self.releasing = false;
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        Ok(())
    }
    pub fn upload_written(&self, id: RequestId) -> Option<usize> {
        self.upload
            .as_ref()
            .filter(|u| u.id == id)
            .map(|u| u.writer.written())
    }
    pub fn upload_ready(&self, id: RequestId) -> bool {
        self.error.is_none()
            && self
                .upload
                .as_ref()
                .is_some_and(|u| u.id == id && u.phase == UploadPhase::Writing && u.writer.held())
    }
    pub fn end(&mut self, id: RequestId, now: u64, deadline: u64) -> Result<(), Error> {
        self.advance(now)?;
        if !self.ready()
            || !self.transaction.done()
            || self.pending_request.is_some()
            || !self.upload_ready(id)
        {
            return Err(StateError::Busy.into());
        }
        let tracked = self
            .transfers
            .iter()
            .flatten()
            .find(|t| t.id == id)
            .ok_or(StateError::Invalid)?;
        if tracked.cancel_pending || tracked.transfer.state() != TransferState::Allowed {
            return Err(StateError::Busy.into());
        }
        let upload = self.upload.as_ref().ok_or(StateError::Invalid)?;
        let submission = self
            .last_submission
            .checked_add(1)
            .ok_or(StateError::Overflow)?;
        let timeout = self
            .opened
            .as_ref()
            .ok_or(StateError::Closed)?
            .limits
            .assembly_timeout_ms;
        let transaction = Transaction::new(
            self.epoch,
            submission,
            Candidate::End {
                transfer: tracked.transfer.id(),
                total_bytes: upload.length,
            },
            now,
            deadline,
            timeout,
        )?;
        self.upload.as_mut().ok_or(StateError::Invalid)?.phase = UploadPhase::Ending;
        self.last_submission = submission;
        self.transaction = transaction;
        self.command = Command::End;
        self.releasing = false;
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        Ok(())
    }
    /// Returns original storage after terminal settlement plus writer cleanup,
    /// or after failure closed the role connection. Does not release the retained
    /// transfer outcome, which still requires explicit acknowledgement/release.
    pub fn take_upload(&mut self, id: RequestId) -> Result<Vec<u8>, Error> {
        let upload = self.upload.as_ref().ok_or(StateError::Invalid)?;
        if upload.id != id {
            return Err(StateError::Invalid.into());
        }
        if self.error.is_none() && (!upload.writer.done() || self.upload_tag.is_some()) {
            return Err(StateError::Busy.into());
        }
        Ok(self
            .upload
            .take()
            .ok_or(StateError::Invalid)?
            .writer
            .into_buffer())
    }
    pub(super) fn drive_upload(&mut self) -> Result<(), Error> {
        if self.upload_tag.is_some() {
            return Ok(());
        }
        let Some(upload) = &mut self.upload else {
            return Ok(());
        };
        if upload.phase == UploadPhase::Begin {
            return Ok(());
        }
        let tracked = self
            .transfers
            .iter()
            .flatten()
            .find(|t| t.id == upload.id)
            .ok_or(StateError::Protocol)?;
        if tracked.cancel_pending && upload.phase != UploadPhase::Settling {
            return Ok(());
        }
        let opened = self.opened.as_mut().ok_or(StateError::Closed)?;
        let wire = &mut opened.pipeline;
        let Some(op) = upload.writer.operation(self.now, wire.msize())? else {
            return Ok(());
        };
        let mut fid = Fid::NOFID;
        let tag = match op {
            Operation::Walk => {
                let (tag, walked) = wire.walk(opened.root, &[b"upload", b"0"])?;
                fid = walked;
                tag
            }
            Operation::Open(fid) => wire.lopen(fid, 1)?,
            Operation::Write { fid, offset, bytes } => wire.write(fid, offset, bytes)?,
            Operation::Clunk(fid) => wire.clunk(fid)?,
        };
        upload.writer.issued(self.now, wire.msize(), fid)?;
        self.upload_tag = Some(tag);
        Ok(())
    }
}
