//! Session-owned storage, including exact disconnected owners. Transport
//! permission and connection supervision remain outside this resource owner.

use super::{
    ContentAllocationStore, ContentCandidateStore, ContentEpochAccounting, ContentResourceStore,
    ContentStoreError, ContentStoreProfile,
};
use sophia_protocol::{ContentGrant, ContentLimits};

pub struct ContentEpochRegistry {
    active: Vec<ContentEpoch>,
    retired: Vec<ContentEpoch>,
    last_grant: ContentGrant,
    max_active_epochs: usize,
    max_bytes: u64,
    max_backing_bytes: u64,
}

struct ContentEpoch {
    profile: ContentStoreProfile,
    allocations: ContentAllocationStore,
    resources: ContentResourceStore,
    candidates: ContentCandidateStore,
    reserved_bytes: u64,
    reserved_backing_bytes: u64,
}

impl ContentEpoch {
    fn new(limits: ContentLimits, profile: ContentStoreProfile) -> Result<Self, ContentStoreError> {
        let reserved_bytes =
            limits.max_staging_bytes + limits.max_resident_bytes + limits.max_retiring_bytes;
        let reserved_backing_bytes = limits.max_resident_bytes + limits.max_retiring_bytes;
        Ok(Self {
            profile,
            candidates: ContentCandidateStore::with_profile(limits.clone(), profile)
                .map_err(|_| ContentStoreError::Malformed)?,
            allocations: ContentAllocationStore::with_profile(limits.clone(), profile)
                .map_err(|_| ContentStoreError::Malformed)?,
            resources: ContentResourceStore::new(limits)?,
            reserved_bytes,
            reserved_backing_bytes,
        })
    }

    fn quiescent(&self) -> bool {
        self.allocations.quiescent() && self.resources.quiescent() && self.candidates.quiescent()
    }

    fn discard_peer_responses(&mut self) {
        // Peer loss accounts for these undeliverable replies. It does not
        // release a render consumer or transfer replies to a new connection.
        while self.resources.take_event().is_some() {}
        while self.allocations.take_event().is_some() {}
        while self.candidates.take_event().is_some() {}
    }
}

impl ContentEpochRegistry {
    pub const MAX_ACTIVE_EPOCHS: usize = 3;
    pub const DEFAULT_ACTIVE_EPOCHS: usize = 2;
    pub const MAX_RETAINED_EPOCHS: usize = 16;

    /// Legacy caller chooses its next connection identity. The content epoch
    /// comes from the common owner, never a transport-local counter. Admission
    /// publishes the watermark only after its complete reservation succeeds.
    pub(crate) fn next_grant(
        &self,
        connection_epoch: u64,
    ) -> Result<ContentGrant, ContentStoreError> {
        if connection_epoch <= self.last_grant.connection_epoch {
            return Err(ContentStoreError::Stale);
        }
        Ok(ContentGrant {
            connection_epoch,
            content_grant_epoch: self
                .last_grant
                .content_grant_epoch
                .checked_add(1)
                .ok_or(ContentStoreError::Stale)?,
        })
    }

    pub fn new(max_bytes: u64) -> Result<Self, ContentStoreError> {
        Self::with_active_capacity(max_bytes, Self::DEFAULT_ACTIVE_EPOCHS)
    }

    /// Explicit owner capacity, independent of client roles or permissions.
    /// Increasing count never increases the aggregate byte or retirement budget.
    pub fn with_active_capacity(
        max_bytes: u64,
        max_active_epochs: usize,
    ) -> Result<Self, ContentStoreError> {
        if max_bytes == 0
            || max_bytes > 64 * 1024 * 1024
            || !(1..=Self::MAX_ACTIVE_EPOCHS).contains(&max_active_epochs)
        {
            return Err(ContentStoreError::Budget);
        }
        Ok(Self {
            active: Vec::with_capacity(max_active_epochs),
            max_active_epochs,
            retired: Vec::with_capacity(Self::MAX_RETAINED_EPOCHS),
            last_grant: ContentGrant::default(),
            max_bytes,
            max_backing_bytes: 64 * 1024 * 1024,
        })
    }

