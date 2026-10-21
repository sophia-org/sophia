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
}

impl Drop for OwnedRingSourceReservation {
    fn drop(&mut self) {
        self.used
            .fetch_sub(self.geometry.source_bytes() as u64, Ordering::Release);
    }
}

impl ContentEpochRegistry {
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
