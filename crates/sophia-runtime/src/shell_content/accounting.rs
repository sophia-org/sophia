//! Read-only inventory of the owners enforcing content budgets.
//! These are protocol/storage charges, not process RSS or driver VRAM.

use super::ContentMemoryUsage;
use sophia_protocol::ContentGrant;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentEpochAccounting {
    /// Last admitted identity remains observable after its final collection.
    pub grant: ContentGrant,
    pub active_epochs: usize,
    pub retired_epochs: usize,
    pub memory: ContentMemoryUsage,
    pub transfers: usize,
    pub resources: usize,
    pub resource_ids: usize,
    pub candidates: usize,
    pub permits: usize,
    pub demands: usize,
    pub allocations: usize,
    pub response_records: usize,
    pub reserved_bytes: u64,
    pub reserved_backing_bytes: u64,
}

impl ContentEpochAccounting {
    /// An empty byte count alone does not settle a candidate or reserved reply.
    pub fn quiescent(&self) -> bool {
        let empty = Self {
            grant: self.grant,
            ..Self::default()
        };
        *self == empty
    }
}
