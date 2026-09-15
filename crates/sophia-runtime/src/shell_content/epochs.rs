use super::{
    ContentAllocationStore, ContentCandidateStore, ContentResourceStore, ContentStoreError,
};
use sophia_protocol::{ContentGrant, ContentLimits};

/// Session-wide owner: reserve a live grant's maximum footprint before admission
/// and retain disconnected epochs until all renderer references have drained.
/// This pool neither exposes a GPU nor negotiates a transport capability.
pub struct ContentEpochPool {
    active: Option<ContentEpoch>,
    retired: Vec<ContentEpoch>,
    last_grant: ContentGrant,
    reserved_live: u64,
    max_retiring_bytes: u64,
    reserved_live_backing: u64,
    max_backing_bytes: u64,
}

struct ContentEpoch {
    allocations: ContentAllocationStore,
    resources: ContentResourceStore,
    candidates: ContentCandidateStore,
}

impl ContentEpoch {
    fn new(limits: ContentLimits) -> Result<Self, ContentStoreError> {
        let candidates =
            ContentCandidateStore::new(limits.clone()).map_err(|_| ContentStoreError::Malformed)?;
        Ok(Self {
            allocations: ContentAllocationStore::new(limits.clone())
                .map_err(|_| ContentStoreError::Malformed)?,
            resources: ContentResourceStore::new(limits)?,
            candidates,
        })
    }

    fn quiescent(&self) -> bool {
        self.allocations.quiescent() && self.resources.quiescent() && self.candidates.quiescent()
    }

    fn revoke(&mut self) {
        self.allocations.revoke();
        self.candidates.revoke();
        self.resources.revoke();
    }
}

impl ContentEpochPool {
    /// Metadata is independently bounded: even tiny pinned resources cannot
    /// retain arbitrarily many replay tables through reconnect churn.
    pub const MAX_RETIRED_EPOCHS: usize = 16;

    pub fn new(max_retiring_bytes: u64) -> Result<Self, ContentStoreError> {
        if max_retiring_bytes == 0 || max_retiring_bytes > 64 * 1024 * 1024 {
            return Err(ContentStoreError::Budget);
        }
        Ok(Self {
            active: None,
            retired: Vec::new(),
            last_grant: ContentGrant::default(),
            reserved_live: 0,
            max_retiring_bytes,
            reserved_live_backing: 0,
            max_backing_bytes: 64 * 1024 * 1024,
        })
    }

    pub fn retired_bytes(&self) -> u64 {
        self.retired
            .iter()
            .map(|epoch| {
                let usage = epoch.resources.usage();
                usage.staging + usage.resident + usage.retiring
            })
            .sum()
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.reserved_live + self.retired_bytes()
    }
    pub fn retired_backing_bytes(&self) -> u64 {
        self.retired
            .iter()
            .map(|epoch| epoch.resources.usage().backing)
            .sum()
    }
    pub fn reserved_backing_bytes(&self) -> u64 {
        self.reserved_live_backing + self.retired_backing_bytes()
    }
    pub(crate) fn active_bulk_occupancy(&self) -> (usize, usize) {
        self.active
            .as_ref()
            .map_or((0, 0), |epoch| epoch.allocations.queued_bulk_occupancy())
    }

    pub(crate) fn active_control_occupancy(&self) -> usize {
        self.active.as_ref().map_or(0, |epoch| {
            epoch.resources.control_occupancy()
                + epoch.candidates.control_occupancy()
                + epoch.allocations.control_occupancy()
        })
    }

    pub fn active_mut(&mut self) -> Option<&mut ContentResourceStore> {
        self.active.as_mut().map(|epoch| &mut epoch.resources)
    }
    pub fn active_allocations_mut(&mut self) -> Option<&mut ContentAllocationStore> {
        self.active.as_mut().map(|epoch| &mut epoch.allocations)
    }
    pub fn active_allocations(&self) -> Option<&ContentAllocationStore> {
        self.active.as_ref().map(|epoch| &epoch.allocations)
    }
    pub fn active(&self) -> Option<&ContentResourceStore> {
        self.active.as_ref().map(|epoch| &epoch.resources)
    }
    pub fn active_candidates_mut(&mut self) -> Option<&mut ContentCandidateStore> {
        self.active.as_mut().map(|epoch| &mut epoch.candidates)
    }
    pub fn active_candidates(&self) -> Option<&ContentCandidateStore> {
        self.active.as_ref().map(|epoch| &epoch.candidates)
    }
    pub fn active_parts_mut(
        &mut self,
    ) -> Option<(&ContentResourceStore, &mut ContentCandidateStore)> {
        self.active
            .as_mut()
            .map(|epoch| (&epoch.resources, &mut epoch.candidates))
    }
    pub fn candidates_mut(&mut self, grant: ContentGrant) -> Option<&mut ContentCandidateStore> {
        if self
            .active
            .as_ref()
            .is_some_and(|epoch| epoch.resources.grant() == grant)
        {
            return self.active_candidates_mut();
        }
        self.retired
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &mut epoch.candidates)
    }

    /// Permission must already have been established by the admission owner.
    /// This reserves storage capacity; it does not establish that permission.
    pub fn admit(&mut self, limits: ContentLimits) -> Result<(), ContentStoreError> {
        self.collect();
        if self.active.is_some() {
            return Err(ContentStoreError::Budget);
        }
        if limits.grant.connection_epoch <= self.last_grant.connection_epoch
            || limits.grant.content_grant_epoch <= self.last_grant.content_grant_epoch
        {
            return Err(ContentStoreError::Stale);
        }
        limits
            .validate()
            .map_err(|_| ContentStoreError::Malformed)?;
        let reserve =
            limits.max_staging_bytes + limits.max_resident_bytes + limits.max_retiring_bytes;
        let backing_reserve = limits.max_resident_bytes + limits.max_retiring_bytes;
        if limits.max_session_retiring_bytes != self.max_retiring_bytes
            || self.retired.len() >= Self::MAX_RETIRED_EPOCHS
            || self.retired_bytes() + reserve > self.max_retiring_bytes
            || self.retired_backing_bytes() + backing_reserve > self.max_backing_bytes
        {
            return Err(ContentStoreError::Budget);
        }
        self.last_grant = limits.grant;
        self.reserved_live = reserve;
        self.reserved_live_backing = backing_reserve;
        self.active = Some(ContentEpoch::new(limits)?);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        if let Some(mut epoch) = self.active.take() {
            epoch.revoke();
            // Delivery obligations are accounted under peer loss; they are not
            // misreported as delivered, nor transferred to the next connection.
            while epoch.resources.take_event().is_some() {}
            while epoch.allocations.take_event().is_some() {}
            while epoch.candidates.take_event().is_some() {}
            if !epoch.quiescent() {
                self.retired.push(epoch);
            }
        }
        self.reserved_live = 0;
        self.reserved_live_backing = 0;
        self.collect();
    }

    pub fn collect(&mut self) {
        if let Some(epoch) = &mut self.active {
            epoch.resources.collect();
        }
        for epoch in &mut self.retired {
            epoch.resources.collect();
            while epoch.resources.take_event().is_some() {}
            while epoch.allocations.take_event().is_some() {}
            while epoch.candidates.take_event().is_some() {}
        }
        self.retired.retain(|epoch| !epoch.quiescent());
    }
}
