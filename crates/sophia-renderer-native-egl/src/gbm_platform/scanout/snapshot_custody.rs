//! Count the backing storage once, until the last frame, import or scanout
//! lease releases it. Invalidating a generation does not release its budget.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::{NativeGbmScanoutBufferExportDetail as Detail, NativeRendererImageSnapshot};

#[derive(Debug)]
pub struct NativeRendererSnapshotEpoch {
    pub card: u64,
    pub inventory_generation: u64,
    pub context: u64,
    valid: AtomicBool,
}

impl NativeRendererSnapshotEpoch {
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
pub struct NativeRendererSnapshotBudget {
    limit_count: usize,
    limit_bytes: u64,
    usage: Mutex<(usize, u64)>,
}

#[derive(Debug)]
pub(super) struct SnapshotCharge {
    budget: Arc<NativeRendererSnapshotBudget>,
    bytes: u64,
    cacheable: AtomicBool,
}

impl Drop for SnapshotCharge {
    fn drop(&mut self) {
        let mut usage = self.budget.usage.lock().unwrap_or_else(|e| e.into_inner());
        usage.0 -= 1;
        usage.1 -= self.bytes;
    }
}

impl NativeRendererSnapshotBudget {
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
        mut snapshot: NativeRendererImageSnapshot,
        epoch: Arc<NativeRendererSnapshotEpoch>,
    ) -> Result<Arc<NativeRendererImageSnapshot>, Detail> {
        if !epoch.is_valid() || snapshot.charge.is_some() {
            return Err(Detail::InvalidRendererImageId);
        }
        // dma-buf seek reports allocation size, including modifier padding.
        // Several planes may share one allocation; charge each inode once.
        let mut allocations = std::collections::BTreeSet::new();
        let mut bytes = 0_u64;
        for plane in snapshot.planes.iter().flatten() {
            let stat = rustix::fs::fstat(&plane.fd).map_err(|_| Detail::InvalidBufferDescriptor)?;
            if allocations.insert((stat.st_dev, stat.st_ino)) {
                let size = rustix::fs::seek(&plane.fd, rustix::fs::SeekFrom::End(0))
                    .map_err(|_| Detail::InvalidBufferDescriptor)?;
                bytes = bytes
                    .checked_add(size)
                    .ok_or(Detail::InvalidBufferDescriptor)?;
            }
        }
        snapshot.charge = Some(self.reserve(bytes)?);
        snapshot.epoch = Some(epoch);
        Ok(Arc::new(snapshot))
    }
}

impl NativeRendererImageSnapshot {
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
