//! Reusable transaction custody. The token outlives a record; the retained
//! bytes move through assembly and accepted custody without a second buffer.
use super::*;

pub(super) struct ReusableWriter {
    id: u64,
    buffer: Vec<u8>,
    refusal: Option<Errno>,
}

/// Record identity ignores the header sequence word (a lean commit's ACK).
fn same_record(staged: &[u8], bytes: &[u8]) -> bool {
    staged.len() == bytes.len()
        && staged.len() >= 32
        && staged[..24] == bytes[..24]
        && staged[32..] == bytes[32..]
}

impl ShellFiles {
    pub(super) fn submit_frame_record(&mut self, bytes: &[u8]) -> Result<u32, Errno> {
        let id = self.reuse_writer.as_ref().ok_or(Errno::ESTALE)?.id;
        // Expire and validate the existing token even for an accepted retry.
        self.read_reusable_transaction(id, 0, 0)?;
        if self.submission_revision() != SHELL_FRAME_REVISION {
            return Err(Errno::EACCES);
        }
        let record = decode_shell_file_record_at_revision(
            bytes,
            ShellFileClass::Candidate,
            SHELL_FRAME_REVISION,
        )
        .map_err(|_| Errno::EINVAL)?;
        if record.header.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if record.header.kind != ShellFileKind::FrameCommit {
            return Err(Errno::EINVAL);
        }
        // Lean: the header word is a cumulative ACK, applied before admission
        // and refusing the whole write before any effect; not part of identity.
        let lean = self.lean();
        if record.header.sequence != 0 {
            if !lean {
                return Err(Errno::EINVAL);
            }
            self.apply_ack(sophia_protocol::shell_files::ShellFileAck {
                connection_epoch: record.header.connection_epoch,
                sequence: record.header.sequence,
            })?;
        }
        let submit = encode_shell_file_submit(ShellFileSubmit {
            connection_epoch: record.header.connection_epoch,
            submission_id: record.header.submission_id,
            candidate_bytes: u32::try_from(bytes.len()).map_err(|_| Errno::EINVAL)?,
        })
        .map_err(|_| Errno::EINVAL)?;
        if let Some(accepted) = &self.accepted {
            // The submit header alone cannot distinguish a changed payload of equal length.
            if accepted.handle != id || !same_record(&accepted.bytes, bytes) {
                return Err(EBUSY);
            }
        } else if !self
            .staging
            .as_ref()
            .is_some_and(|staging| staging.handle == id && same_record(&staging.bytes, bytes))
        {
            self.write_reusable_transaction(id, 0, bytes)?;
            if lean && let Some(staging) = self.staging.as_mut() {
                staging.bytes[24..32].fill(0);
            }
        }
        self.submit_transaction(&submit, lean)?;
        Ok(bytes.len() as u32)
    }

    /// The cumulative journal ACK, from `ack` or a lean commit's header word.
    pub(super) fn apply_ack(
        &mut self,
        ack: sophia_protocol::shell_files::ShellFileAck,
    ) -> Result<(), Errno> {
        self.journal.ack(ack, Instant::now())?;
        // A pending image refusal retries from ACK progress; its
        // outcome never fails the acknowledgement itself.
        let _ = self.retry_pending_image_refusal();
        if self
            .accepted
            .as_ref()
            .is_some_and(|a| a.sequence <= ack.sequence)
        {
            self.recycle_transaction();
        }
        Ok(())
    }

    pub(super) fn open_reusable_transaction(&mut self) -> Result<Handle, Errno> {
        self.expire();
        if self.reuse_writer.is_some() || self.staging.is_some() || self.accepted.is_some() {
            return Err(EBUSY);
        }
        let id = self.allocate_qid()?;
        self.reuse_writer = Some(ReusableWriter {
            id,
            // Reserve the existing assembly allowance once. This also prevents
            // geometric growth beyond that allowance on fragmented writes.
            buffer: Vec::with_capacity(STAGING.max_bytes),
            refusal: None,
        });
        Ok(Handle::ReusableTransaction(id))
    }

    pub(super) fn read_reusable_transaction(
        &mut self,
        id: u64,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.expire();
        let writer = self
            .reuse_writer
            .as_ref()
            .filter(|w| w.id == id)
            .ok_or(Errno::ESTALE)?;
        let bytes = self
            .staging
            .as_ref()
            .filter(|s| s.handle == id)
            .map(|s| &s.bytes)
            .or_else(|| {
                self.accepted
                    .as_ref()
                    .filter(|a| a.handle == id)
                    .map(|a| &a.bytes)
            })
            .unwrap_or(&writer.buffer);
        Ok(slice(bytes, offset, count))
    }

    pub(super) fn write_reusable_transaction(
        &mut self,
        id: u64,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        let writer = self
            .reuse_writer
            .as_mut()
            .filter(|w| w.id == id)
            .ok_or(Errno::ESTALE)?;
        if writer.refusal.is_some() || self.accepted.is_some() {
            return Err(EBUSY);
        }
        if let Some(staging) = self.staging.as_mut() {
            debug_assert_eq!(staging.handle, id);
            return staging.write(offset, data, Instant::now());
        }
        if offset != 0 {
            return Err(Errno::EINVAL);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let mut staging = Staging::new(id, STAGING);
        staging.bytes = std::mem::take(&mut writer.buffer);
        match staging.write(offset, data, Instant::now()) {
            Ok(count) => {
                self.staging = Some(staging);
                Ok(count)
            }
            Err(error) => {
                writer.buffer = staging.bytes;
                Err(error)
            }
        }
    }

    /// `lean`: a complete commit under bit 20, settled by its Rwrite alone.
    pub(super) fn submit_transaction(&mut self, data: &[u8], lean: bool) -> Result<(), Errno> {
        let submit = decode_shell_file_submit(data).map_err(|_| Errno::EINVAL)?;
        if submit.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if let Some(error) = self.reuse_writer.as_ref().and_then(|w| w.refusal) {
            return Err(error);
        }
        let result = self.submit_decoded(submit, lean);
        if let Err(error) = result
            && error != Errno::EAGAIN
            && let Some(writer) = self.reuse_writer.as_mut()
            && self.staging.as_ref().is_some_and(|s| s.handle == writer.id)
        {
            writer.refusal = Some(error);
        }
        result
    }

    pub(super) fn recycle_transaction(&mut self) {
        let Some(mut accepted) = self.accepted.take() else {
            return;
        };
        if let Some(writer) = self
            .reuse_writer
            .as_mut()
            .filter(|w| w.id == accepted.handle)
        {
            accepted.bytes.clear();
            writer.buffer = accepted.bytes;
        }
    }

    pub(super) fn release_reusable_transaction(&mut self, id: u64) {
        if self.reuse_writer.as_ref().is_some_and(|w| w.id == id) {
            self.reuse_writer = None;
            if self.staging.as_ref().is_some_and(|s| s.handle == id) {
                self.staging = None;
            }
        }
        // Accepted custody survives clunk until its covering ACK or teardown.
    }
}
