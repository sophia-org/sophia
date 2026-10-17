//! The component's event journal: whole records, byte offsets, retention
//! until acknowledged. Credited responses own a terminal reserve that
//! unsolicited records can never consume.
use std::time::Instant;

use sophia_9p::journal::{Journal as Custody, JournalBounds as Limits};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::shell_files::{
    SHELL_FILE_MAX_JOURNAL_RECORDS, SHELL_FILE_TERMINAL_RESERVE_RECORDS, ShellFileAck,
    ShellFileHeader, ShellFileKind, encode_shell_file_record_at_revision,
};

/// Byte bounds derived per role from its largest Session-to-client record
/// (docs/sophia-shell-files.md, per-role disclosure bounds).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) struct JournalBounds {
    pub bytes: usize,
    pub reserve_bytes: usize,
}

pub(in crate::shell_transport) struct Journal {
    custody: Custody,
    bounds: JournalBounds,
    last_progress: Instant,
    revision: u16,
    /// One held future record, stored as its total byte size; the record
    /// count is implicitly one. While held, ordinary credited appends
    /// leave one record and this many bytes free; only
    /// [`Self::append_consuming_reservation`] may spend it. Uncredited
    /// appends need no subtraction: the terminal reserve they already
    /// cannot touch is where a credited held record lives, so subtracting
    /// again would fence the same room twice. This is accounting, not a
    /// named slot: holding reserves capacity, it does not promise any
    /// particular record a sequence.
    held: Option<usize>,
}

impl Journal {
    pub(in crate::shell_transport) fn new(epoch: u64, bounds: JournalBounds, now: Instant) -> Self {
        Self {
            custody: Custody::new(epoch),
            bounds,
            last_progress: now,
            revision: 8,
            held: None,
        }
    }

    pub(super) fn enable_image_revision(&mut self) {
        self.revision = 9;
    }

    /// Unsolicited records leave the terminal reserve (records and bytes)
    /// free for credited responses.
    fn limits(&self, credited: bool) -> Limits {
        if credited {
            Limits {
                records: usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS),
                bytes: self.bounds.bytes,
            }
        } else {
            Limits {
                records: usize::from(
                    SHELL_FILE_MAX_JOURNAL_RECORDS - SHELL_FILE_TERMINAL_RESERVE_RECORDS,
                ),
                bytes: self.bounds.bytes.saturating_sub(self.bounds.reserve_bytes),
            }
        }
    }

    /// Whether a record of `size` bytes fits now.
    pub(in crate::shell_transport) fn fits(&self, size: usize, credited: bool) -> bool {
        self.custody.fits(size, self.append_limits(credited))
    }

    fn append_limits(&self, credited: bool) -> Limits {
        let mut limits = self.limits(credited);
        if credited && let Some(held) = self.held {
            limits.records = limits.records.saturating_sub(1);
            limits.bytes = limits.bytes.saturating_sub(held);
        }
        limits
    }

    /// Holds room for one future record of `size` total bytes. Fails if a
    /// reservation is already held or the record would not fit under
    /// credited bounds once held. While held, ordinary appends (credited
    /// or not) leave one record and `size` bytes free.
    pub(in crate::shell_transport) fn hold_reservation(&mut self, size: usize) -> bool {
        if self.held.is_some()
            || size > self.bounds.reserve_bytes
            || !self.custody.fits(size, self.limits(true))
        {
            return false;
        }
        self.held = Some(size);
        true
    }

    /// Drops a held reservation without spending it.
    pub(in crate::shell_transport) fn release_reservation(&mut self) {
        self.held = None;
    }

    /// Appends one complete event. Every fallible check precedes any change:
    /// a refusal spends no sequence, offset or byte.
    pub(in crate::shell_transport) fn append(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<u64, Errno> {
        let bytes = encode_shell_file_record_at_revision(
            ShellFileHeader {
                kind,
                connection_epoch: self.custody.epoch(),
                submission_id: 0,
                sequence: self.custody.next_sequence(),
            },
            body,
            self.revision,
        )
        .map_err(|_| Errno::EINVAL)?;
        // A held reservation is fenced from ordinary credited appends.
        // Uncredited appends already cannot touch the terminal reserve
        // the held record will spend, so they are not subtracted twice.
        let limits = self.append_limits(credited);
        Ok(self.custody.prepare(bytes, limits)?.commit())
    }

    /// Appends the record a reservation was held for, spending it. The
    /// encoded record must be exactly the held byte size; a mismatch keeps
    /// the reservation held and changes nothing.
    pub(in crate::shell_transport) fn append_consuming_reservation(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
    ) -> Result<u64, Errno> {
        // Reservations exist for terminal events; spending one on any
        // other class is a programming error, refused before any change.
        if sophia_protocol::shell_files::shell_file_class(kind)
            != sophia_protocol::shell_files::ShellFileClass::Event
        {
            return Err(Errno::EINVAL);
        }
        let held = self.held.ok_or(Errno::EINVAL)?;
        let bytes = encode_shell_file_record_at_revision(
            ShellFileHeader {
                kind,
                connection_epoch: self.custody.epoch(),
                submission_id: 0,
                sequence: self.custody.next_sequence(),
            },
            body,
            self.revision,
        )
        .map_err(|_| Errno::EINVAL)?;
        if bytes.len() != held {
            return Err(Errno::EINVAL);
        }
        let prepared = self.custody.prepare(bytes, self.limits(true))?;
        self.held = None;
        Ok(prepared.commit())
    }

    pub(in crate::shell_transport) fn read(
        &self,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.custody.read(offset, count)
    }

    pub(in crate::shell_transport) fn read_buffered(
        &self,
        offset: u64,
        count: u32,
        scratch: &mut Vec<u8>,
    ) -> Result<ReadOutcome, Errno> {
        self.custody.read_buffered(offset, count, scratch)
    }

    /// Releases retention through `ack.sequence`. Acknowledgement releases
    /// transport retention only; it never confirms a semantic outcome.
    pub(in crate::shell_transport) fn ack(
        &mut self,
        ack: ShellFileAck,
        now: Instant,
    ) -> Result<(), Errno> {
        if self.custody.ack(ack.connection_epoch, ack.sequence)? {
            self.last_progress = now;
        }
        Ok(())
    }

    /// The last acknowledgement advance, or the journal's creation.
    pub(in crate::shell_transport) fn last_progress(&self) -> Instant {
        self.last_progress
    }

    pub(in crate::shell_transport) fn records(&self) -> usize {
        self.custody.position().records
    }

    pub(in crate::shell_transport) fn size(&self) -> u64 {
        self.custody.size()
    }
}

#[cfg(test)]
#[path = "../../../tests/support/shell_file_journal.rs"]
mod tests;
