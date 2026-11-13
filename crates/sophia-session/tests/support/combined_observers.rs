//! G3-5d d-2b3d, test-support only: slot-indexed, read-only observers of a
//! slot's persistent transport and its grant's stores (REV2 D5). They take
//! no service borrow, so they read in every phase, before and after stop,
//! beside the slot's current attempt and phase. A slot that does not exist
//! reads `None`, never an empty snapshot; a slot without a wire reads
//! `wired == false`.
use super::*;
use sophia_protocol::ContentOutputId;
use sophia_runtime::owned_ring_protocol::Identity;
use sophia_runtime::{StoreEventFacts, TransportFacts};

impl ShellComponentConnections {
    pub(crate) fn transport_facts(
        &self,
        slot: usize,
    ) -> Option<(
        Option<(ContentGrant, ComponentConnectionPhase)>,
        TransportFacts,
    )> {
        let connection = self.connections.get(slot)?;
        Some((connection.attempt, connection.transport.transport_facts()))
    }

    pub(crate) fn store_event_facts(&self, slot: usize) -> Option<StoreEventFacts> {
        self.connections
            .get(slot)?
            .transport
            .store_event_facts(&self.epochs)
    }

    /// `None` without an image owner in the slot.
    pub(crate) fn owned_releasing(&self, slot: usize) -> Option<Vec<Identity>> {
        self.connections
            .get(slot)?
            .images
            .as_ref()
            .map(|owner| owner.owned_releasing())
    }

    pub(crate) fn candidate_prepared(
        &self,
        slot: usize,
        output: ContentOutputId,
        generation: u64,
    ) -> Option<bool> {
        self.connections
            .get(slot)?
            .transport
            .candidate_prepared(&self.epochs, output, generation)
    }
}
