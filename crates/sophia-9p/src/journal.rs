//! Custody for record files: an append-only event journal read by byte offset
//! and released by acknowledgement, and the staging buffer a client's record
//! writes assemble in.
//!
//! Both are mechanism only. Records arrive already encoded, and the owner
//! chooses every bound per append, so role vocabularies, credited reserves and
//! disclosure limits stay with the export that serves the files. Every record
//! begins with its total length as a little-endian `u32`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::export::ReadOutcome;
use crate::records::Errno;

/// The most a journal may hold once one more record is added.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalBounds {
    pub records: usize,
    pub bytes: usize,
}

/// Where a journal stands: the next sequence, the end offset, and what it
/// still retains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalPosition {
    pub next_sequence: u64,
    pub tail: u64,
    pub records: usize,
    pub bytes: usize,
}

struct Retained {
    sequence: u64,
    start: u64,
    bytes: Vec<u8>,
}

/// One epoch's events. Sequences start at 1 and offsets at 0 unless the
/// journal starts at a given position; neither is ever reused.
pub struct Journal {
    epoch: u64,
    records: VecDeque<Retained>,
    bytes: usize,
    tail: u64,
    floor: u64,
    next: u64,
    acknowledged: u64,
}

/// A local reservation. The mutable borrow forbids another journal change
/// before commit; dropping it publishes nothing and spends no sequence, offset
/// or byte.
pub struct PreparedRecord<'a> {
    journal: &'a mut Journal,
    bytes: Vec<u8>,
    next: u64,
    tail: u64,
}

impl PreparedRecord<'_> {
    /// Publishes the record and returns its sequence.
    pub fn commit(self) -> u64 {
        let journal = self.journal;
        let sequence = journal.next;
        journal.bytes += self.bytes.len();
        journal.records.push_back(Retained {
            sequence,
            start: journal.tail,
            bytes: self.bytes,
        });
        journal.next = self.next;
        journal.tail = self.tail;
        sequence
    }
}

impl Journal {
    pub fn new(epoch: u64) -> Self {
        Self::starting_at(epoch, 1, 0)
    }

    /// A journal whose first record takes `next_sequence` at offset `tail`.
    /// Reads below `tail` are stale.
    pub fn starting_at(epoch: u64, next_sequence: u64, tail: u64) -> Self {
        Self {
            epoch,
            records: VecDeque::new(),
            bytes: 0,
            tail,
            floor: tail,
            next: next_sequence,
            acknowledged: next_sequence.saturating_sub(1),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The sequence the next committed record will carry, for its header.
    pub fn next_sequence(&self) -> u64 {
        self.next
    }

    pub fn position(&self) -> JournalPosition {
        JournalPosition {
            next_sequence: self.next,
            tail: self.tail,
            records: self.records.len(),
            bytes: self.bytes,
        }
    }

    /// Whether one more record of `size` bytes stays within `bounds`.
    pub fn fits(&self, size: usize, bounds: JournalBounds) -> bool {
        self.records.len() < bounds.records && size <= bounds.bytes.saturating_sub(self.bytes)
    }

    /// Reserves `bytes`, one complete encoded record, as the next record.
    /// Every fallible check precedes any change: `EAGAIN` when it does not fit
    /// `bounds`, `ENOSPC` when sequences or offsets are exhausted.
    pub fn prepare(
        &mut self,
        bytes: Vec<u8>,
        bounds: JournalBounds,
    ) -> Result<PreparedRecord<'_>, Errno> {
        if !self.fits(bytes.len(), bounds) {
            return Err(Errno::EAGAIN);
        }
        let next = self.next.checked_add(1).ok_or(Errno::ENOSPC)?;
        let tail = self
            .tail
            .checked_add(bytes.len() as u64)
            .ok_or(Errno::ENOSPC)?;
        Ok(PreparedRecord {
            journal: self,
            bytes,
            next,
            tail,
        })
    }

    /// Reads whole or partial records from `offset`. Released offsets are
    /// stale; the end waits for the next record.
    pub fn read(&self, offset: u64, count: u32) -> Result<ReadOutcome, Errno> {
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

    /// Releases retention through `sequence` and reports whether it advanced;
    /// repeating the last acknowledgement is accepted and advances nothing.
    /// Acknowledgement releases transport retention only and never confirms
    /// what a record meant.
    pub fn ack(&mut self, epoch: u64, sequence: u64) -> Result<bool, Errno> {
        if epoch != self.epoch {
            return Err(Errno::ESTALE);
        }
        if sequence == self.acknowledged && sequence != 0 {
            return Ok(false);
        }
        if sequence < self.acknowledged || !self.records.iter().any(|r| r.sequence == sequence) {
            return Err(Errno::EINVAL);
        }
        while self.records.front().is_some_and(|r| r.sequence <= sequence) {
            let record = self.records.pop_front().expect("front checked");
            self.bytes -= record.bytes.len();
            self.floor = record.start + record.bytes.len() as u64;
        }
        self.acknowledged = sequence;
        Ok(true)
    }

    /// The journal file's size: the end offset.
    pub fn size(&self) -> u64 {
        self.tail
    }
}

/// What one staged record may be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StagingBounds {
    /// The smallest declared length: the record header.
    pub header_bytes: usize,
    pub max_bytes: usize,
    /// From the first byte written to the record's completion.
    pub assembly: Duration,
}

/// One open handle's record under assembly: append at the end or repeat an
/// already written range exactly, within the declared length and deadline.
pub struct Staging {
    pub handle: u64,
    pub bytes: Vec<u8>,
    bounds: StagingBounds,
    deadline: Option<Instant>,
}

impl Staging {
    pub fn new(handle: u64, bounds: StagingBounds) -> Self {
        Self {
            handle,
            bytes: Vec::new(),
            bounds,
            deadline: None,
        }
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    /// How long a waiter may sleep before this assembly expires, at most
    /// `maximum`.
    pub fn wait(&self, now: Instant, maximum: Duration) -> Duration {
        self.deadline.map_or(maximum, |deadline| {
            maximum.min(deadline.saturating_duration_since(now))
        })
    }

    pub fn write(&mut self, offset: u64, bytes: &[u8], now: Instant) -> Result<u32, Errno> {
        if self.expired(now) {
            return Err(Errno::ESTALE);
        }
        let offset = usize::try_from(offset).map_err(|_| Errno::EINVAL)?;
        let end = offset
            .checked_add(bytes.len())
            .filter(|n| *n <= self.bounds.max_bytes)
            .ok_or(Errno::EINVAL)?;
        if offset > self.bytes.len() {
            return Err(Errno::EINVAL);
        }
        if offset < self.bytes.len() {
            if end > self.bytes.len() || self.bytes[offset..end] != *bytes {
                return Err(Errno::EINVAL);
            }
            return Ok(bytes.len() as u32);
        }
        // Check a newly completed length before touching the retained prefix.
        if end >= 4 {
            let mut length = [0; 4];
            let retained = self.bytes.len().min(4);
            length[..retained].copy_from_slice(&self.bytes[..retained]);
            if retained < 4 {
                length[retained..].copy_from_slice(&bytes[..4 - retained]);
            }
            let declared = u32::from_le_bytes(length) as usize;
            if !(self.bounds.header_bytes..=self.bounds.max_bytes).contains(&declared)
                || end > declared
            {
                return Err(Errno::EINVAL);
            }
        }
        if !bytes.is_empty() {
            self.deadline.get_or_insert(now + self.bounds.assembly);
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len() as u32)
    }
}
