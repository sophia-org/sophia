use super::*;
#[derive(Default)]
pub(super) struct DrawQueue {
    pending: Option<(sophia_draw::Batch, Instant)>,
    last: Option<sophia_draw::Batch>,
}
impl ShellFiles {
    pub(super) fn has_draw_work(&self) -> bool {
        self.draw.last.is_some()
    }

    pub(in crate::shell_transport) fn write_draw(&mut self, bytes: &[u8]) -> Result<u32, Errno> {
        if self.owned_mode() {
            return Err(Errno::EACCES);
        }
        let limits = self
            .content_limits
            .as_ref()
            .filter(|_| self.negotiated && self.content && !self.revoked)
            .ok_or(Errno::EACCES)?;
        let batch = sophia_draw::Batch::decode(bytes).map_err(|_| Errno::EINVAL)?;
        if batch.grant != limits.grant {
            return Err(Errno::ESTALE);
        }
        if let Some(last) = &self.draw.last {
            if last == &batch {
                return Ok(bytes.len() as u32);
            }
            if batch.transaction <= last.transaction {
                return Err(EALREADY);
            }
        }
        if self.draw.pending.is_some() {
            return Err(Errno::EAGAIN);
        }
        // Rwrite moves command custody, not resource acceptance or presentation.
        // Keep the exact last batch so a lost Rwrite retry cannot execute twice.
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(u64::from(limits.transfer_timeout_ms)))
            .ok_or(Errno::EINVAL)?;
        self.draw.last = Some(batch.clone());
        self.draw.pending = Some((batch, deadline));
        Ok(bytes.len() as u32)
    }
    pub(in crate::shell_transport) fn pending_draw(
        &self,
    ) -> Option<&(sophia_draw::Batch, Instant)> {
        self.draw.pending.as_ref()
    }
    pub(in crate::shell_transport) fn take_draw(
        &mut self,
    ) -> Option<(sophia_draw::Batch, Instant)> {
        self.draw.pending.take()
    }
}
