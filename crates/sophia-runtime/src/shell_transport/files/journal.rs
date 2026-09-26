//! The component's event journal: whole records, byte offsets, retention
//! until acknowledged. Credited responses own a terminal reserve that
//! unsolicited records can never consume.
use std::collections::VecDeque;
use std::time::Instant;

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

struct Record {
    sequence: u64,
    start: u64,
    bytes: Vec<u8>,
}

pub(in crate::shell_transport) struct Journal {
    epoch: u64,
    bounds: JournalBounds,
    records: VecDeque<Record>,
    bytes: usize,
    tail: u64,
    floor: u64,
    next: u64,
    acknowledged: u64,
    last_progress: Instant,
}

impl Journal {
    pub(in crate::shell_transport) fn new(epoch: u64, bounds: JournalBounds, now: Instant) -> Self {
        Self {
            epoch,
            bounds,
            records: VecDeque::with_capacity(usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS)),
            bytes: 0,
            tail: 0,
            floor: 0,
            next: 1,
            acknowledged: 0,
            last_progress: now,
        }
    }

    /// Whether a record of `size` bytes fits now. Unsolicited records leave
    /// the terminal reserve (records and bytes) free for credited responses.
    pub(in crate::shell_transport) fn fits(&self, size: usize, credited: bool) -> bool {
        let (records, bytes) = if credited {
            (
                usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS),
                self.bounds.bytes,
            )
        } else {
            (
                usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS - SHELL_FILE_TERMINAL_RESERVE_RECORDS),
                self.bounds.bytes.saturating_sub(self.bounds.reserve_bytes),
            )
        };
        self.records.len() < records && size <= bytes.saturating_sub(self.bytes)
    }

    /// Appends one complete event. Every fallible check precedes any change:
    /// a refusal spends no sequence, offset or byte.
    pub(in crate::shell_transport) fn append(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<u64, Errno> {
        let sequence = self.next;
        let bytes = encode_shell_file_record(
            ShellFileHeader {
                kind,
                connection_epoch: self.epoch,
                submission_id: 0,
                sequence,
            },
            body,
        )
        .map_err(|_| Errno::EINVAL)?;
        if !self.fits(bytes.len(), credited) {
            return Err(Errno::EAGAIN);
        }
        let next = self.next.checked_add(1).ok_or(Errno::ENOSPC)?;
        let tail = self
            .tail
            .checked_add(bytes.len() as u64)
            .ok_or(Errno::ENOSPC)?;
        self.bytes += bytes.len();
        self.records.push_back(Record {
            sequence,
            start: self.tail,
            bytes,
        });
        self.next = next;
        self.tail = tail;
        Ok(sequence)
    }

    pub(in crate::shell_transport) fn read(
        &self,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        if offset < self.floor {
            return Err(Errno::ESTALE);
        }
        if offset > self.tail {
            return Err(Errno::EINVAL);
        }
        if count == 0 {
            return Ok(ReadOutcome::Ready(Vec::new()));
        }
        if offset == self.tail {
            return Ok(ReadOutcome::Pending);
        }
        let mut bytes = Vec::new();
        for record in &self.records {
            let end = record.start + record.bytes.len() as u64;
            if offset >= end {
                continue;
            }
            let start = offset.saturating_sub(record.start) as usize;
            let take = (count as usize - bytes.len()).min(record.bytes.len() - start);
            bytes.extend_from_slice(&record.bytes[start..start + take]);
            if bytes.len() == count as usize {
                break;
            }
        }
        Ok(ReadOutcome::Ready(bytes))
    }

    /// Releases retention through `ack.sequence`. Acknowledgement releases
    /// transport retention only; it never confirms a semantic outcome.
    pub(in crate::shell_transport) fn ack(
        &mut self,
        ack: ShellFileAck,
        now: Instant,
    ) -> Result<(), Errno> {
        if ack.connection_epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if ack.sequence == self.acknowledged && ack.sequence != 0 {
            return Ok(());
        }
        if ack.sequence < self.acknowledged
            || !self.records.iter().any(|r| r.sequence == ack.sequence)
        {
            return Err(Errno::EINVAL);
        }
        while self
            .records
            .front()
            .is_some_and(|r| r.sequence <= ack.sequence)
        {
            let record = self.records.pop_front().expect("front checked");
            self.bytes -= record.bytes.len();
            self.floor = record.start + record.bytes.len() as u64;
        }
        self.acknowledged = ack.sequence;
        self.last_progress = now;
        Ok(())
    }

    /// The last acknowledgement advance, or the journal's creation.
    pub(in crate::shell_transport) fn last_progress(&self) -> Instant {
        self.last_progress
    }

    pub(in crate::shell_transport) fn records(&self) -> usize {
        self.records.len()
    }

    pub(in crate::shell_transport) fn size(&self) -> u64 {
        self.tail
    }
}

#[cfg(test)]
#[path = "../../../tests/support/shell_file_journal.rs"]
mod tests;
