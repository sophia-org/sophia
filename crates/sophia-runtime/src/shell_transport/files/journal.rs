//! The component's event journal: whole records, byte offsets, retention
//! until acknowledged. Credited responses own a terminal reserve that
//! unsolicited records can never consume.
use std::time::Instant;

use sophia_9p::journal::{Journal as Custody, JournalBounds as Limits};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::shell_files::{
    SHELL_FILE_MAX_JOURNAL_RECORDS, SHELL_FILE_TERMINAL_RESERVE_RECORDS, ShellFileAck,
    ShellFileHeader, ShellFileKind, encode_shell_file_record,
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
}

impl Journal {
    pub(in crate::shell_transport) fn new(epoch: u64, bounds: JournalBounds, now: Instant) -> Self {
        Self {
            custody: Custody::new(epoch),
            bounds,
            last_progress: now,
        }
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
        self.custody.fits(size, self.limits(credited))
    }

    /// Appends one complete event. Every fallible check precedes any change:
    /// a refusal spends no sequence, offset or byte.
    pub(in crate::shell_transport) fn append(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<u64, Errno> {
        let bytes = encode_shell_file_record(
            ShellFileHeader {
                kind,
                connection_epoch: self.custody.epoch(),
                submission_id: 0,
                sequence: self.custody.next_sequence(),
            },
            body,
        )
        .map_err(|_| Errno::EINVAL)?;
        let limits = self.limits(credited);
        Ok(self.custody.prepare(bytes, limits)?.commit())
    }

    pub(in crate::shell_transport) fn read(
        &self,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.custody.read(offset, count)
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
