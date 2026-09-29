//! Read coverage is bounded by retained journal bytes. Arbitrary partial and
//! overlapping reads count, but holes cannot authorize cumulative release.
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::output_files::*;

use crate::OutputFileJournal;

pub(crate) struct OutputFileReads {
    floor: u64,
    covered: Vec<bool>,
    acknowledged: u64,
}

impl OutputFileReads {
    pub(crate) fn new() -> Self {
        Self {
            floor: 0,
            covered: Vec::new(),
            acknowledged: 0,
        }
    }

    pub(crate) fn read(
        &mut self,
        journal: &OutputFileJournal,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        let result = journal.read(offset, count)?;
        if let ReadOutcome::Ready(bytes) = &result {
            // The owner has not acknowledged past floor; retention bounds
            // therefore also bound the coverage array, regardless of offsets.
            let retained =
                usize::try_from(journal.position().tail - self.floor).map_err(|_| Errno::ENOSPC)?;
            self.covered.resize(retained, false);
            let start = usize::try_from(offset - self.floor).map_err(|_| Errno::EINVAL)?;
            self.covered[start..start + bytes.len()].fill(true);
        }
        Ok(result)
    }

    /// Return the byte boundary to release only when every preceding record
    /// has been read. The journal remains untouched until the owner also
    /// verifies object-publication dependencies.
    pub(crate) fn prepare_ack(
        &self,
        journal: &OutputFileJournal,
        sequence: u64,
    ) -> Result<usize, Errno> {
        if sequence == self.acknowledged && sequence != 0 {
            return Ok(0);
        }
        if sequence <= self.acknowledged || sequence >= journal.position().next_sequence {
            return Err(Errno::EINVAL);
        }
        let mut end: usize = 0;
        loop {
            let ReadOutcome::Ready(header) =
                journal.read(self.floor + end as u64, OUTPUT_FILE_HEADER_BYTES as u32)?
            else {
                return Err(Errno::EINVAL);
            };
            if header.len() != OUTPUT_FILE_HEADER_BYTES {
                return Err(Errno::EINVAL);
            }
            let size = u32::from_le_bytes(header[..4].try_into().expect("header")) as usize;
            let seq = u64::from_le_bytes(header[24..32].try_into().expect("header"));
            end = end.checked_add(size).ok_or(Errno::ENOSPC)?;
            if end > self.covered.len() {
                return Err(Errno::EAGAIN);
            }
            if seq == sequence {
                return if self.covered[..end].iter().all(|read| *read) {
                    Ok(end)
                } else {
                    Err(Errno::EAGAIN)
                };
            }
            if seq > sequence {
                return Err(Errno::EINVAL);
            }
        }
    }

    pub(crate) fn commit_ack(&mut self, sequence: u64, bytes: usize) {
        self.covered.drain(..bytes);
        self.floor += bytes as u64;
        self.acknowledged = sequence;
    }
}
