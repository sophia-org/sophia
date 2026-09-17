//! Single-connection compatibility facade over the actual bounded epoch owner.
//! Session multi-component admission uses ContentEpochRegistry directly; this
//! facade deliberately cannot admit a second simultaneous grant.

use super::{
    ContentAllocationStore, ContentCandidateStore, ContentEpochAccounting, ContentEpochRegistry,
    ContentResourceStore, ContentStoreError,
};
use sophia_protocol::{ContentGrant, ContentLimits};

pub struct ContentEpochPool {
    epochs: ContentEpochRegistry,
    grant: ContentGrant,
}

impl ContentEpochPool {
    pub const MAX_RETIRED_EPOCHS: usize = ContentEpochRegistry::MAX_RETAINED_EPOCHS;

    pub fn new(max_retiring_bytes: u64) -> Result<Self, ContentStoreError> {
        Ok(Self {
            epochs: ContentEpochRegistry::new(max_retiring_bytes)?,
            grant: ContentGrant::default(),
        })
    }

    pub fn finish_after_backend_drop<B>(&mut self, backend: B) -> Result<usize, B> {
        self.epochs.finish_after_backend_drop(backend)
    }

    pub fn accounting(&self) -> ContentEpochAccounting {
        self.epochs.accounting()
    }

    pub fn retired_bytes(&self) -> u64 {
        self.epochs.retired_bytes()
    }

    pub fn retired_backing_bytes(&self) -> u64 {
        self.epochs.retired_backing_bytes()
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.epochs.reserved_bytes()
    }

    pub fn reserved_backing_bytes(&self) -> u64 {
        self.epochs.reserved_backing_bytes()
    }

    pub fn active_mut(&mut self) -> Option<&mut ContentResourceStore> {
        self.epochs.resources_mut(self.grant)
    }

    pub fn active(&self) -> Option<&ContentResourceStore> {
        self.epochs.resources(self.grant)
    }

    pub fn active_allocations_mut(&mut self) -> Option<&mut ContentAllocationStore> {
        self.epochs.allocations_mut(self.grant)
    }

    pub fn active_allocations(&self) -> Option<&ContentAllocationStore> {
        self.epochs.allocations(self.grant)
    }

    pub fn active_candidates_mut(&mut self) -> Option<&mut ContentCandidateStore> {
        self.epochs.active_candidates_mut(self.grant)
    }

    pub fn active_candidates(&self) -> Option<&ContentCandidateStore> {
        self.epochs.active_candidates(self.grant)
    }

    pub fn active_parts_mut(
        &mut self,
    ) -> Option<(&ContentResourceStore, &mut ContentCandidateStore)> {
        self.epochs.active_parts_mut(self.grant)
    }

    pub fn candidates_mut(&mut self, grant: ContentGrant) -> Option<&mut ContentCandidateStore> {
        self.epochs.candidates_mut(grant)
    }

    pub fn admit(&mut self, limits: ContentLimits) -> Result<(), ContentStoreError> {
        self.collect();
        if self.active().is_some() {
            return Err(ContentStoreError::Budget);
        }
        let grant = limits.grant;
        self.epochs.admit(limits)?;
        self.grant = grant;
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.epochs.disconnect(self.grant);
    }

    pub fn collect(&mut self) {
        self.epochs.collect();
    }
}
