use super::*;
use std::collections::VecDeque;

struct Record {
    sequence: u64,
    start: u64,
    bytes: Vec<u8>,
}

#[path = "../../../../tests/support/policy_file_journal_reservation.rs"]
pub(super) mod reservation_tests;

pub(super) struct Journal {
    epoch: u64,
    records: VecDeque<Record>,
    bytes: usize,
    tail: u64,
    floor: u64,
    next: u64,
    acknowledged: u64,
}

/// Local reservation only. The mutable borrow forbids another journal change
/// before commit; dropping it publishes nothing and spends no identity.
pub(super) struct PreparedEvent<'a> {
    journal: &'a mut Journal,
    bytes: Vec<u8>,
    next: u64,
    tail: u64,
}
impl PreparedEvent<'_> {
    pub(super) fn commit(self) -> u64 {
        let sequence = self.journal.next;
        self.journal.bytes += self.bytes.len();
        self.journal.records.push_back(Record {
            sequence,
            start: self.journal.tail,
            bytes: self.bytes,
        });
        self.journal.next = self.next;
        self.journal.tail = self.tail;
        sequence
    }
}

impl Journal {
    pub(super) fn new(epoch: u64) -> Self {
        Self {
            epoch,
            records: VecDeque::with_capacity(usize::from(WM_FILE_MAX_JOURNAL_RECORDS)),
            bytes: 0,
            tail: 0,
            floor: 0,
            next: 1,
            acknowledged: 0,
        }
    }

    pub(super) fn append(&mut self, kind: WmFileKind, body: &[u8]) -> Result<u64, Errno> {
        if wm_file_class(kind) != WmFileClass::Event {
            return Err(Errno::EINVAL);
        }
        let size = WM_FILE_HEADER_BYTES
            .checked_add(body.len())
            .ok_or(Errno::EINVAL)?;
        if size > WM_FILE_MAX_BYTES {
            return Err(Errno::EINVAL);
        }
        if self.records.len() == usize::from(WM_FILE_MAX_JOURNAL_RECORDS)
            || size > WM_FILE_MAX_BYTES - self.bytes
        {
            return Err(Errno::EAGAIN);
        }
        let bytes = encode_wm_file_record(
            WmFileHeader {
                kind,
                connection_epoch: self.epoch,
                submission_id: 0,
                sequence: self.next,
            },
            body,
        )
        .map_err(|_| Errno::EINVAL)?;
        self.commit(bytes)
    }

    pub(super) fn prepare_encoded(
        &mut self,
        kind: WmFileKind,
        encode: impl FnOnce(WmFileHeader) -> Result<Vec<u8>, Errno>,
    ) -> Result<PreparedEvent<'_>, Errno> {
        if wm_file_class(kind) != WmFileClass::Event {
            return Err(Errno::EINVAL);
        }
        if self.records.len() == usize::from(WM_FILE_MAX_JOURNAL_RECORDS) {
            return Err(Errno::EAGAIN);
        }
        let header = WmFileHeader {
            kind,
            connection_epoch: self.epoch,
            submission_id: 0,
            sequence: self.next,
        };
        let bytes = encode(header)?;
        let record =
            decode_wm_file_record(&bytes, WmFileClass::Event).map_err(|_| Errno::EINVAL)?;
        if record.header != header {
            return Err(Errno::EINVAL);
        }
        self.prepare(bytes)
    }

    fn commit(&mut self, bytes: Vec<u8>) -> Result<u64, Errno> {
        Ok(self.prepare(bytes)?.commit())
    }

    fn prepare(&mut self, bytes: Vec<u8>) -> Result<PreparedEvent<'_>, Errno> {
        let size = bytes.len();
        if size > WM_FILE_MAX_BYTES {
            return Err(Errno::EINVAL);
        }
        if self.records.len() == usize::from(WM_FILE_MAX_JOURNAL_RECORDS)
            || size > WM_FILE_MAX_BYTES - self.bytes
        {
            return Err(Errno::EAGAIN);
        }
        let next = self.next.checked_add(1).ok_or(Errno::ENOSPC)?;
        let tail = self.tail.checked_add(size as u64).ok_or(Errno::ENOSPC)?;
        Ok(PreparedEvent {
            journal: self,
            bytes,
            next,
            tail,
        })
    }

    pub(super) fn read(&self, offset: u64, count: u32) -> Result<ReadOutcome, Errno> {
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

    pub(super) fn ack(&mut self, ack: WmFileAck) -> Result<(), Errno> {
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
        Ok(())
    }

    pub(super) fn size(&self) -> u64 {
        self.tail
    }
}
