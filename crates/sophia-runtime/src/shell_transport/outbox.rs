use std::collections::VecDeque;

use super::outbound::{Admitted, OutboundRecord};

/// One FIFO owns every admitted typed record until a wire takes custody of it
/// whole: the file journal's append, or the socket's last written byte. A
/// record leaves only through `pop_front`; a refused or partial transfer leaves
/// it here, still charged, for a later service turn.
///
/// Each record carries its admission sequence. A wire that also queues records
/// of its own (the socket's legacy frames) stamps them from the same counter,
/// so the order the component observes is exactly the order of admission.
#[derive(Default)]
pub(super) struct ShellOutbox {
    records: VecDeque<Queued>,
    charged: usize,
    bulk: usize,
    controls: usize,
    next_sequence: u64,
}

pub(super) struct Queued {
    pub(super) sequence: u64,
    pub(super) record: OutboundRecord,
    pub(super) control: bool,
    pub(super) charge: usize,
}

impl ShellOutbox {
    /// Total queue charge of every retained record, control and bulk.
    pub(super) fn charged(&self) -> usize {
        self.charged
    }

    pub(super) fn records(&self) -> usize {
        self.records.len()
    }

    pub(super) fn bulk_bytes(&self) -> usize {
        self.bulk
    }

    pub(super) fn controls(&self) -> usize {
        self.controls
    }

    pub(super) fn clear(&mut self) {
        self.records.clear();
        self.charged = 0;
        self.bulk = 0;
        self.controls = 0;
    }

    /// Reserves the next position in the component's single output order.
    pub(super) fn next_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        sequence
    }

    /// The custody transfer. Admission already checked encoding, wire bounds
    /// and capacity, so nothing here can refuse.
    pub(super) fn push(&mut self, admitted: Admitted) {
        let sequence = self.next_sequence();
        self.charged += admitted.charge;
        if admitted.control {
            self.controls += 1;
        } else {
            self.bulk += admitted.charge;
        }
        self.records.push_back(Queued {
            sequence,
            record: admitted.record,
            control: admitted.control,
            charge: admitted.charge,
        });
    }

    pub(super) fn front(&self) -> Option<&Queued> {
        self.records.front()
    }

    /// Releases the front record after a wire took whole custody of it.
    pub(super) fn pop_front(&mut self) -> Queued {
        let queued = self.records.pop_front().expect("a queued record");
        self.charged -= queued.charge;
        if queued.control {
            self.controls -= 1;
        } else {
            self.bulk -= queued.charge;
        }
        queued
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_outbox.rs"]
mod tests;
