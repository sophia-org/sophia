//! Snapshot the existing epoch, input and aggregate response owners.

use super::{ShellSessionTransport, control_budget::CONTROL_FRAME_BYTES};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShellContentAccounting {
    pub epochs: crate::ContentEpochAccounting,
    /// Store credits, queued store responses, action-cancel/outcome credits,
    /// and FIFO frames. A partial write still owns the entire frame charge.
    pub response_records: usize,
    pub response_bytes: usize,
    pub input_records: usize,
    pub input_bytes: usize,
}

impl ShellContentAccounting {
    pub fn quiescent(&self) -> bool {
        self.epochs.quiescent()
            && self.response_records == 0
            && self.response_bytes == 0
            && self.input_records == 0
            && self.input_bytes == 0
    }
}

impl ShellSessionTransport {
    /// Observe charges rather than allocating a second resource registry.
    pub fn content_accounting(&self) -> ShellContentAccounting {
        let epochs = self.content_epochs.accounting();
        let (bulk_records, bulk_bytes) = self.content_epochs.active_bulk_occupancy();
        let reserved = epochs.response_records
            + self.action_cancellations.len()
            + usize::from(self.indicator_response.is_some());
        let controls = reserved - bulk_records + self.output.controls();
        ShellContentAccounting {
            epochs,
            response_records: reserved + self.output.records(),
            response_bytes: bulk_bytes + self.output.bulk_bytes() + controls * CONTROL_FRAME_BYTES,
            input_records: self.inbox.len(),
            input_bytes: self.input.len() + self.inbox.iter().map(Vec::len).sum::<usize>(),
        }
    }

    /// Collection only releases stores whose actual tracked consumers ended.
    /// Calling this after disconnect is safe and requires no socket operation.
    pub fn collect_content_accounting(&mut self) -> ShellContentAccounting {
        self.content_epochs.collect();
        self.content_accounting()
    }
}
