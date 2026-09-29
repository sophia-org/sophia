use std::collections::VecDeque;

use super::outbound::{Admitted, OutboundRecord};

/// One FIFO owns every admitted typed record until the export takes custody
/// whole. A record leaves only through `pop_front`; a refused transfer leaves
/// it here, still charged, for a later service turn.
#[derive(Default)]
pub(super) struct ShellOutbox {
    records: VecDeque<Queued>,
    charged: usize,
    bulk: usize,
    controls: usize,
}

pub(super) struct Queued {
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

    /// The custody transfer. Admission already checked encoding, wire bounds
    /// and capacity, so nothing here can refuse.
    pub(super) fn push(&mut self, admitted: Admitted) {
        self.charged += admitted.charge;
        if admitted.control {
            self.controls += 1;
        } else {
            self.bulk += admitted.charge;
        }
        self.records.push_back(Queued {
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
pub(super) mod tests;
