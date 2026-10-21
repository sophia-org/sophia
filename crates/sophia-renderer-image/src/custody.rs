//! Count the backing storage once, until the last frame, import or scanout
//! lease releases it. Invalidating a generation does not release its budget.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{RendererImageSnapshot, SnapshotError as Detail};

#[derive(Debug)]
pub struct SnapshotEpoch {
    pub card: u64,
    pub inventory_generation: u64,
    pub context: u64,
    valid: AtomicBool,
}

impl SnapshotEpoch {
    pub fn new(card: u64, inventory_generation: u64, context: u64) -> Arc<Self> {
        Arc::new(Self {
            card,
            inventory_generation,
            context,
            valid: AtomicBool::new(true),
        })
    }

    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }
    pub fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub struct SnapshotBudget {
    limit_count: usize,
    limit_bytes: u64,
    usage: Mutex<(usize, u64)>,
}

#[derive(Debug)]
pub(crate) struct SnapshotCharge {
    budget: Arc<SnapshotBudget>,
    bytes: u64,
    cacheable: AtomicBool,
}

/// Capacity held before capture starts. This is accounting, not proof that GPU
/// reads have finished. An executor must retain it through uncertain work.
///
/// Reservations cannot duplicate physical capacity:
/// ```compile_fail
/// use sophia_renderer_image::SnapshotBudget;
/// let budget = SnapshotBudget::new(1, 64);
/// let reservation = budget.reserve_capture(64).unwrap();
/// let duplicate = reservation.clone();
/// ```
#[cfg(feature = "experimental-shell-capture")]
#[derive(Debug)]
#[must_use]
pub struct SnapshotReservation {
    charge: Arc<SnapshotCharge>,
}

#[cfg(feature = "experimental-shell-capture")]
impl SnapshotReservation {
    pub fn byte_bound(&self) -> u64 {
        self.charge.bytes
    }

    /// Exact budget identity for the renderer's pre-submit admission check.
    /// This borrows accounting authority; it does not transfer or renew it.
    pub fn belongs_to(&self, budget: &Arc<SnapshotBudget>) -> bool {
        Arc::ptr_eq(&self.charge.budget, budget)
    }
}

/// Refusal returns both the image and its reservation: the caller still owns
/// cleanup, including any submitted GPU work. No budget is returned implicitly.
#[cfg(feature = "experimental-shell-capture")]
#[derive(Debug)]
#[must_use]
pub struct SnapshotRetentionFailure {
    pub detail: Detail,
    pub snapshot: RendererImageSnapshot,
    pub reservation: SnapshotReservation,
}

impl Drop for SnapshotCharge {
    fn drop(&mut self) {
        let mut usage = self.budget.usage.lock().unwrap_or_else(|e| e.into_inner());
        usage.0 -= 1;
        usage.1 -= self.bytes;
    }
}

impl SnapshotBudget {
    pub fn new(limit_count: usize, limit_bytes: u64) -> Arc<Self> {
        Arc::new(Self {
            limit_count,
            limit_bytes,
            usage: Mutex::new((0, 0)),
        })
    }