    /// Reserves the full possible footprint and one future retirement slot.
    /// Both epochs are minted monotonically by the Session admission owner;
    /// neither a component name nor this storage reservation grants authority.
    pub fn admit(&mut self, limits: ContentLimits) -> Result<(), ContentStoreError> {
        self.admit_with_profile(limits, ContentStoreProfile::Legacy)
    }

    pub fn admit_with_profile(
        &mut self,
        limits: ContentLimits,
        profile: ContentStoreProfile,
    ) -> Result<(), ContentStoreError> {
        self.collect();
        if self.active.len() == self.max_active_epochs
            || self.active.len() + self.retired.len() >= Self::MAX_RETAINED_EPOCHS
        {
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
        let reserve = limits
            .max_staging_bytes
            .checked_add(limits.max_resident_bytes)
            .and_then(|bytes| bytes.checked_add(limits.max_retiring_bytes))
            .ok_or(ContentStoreError::Budget)?;
        let backing = limits
            .max_resident_bytes
            .checked_add(limits.max_retiring_bytes)
            .ok_or(ContentStoreError::Budget)?;
        if limits.max_session_retiring_bytes != self.max_bytes
            || self
                .reserved_bytes()
                .checked_add(reserve)
                .is_none_or(|n| n > self.max_bytes)
            || self
                .reserved_backing_bytes()
                .checked_add(backing)
                .is_none_or(|n| n > self.max_backing_bytes)
        {
            return Err(ContentStoreError::Budget);
        }
        let grant = limits.grant;
        let epoch = ContentEpoch::new(limits, profile)?;
        // All fallible construction precedes publication or watermark change.
        self.active.push(epoch);
        self.last_grant = grant;
        Ok(())
    }

    pub fn profile(&self, grant: ContentGrant) -> Option<ContentStoreProfile> {
        self.active
            .iter()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| epoch.profile)
    }

