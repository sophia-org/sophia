//! Ring extents are charged once, independently of every copied destination.
use super::*;
use crate::OwnedRingGeometry;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Non-cloneable Session reservation. The ring's shared source owner retains it
/// until the descriptor and every possible worker read have gone away. An epoch
/// with this charge cannot become quiescent merely because its peer disconnected.
pub struct OwnedRingSourceReservation {
    grant: ContentGrant,
    geometry: OwnedRingGeometry,
    used: Arc<AtomicU64>,
}

impl OwnedRingSourceReservation {
    pub fn grant(&self) -> ContentGrant {
        self.grant
    }
    pub fn geometry(&self) -> OwnedRingGeometry {
        self.geometry
    }
    pub(crate) fn is_charged_to(&self, used: &Arc<AtomicU64>) -> bool {
        Arc::ptr_eq(&self.used, used)
    }
}

impl Drop for OwnedRingSourceReservation {
    fn drop(&mut self) {
        self.used
            .fetch_sub(self.geometry.source_bytes() as u64, Ordering::Release);
    }
}

impl ContentEpochRegistry {
    pub(crate) fn owned_copy_description(
        &self,
        grant: ContentGrant,
        request: &crate::OwnedCopyImageRequest,
    ) -> Option<sophia_protocol::ContentResourceBegin> {
        self.active
            .iter()
            .find(|e| e.resources.grant() == grant)
            .and_then(|e| e.allocations.owned_copy_description(request).ok())
    }

    pub(crate) fn reserve_owned_copy_image(
        &mut self,
        ring: &crate::OwnedRing,
        request: crate::OwnedCopyImageRequest,
        worker: &mut crate::IsbValidationWorker,
        now: u64,
        deadline: std::time::Instant,
    ) -> Result<crate::OwnedCopyImageAdmission, crate::IsbImageAdmissionError> {
        use crate::IsbImageAdmissionError as Error;
        ring.check_offer(request.slot)
            .map_err(|_| Error::Resource(ContentStoreError::Malformed))?;
        let grant = ring.grant();
        let epoch = self
            .active
            .iter_mut()
            .find(|e| e.resources.grant() == grant)
            .ok_or(Error::Resource(ContentStoreError::Stale))?;
        if !ring.is_charged_to(&epoch.source_bytes) {
            return Err(Error::Resource(ContentStoreError::Stale));
        }
        let description = epoch
            .allocations
            .owned_copy_description(&request)
            .map_err(Error::Allocation)?;
        let bytes = usize::try_from(description.total_bytes)
            .map_err(|_| Error::Resource(ContentStoreError::Malformed))?;
        if bytes > ring.geometry().pixel_bytes() || request.slot.offer == 0 {
            return Err(Error::Resource(ContentStoreError::Malformed));
        }
        let job = worker.reserve(bytes, deadline).map_err(Error::Worker)?;
        if let Err(error) = epoch.resources.begin_isb(
            sophia_protocol::TransactionId::from_raw(request.slot.offer),
            description.clone(),
            now,
        ) {
            worker.release_reserved(job);
            return Err(Error::Resource(error));
        }
        Ok(crate::OwnedCopyImageAdmission {
            metadata: crate::OwnedCopyImageMetadata {
                request,
                binding: ring.binding(),
                description,
            },
            worker_job: job,
            grant,
            deadline,
        })
    }

    /// Reserve from the same aggregate source/backing envelopes as resource and
    /// reconnect admission. This is storage custody, never export authorization.
    /// Full active resource ceilings remain fenced, so later accepted resources
    /// cannot consume capacity promised to this source or vice versa.
    pub fn reserve_owned_ring_source(
        &mut self,
        grant: ContentGrant,
        geometry: OwnedRingGeometry,
    ) -> Result<OwnedRingSourceReservation, ContentStoreError> {
        let epoch = self
            .active
            .iter()
            .find(|epoch| epoch.resources.grant() == grant)
            .ok_or(ContentStoreError::Stale)?;
        if epoch.resources.is_revoked() {
            return Err(ContentStoreError::Revoked);
        }
        let bytes = geometry.source_bytes() as u64;
        if self
            .reserved_bytes()
            .checked_add(bytes)
            .is_none_or(|n| n > self.max_bytes)
            || self
                .reserved_backing_bytes()
                .checked_add(bytes)
                .is_none_or(|n| n > self.max_backing_bytes)
        {
            return Err(ContentStoreError::Budget);
        }
        let used = epoch.source_bytes.clone();
        // The registry is the sole adding owner; workers can only drop charges.
        // Concurrent drops make these preceding aggregate checks conservative.
        used.fetch_add(bytes, Ordering::Relaxed);
        Ok(OwnedRingSourceReservation {
            grant,
            geometry,
            used,
        })
    }
}