    pub fn usage(&self) -> (usize, u64) {
        *self.usage.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reserve a proven upper bound on the destination allocation, not just
    /// width * height * 4. No source may be submitted before this succeeds.
    #[cfg(feature = "experimental-shell-capture")]
    pub fn reserve_capture(
        self: &Arc<Self>,
        byte_bound: u64,
    ) -> Result<SnapshotReservation, Detail> {
        self.reserve(byte_bound)
            .map(|charge| SnapshotReservation { charge })
    }

    /// Transfer pre-capture capacity into an immutable snapshot without a
    /// second charge. This operation does not prove capture completion.
    #[cfg(feature = "experimental-shell-capture")]
    pub fn retain_reserved(
        self: &Arc<Self>,
        mut snapshot: RendererImageSnapshot,
        epoch: Arc<SnapshotEpoch>,
        mut reservation: SnapshotReservation,
    ) -> Result<Arc<RendererImageSnapshot>, SnapshotRetentionFailure> {
        let checked = (|| {
            if !epoch.is_valid()
                || snapshot.charge.is_some()
                || !Arc::ptr_eq(self, &reservation.charge.budget)
            {
                return Err(Detail::InvalidRendererImageId);
            }
            let bytes = snapshot.allocation_bytes()?;
            if bytes == 0 || bytes > reservation.byte_bound() {
                return Err(Detail::RendererImageStoreFull);
            }
            Ok(bytes)
        })();
        let bytes = match checked {
            Ok(bytes) => bytes,
            Err(detail) => {
                return Err(SnapshotRetentionFailure {
                    detail,
                    snapshot,
                    reservation,
                });
            }
        };
        // The reservation is intentionally non-cloneable, so no second owner
        // can retain the old bound or transfer the same credit twice.
        let Some(charge) = Arc::get_mut(&mut reservation.charge) else {
            return Err(SnapshotRetentionFailure {
                detail: Detail::InvalidRendererImageId,
                snapshot,
                reservation,
            });
        };
        let mut usage = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        usage.1 -= charge.bytes - bytes;
        charge.bytes = bytes;
        drop(usage);
        snapshot.charge = Some(reservation.charge);
        snapshot.epoch = Some(epoch);
        Ok(Arc::new(snapshot))
    }

    fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Arc<SnapshotCharge>, Detail> {
        let mut usage = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        if bytes == 0
            || usage.0 >= self.limit_count
            || bytes > self.limit_bytes.saturating_sub(usage.1)
        {
            return Err(Detail::RendererImageStoreFull);
        }
        usage.0 += 1;
        usage.1 += bytes;
        Ok(Arc::new(SnapshotCharge {
            budget: self.clone(),
            bytes,
            cacheable: AtomicBool::new(true),
        }))
    }

    pub fn retain(
        self: &Arc<Self>,
        mut snapshot: RendererImageSnapshot,
        epoch: Arc<SnapshotEpoch>,
    ) -> Result<Arc<RendererImageSnapshot>, Detail> {
        if !epoch.is_valid() || snapshot.charge.is_some() {
            return Err(Detail::InvalidRendererImageId);
        }
        let bytes = snapshot.allocation_bytes()?;
        snapshot.charge = Some(self.reserve(bytes)?);
        snapshot.epoch = Some(epoch);
        Ok(Arc::new(snapshot))
    }
}

impl RendererImageSnapshot {
    fn allocation_bytes(&self) -> Result<u64, Detail> {
        // dma-buf seek reports allocation size, including modifier padding.
        // Several planes may share one allocation; charge each inode once.
        let mut allocations = std::collections::BTreeSet::new();
        let mut bytes = 0_u64;
        for plane in self.planes.iter().flatten() {
            let stat = rustix::fs::fstat(&plane.fd).map_err(|_| Detail::InvalidBufferDescriptor)?;
            if allocations.insert((stat.st_dev, stat.st_ino)) {
                let size = rustix::fs::seek(&plane.fd, rustix::fs::SeekFrom::End(0))
                    .map_err(|_| Detail::InvalidBufferDescriptor)?;
                bytes = bytes
                    .checked_add(size)
                    .ok_or(Detail::InvalidBufferDescriptor)?;
            }
        }
        Ok(bytes)
    }
    /// Queued frames remain valid after demand ends; only their imports become
    /// transient. This prevents a delayed frame from repopulating a long-lived
    /// cache after the owner's broadcast eviction has already run.
    pub fn retire_import_cache(&self) {
        if let Some(charge) = &self.charge {
            charge.cacheable.store(false, Ordering::Release);
        }
    }
    pub fn import_cacheable(&self) -> bool {
        self.is_current()
            && self
                .charge
                .as_ref()
                .is_none_or(|charge| charge.cacheable.load(Ordering::Acquire))
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/snapshot_custody.rs"
    ));
}

#[cfg(all(test, feature = "experimental-shell-capture"))]
mod reservation_tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/snapshot_reservation.rs"
    ));
}
