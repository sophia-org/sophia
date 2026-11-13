//! G3-5d d-2b3d, test-support only: read-only facts of one component's
//! persistent transport, from its outbox, its file wire's journal, its
//! owned-ring credits and its grant's three content stores. Reading
//! drains, publishes, ACKs, flushes, services and advances nothing. A
//! missing wire reads as `wired == false` with empty wire facts, never as
//! an empty journal.
use super::ShellComponentTransport;
use super::files::{JournalFacts, RetainedEntry};
use super::outbound::OutboundRecord;
use crate::ContentEpochRegistry;
use crate::owned_ring_protocol::Offer;
use sophia_protocol::{ShellContentRecord, TransactionId};

/// A queued record's identity, as the FIFO holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutboxRecordFacts {
    #[cfg(feature = "experimental-native-frame-transaction")]
    Frame(sophia_protocol::shell_files::ShellFrameRecord),
    Content(TransactionId, ShellContentRecord),
    Other(&'static str),
}

impl From<&OutboundRecord> for OutboxRecordFacts {
    fn from(record: &OutboundRecord) -> Self {
        match record {
            #[cfg(feature = "experimental-native-frame-transaction")]
            OutboundRecord::Frame(frame) => Self::Frame(frame.clone()),
            OutboundRecord::Content(transaction, record) => {
                Self::Content(*transaction, record.clone())
            }
            OutboundRecord::Descriptor(_) => Self::Other("Descriptor"),
            OutboundRecord::NativeLauncher(..) => Self::Other("NativeLauncher"),
            OutboundRecord::CatalogAction(..) => Self::Other("CatalogAction"),
            OutboundRecord::IndicatorOutcome(..) => Self::Other("IndicatorOutcome"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxEntryFacts {
    pub resource_settlement: bool,
    pub control: bool,
    pub charge: usize,
    pub record: OutboxRecordFacts,
}

/// One live owned-ring credit: its offer and whether the status
/// reservation is still unspent (`release` is spent with the slot).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedCreditFacts {
    pub offer: Offer,
    pub status_reserved: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransportFacts {
    /// A file wire is held; its facts below are `Some`/nonempty only then.
    pub wired: bool,
    pub output_blocked: bool,
    pub outbox: Vec<OutboxEntryFacts>,
    pub journal: Option<JournalFacts>,
    pub table: Vec<RetainedEntry>,
    pub owned_live_credits: Vec<OwnedCreditFacts>,
    pub owned_publication_sequence: Option<u64>,
    pub lean_frames_in_flight: Option<u32>,
}

/// The grant's queued store events, not yet transferred to the outbox.
/// Candidate records are kept as their Debug text: the queue holds two
/// record types behind one head.
#[derive(Clone, Debug, PartialEq)]
pub struct StoreEventFacts {
    pub resources: Vec<(TransactionId, ShellContentRecord)>,
    pub candidates: Vec<(TransactionId, String)>,
    pub allocations: Vec<(TransactionId, ShellContentRecord)>,
}

impl ShellComponentTransport {
    pub fn transport_facts(&self) -> TransportFacts {
        let export = self.wire.as_ref().map(|wire| wire.export());
        TransportFacts {
            wired: export.is_some(),
            output_blocked: self.output_blocked,
            outbox: self.output.snapshot(),
            journal: export.map(|files| files.journal().facts()),
            table: export.map_or_else(Vec::new, |files| files.journal().retained_table()),
            owned_live_credits: export.map_or_else(Vec::new, |files| files.owned_live_credits()),
            owned_publication_sequence: export.map(|files| files.owned_publication_sequence()),
            lean_frames_in_flight: export.map(|files| files.lean_frames_in_flight()),
        }
    }

    /// `None` when the store grant has no active epoch.
    pub fn store_event_facts(&self, epochs: &ContentEpochRegistry) -> Option<StoreEventFacts> {
        let grant = self.store_grant;
        Some(StoreEventFacts {
            resources: epochs.resources(grant)?.pending_events(),
            candidates: epochs.active_candidates(grant)?.pending_events(),
            allocations: epochs.allocations(grant)?.pending_events(),
        })
    }

    /// `ContentCandidateStore::candidate_prepared` for the store grant.
    #[cfg(feature = "experimental-combined-frame-job")]
    pub fn candidate_prepared(
        &self,
        epochs: &ContentEpochRegistry,
        output: sophia_protocol::ContentOutputId,
        generation: u64,
    ) -> Option<bool> {
        epochs
            .active_candidates(self.store_grant)?
            .candidate_prepared(output, generation)
    }
}