    pub fn resources(&self, grant: ContentGrant) -> Option<&ContentResourceStore> {
        self.active
            .iter()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &epoch.resources)
    }

    pub(crate) fn bulk_occupancy(&self, grant: ContentGrant) -> (usize, usize) {
        self.allocations(grant)
            .map_or((0, 0), ContentAllocationStore::queued_bulk_occupancy)
    }

    pub(crate) fn control_occupancy(&self, grant: ContentGrant) -> usize {
        self.resources(grant)
            .map_or(0, ContentResourceStore::control_occupancy)
            + self
                .allocations(grant)
                .map_or(0, ContentAllocationStore::control_occupancy)
            + self
                .active_candidates(grant)
                .map_or(0, ContentCandidateStore::control_occupancy)
    }

    pub fn resources_mut(&mut self, grant: ContentGrant) -> Option<&mut ContentResourceStore> {
        self.active
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &mut epoch.resources)
    }

    pub fn allocations(&self, grant: ContentGrant) -> Option<&ContentAllocationStore> {
        self.active
            .iter()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &epoch.allocations)
    }

    pub fn allocations_mut(&mut self, grant: ContentGrant) -> Option<&mut ContentAllocationStore> {
        self.active
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &mut epoch.allocations)
    }

    pub fn active_candidates(&self, grant: ContentGrant) -> Option<&ContentCandidateStore> {
        self.active
            .iter()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &epoch.candidates)
    }

    pub fn active_candidates_mut(
        &mut self,
        grant: ContentGrant,
    ) -> Option<&mut ContentCandidateStore> {
        self.active
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &mut epoch.candidates)
    }

    pub fn active_parts_mut(
        &mut self,
        grant: ContentGrant,
    ) -> Option<(&ContentResourceStore, &mut ContentCandidateStore)> {
        self.active
            .iter_mut()
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| (&epoch.resources, &mut epoch.candidates))
    }

    /// Native completions can settle the exact disconnected store. New peer
    /// requests must use active_candidates_mut instead.
    pub fn candidates_mut(&mut self, grant: ContentGrant) -> Option<&mut ContentCandidateStore> {
        self.active
            .iter_mut()
            .chain(&mut self.retired)
            .find(|epoch| epoch.resources.grant() == grant)
            .map(|epoch| &mut epoch.candidates)
    }

    /// Revoke this grant only. A repeated/stale disconnect cannot revoke a
    /// successor or a neighbor. Preallocated retirement capacity follows every
    /// admitted owner, so this transfer never grows the retirement inventory.
    pub fn disconnect(&mut self, grant: ContentGrant) -> bool {
        let Some(index) = self
            .active
            .iter()
            .position(|epoch| epoch.resources.grant() == grant)
        else {
            return false;
        };
        let mut epoch = self.active.remove(index);
        epoch.allocations.revoke();
        epoch.candidates.revoke();
        epoch.resources.revoke();
        epoch.discard_peer_responses();
        if !epoch.quiescent() {
            self.retired.push(epoch);
        }
        self.collect();
        true
    }

    pub fn collect(&mut self) {
        for epoch in &mut self.active {
            epoch.resources.collect();
        }
        for epoch in &mut self.retired {
            epoch.resources.collect();
            epoch.discard_peer_responses();
        }
        self.retired.retain(|epoch| !epoch.quiescent());
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

    pub fn retired_backing_bytes(&self) -> u64 {
        self.retired
            .iter()
            .map(|epoch| epoch.resources.usage().backing)
            .sum()
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.active
            .iter()
            .map(|epoch| epoch.reserved_bytes)
            .sum::<u64>()
            + self.retired_bytes()
    }

    pub fn reserved_backing_bytes(&self) -> u64 {
        self.active
            .iter()
            .map(|epoch| epoch.reserved_backing_bytes)
            .sum::<u64>()
            + self.retired_backing_bytes()
    }

    pub fn accounting(&self) -> ContentEpochAccounting {
        let mut value = ContentEpochAccounting {
            grant: self.last_grant,
            active_epochs: self.active.len(),
            retired_epochs: self.retired.len(),
            reserved_bytes: self.reserved_bytes(),
            reserved_backing_bytes: self.reserved_backing_bytes(),
            ..Default::default()
        };
        for epoch in self.active.iter().chain(&self.retired) {
            epoch.resources.add_accounting(&mut value);
            epoch.candidates.add_accounting(&mut value);
            epoch.allocations.add_accounting(&mut value);
        }
        value
    }

    /// Observe every retained generation of this profile after the caller's
    /// collection pass. Never substitute only the most recent predecessor or
    /// its negotiated ceiling for the resources actually still owned.
    pub fn reconnect_budget(&self, profile: ContentStoreProfile) -> super::ContentReconnectBudget {
        let mut budget = super::ContentReconnectBudget {
            capacity_bytes: self.max_bytes,
            capacity_backing_bytes: self.max_backing_bytes,
            reserved_bytes: self.reserved_bytes(),
            reserved_backing_bytes: self.reserved_backing_bytes(),
            active_epochs: self.active.len(),
            retired_epochs: self.retired.len(),
            ..Default::default()
        };
        for epoch in self.retired.iter().filter(|epoch| epoch.profile == profile) {
            let usage = epoch.resources.usage();
            budget.own_retired_bytes += usage.staging + usage.resident + usage.retiring;
            budget.own_retired_epochs += 1;
        }
        budget
    }

    /// Final Session backend shutdown only, after its workers have ended.
    /// Join success alone is not proof of that disposition. Any still-live
    /// grant refuses the transfer and returns the actual backend unchanged.
    pub fn finish_after_backend_drop<B>(&mut self, backend: B) -> Result<usize, B> {
        if !self.active.is_empty() {
            return Err(backend);
        }
        drop(backend);
        let mut count = 0;
        for epoch in &mut self.retired {
            while let Some((output, generation)) = epoch.candidates.first_submitted_identity() {
                epoch
                    .candidates
                    .renderer_failed(output, generation)
                    .expect("the unchanged retired store owns this exact submitted identity");
                count += 1;
            }
        }
        self.collect();
        Ok(count)
    }
}
