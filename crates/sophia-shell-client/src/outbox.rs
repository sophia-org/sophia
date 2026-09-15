use std::collections::VecDeque;

use crate::{MAX_QUEUED_BYTES, MAX_QUEUED_FRAMES, ShellClientError};

// The r5 ceiling is sixteen pending actions. Each can need an ACK plus an
// indicator activation. Uploads cannot occupy these record or byte credits.
const CONTROL_RECORDS: usize = 32;
const CONTROL_BYTES: usize = CONTROL_RECORDS * 256;

#[derive(Default)]
pub(crate) struct ClientOutbox {
    frames: VecDeque<Frame>,
    bytes: usize,
    bulk_records: usize,
    bulk_bytes: usize,
}
struct Frame {
    bytes: Box<[u8]>,
    offset: usize,
    control: bool,
}
impl ClientOutbox {
    pub(crate) fn enqueue(
        &mut self,
        frames: Vec<Vec<u8>>,
        control: bool,
    ) -> Result<(), ShellClientError> {
        self.enqueue_after(frames, control, || Ok(()))
    }

    /// Reserve the complete FIFO transfer before updating its producer. The
    /// callback cannot access this mutably borrowed FIFO. Once it succeeds,
    /// only infallible moves and integer accounting remain before ownership.
    pub(crate) fn enqueue_after(
        &mut self,
        frames: Vec<Vec<u8>>,
        control: bool,
        before_commit: impl FnOnce() -> Result<(), ShellClientError>,
    ) -> Result<(), ShellClientError> {
        let bytes: usize = frames.iter().map(Vec::len).sum();
        if self.frames.len().saturating_add(frames.len()) > MAX_QUEUED_FRAMES
            || self.bytes.saturating_add(bytes) > MAX_QUEUED_BYTES
            || (!control
                && (self.bulk_records.saturating_add(frames.len())
                    > MAX_QUEUED_FRAMES - CONTROL_RECORDS
                    || self.bulk_bytes.saturating_add(bytes) > MAX_QUEUED_BYTES - CONTROL_BYTES))
        {
            return Err(ShellClientError::QueueSaturated);
        }
        // Prepare all allocations before transferring either half. Extending
        // the already-reserved FIFO cannot leave just an ACK after unwind.
        let prepared: Vec<_> = frames
            .into_iter()
            .map(|bytes| Frame {
                bytes: bytes.into_boxed_slice(),
                offset: 0,
                control,
            })
            .collect();
        let records = prepared.len();
        self.frames
            .try_reserve(records)
            .map_err(|_| ShellClientError::QueueSaturated)?;
        before_commit()?;
        self.frames.extend(prepared);
        self.bytes += bytes;
        if !control {
            self.bulk_records += records;
            self.bulk_bytes += bytes;
        }
        Ok(())
    }
    pub(crate) fn front(&self) -> Option<&[u8]> {
        self.frames
            .front()
            .map(|frame| &frame.bytes[frame.offset..])
    }
    pub(crate) fn written(&mut self, count: usize) {
        let frame = self
            .frames
            .front_mut()
            .expect("write requires an owned frame");
        assert!(count <= frame.bytes.len() - frame.offset);
        frame.offset += count;
        if frame.offset == frame.bytes.len() {
            self.bytes -= frame.bytes.len();
            if !frame.control {
                self.bulk_records -= 1;
                self.bulk_bytes -= frame.bytes.len();
            }
            self.frames.pop_front();
        }
    }
}

#[cfg(test)]
#[path = "../tests/support/outbox.rs"]
mod tests;
