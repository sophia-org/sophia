//! Whole SFT records use the connection's existing custody and journal queues.
use crate::wire::Inbound;
use crate::{Admission, ContentReservation, ShellClientError, ShellConnection};
use sophia_shell_protocol::shell_files::ShellFrameRecord;

impl ShellConnection {
    /// Reserve one complete SFT request without I/O or consuming caller storage.
    /// Commit issues an ordinary custody ticket, not stream credit or acceptance.
    /// Dropping the reservation sends nothing; an uncertain submission must not
    /// be replayed on a replacement connection.
    ///
    /// # Errors
    /// Refuses missing r13 authority, invalid shape, closed transport or capacity.
    pub fn reserve_frame(
        &mut self,
        record: &ShellFrameRecord,
    ) -> Result<ContentReservation<'_>, ShellClientError> {
        if self.wire.peer_closed() {
            return Err(ShellClientError::PeerClosed);
        }
        let units = self.wire.encode_frame(record)?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare(units, false, &mut self.ledger)?;
        Ok(ContentReservation {
            connection: self,
            prepared: Some(prepared),
        })
    }

    /// Queue one whole SFT request; caller data remains available on refusal.
    /// No embedded source is separately submitted through the legacy image path.
    ///
    /// # Errors
    /// Returns the same failures as [`Self::reserve_frame`].
    pub fn enqueue_frame(
        &mut self,
        record: &ShellFrameRecord,
    ) -> Result<Admission, ShellClientError> {
        Ok(self.reserve_frame(record)?.commit())
    }

    /// Take the oldest buffered SFT event without driving the connection.
    /// Prepared and journal ACKs do not return frame credit. The owning stream
    /// must correlate the complete key and generation before using a terminal.
    ///
    /// # Errors
    /// Refuses missing r13 authority, or reports closure after buffered events drain.
    pub fn take_frame(&mut self) -> Result<Option<ShellFrameRecord>, ShellClientError> {
        self.wire.require_frame()?;
        let at = self
            .inbox
            .iter()
            .position(|item| matches!(item, Inbound::Frame(_)));
        if let Some(Inbound::Frame(value)) = at.and_then(|index| self.inbox.remove(index)) {
            return Ok(Some(value));
        }
        if self.wire.peer_closed() {
            Err(ShellClientError::PeerClosed)
        } else {
            Ok(None)
        }
    }
}
