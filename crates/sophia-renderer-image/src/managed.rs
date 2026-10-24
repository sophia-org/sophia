//! Logical custody has no descriptor or physical-budget ownership.
use crate::{RendererImageId, SnapshotEpoch};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedCaptureInfo {
    pub image_id: RendererImageId,
    pub allocation_id: u64,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

impl ManagedCaptureInfo {
    pub fn is_valid(&self) -> bool {
        self.image_id.is_valid()
            && self.allocation_id != 0
            && self.generation != 0
            && self.width != 0
            && self.height != 0
            && i32::try_from(self.width).is_ok()
            && i32::try_from(self.height).is_ok()
            && u64::from(self.width)
                .checked_mul(u64::from(self.height))
                .and_then(|n| n.checked_mul(4))
                .is_some_and(|n| n <= self.bytes)
    }
}

#[derive(Debug)]
pub struct ManagedCaptureLease {
    info: ManagedCaptureInfo,
    epoch: Arc<SnapshotEpoch>,
    valid: AtomicBool,
    retired: AtomicBool,
    wake: sophia_wake::Notifier,
}

impl ManagedCaptureLease {
    /// The renderer installs the corresponding promoted image before handing
    /// this storage handle to its opaque completion. This is not a proof.
    pub fn new(
        info: ManagedCaptureInfo,
        epoch: Arc<SnapshotEpoch>,
        wake: sophia_wake::Notifier,
    ) -> Result<Arc<Self>, crate::SnapshotError> {
        if !info.is_valid() || !epoch.is_valid() {
            return Err(crate::SnapshotError::InvalidRendererImageId);
        }
        Ok(Arc::new(Self {
            info,
            epoch,
            valid: AtomicBool::new(true),
            retired: AtomicBool::new(false),
            wake,
        }))
    }
    pub fn info(&self) -> &ManagedCaptureInfo {
        &self.info
    }
    pub fn epoch(&self) -> &Arc<SnapshotEpoch> {
        &self.epoch
    }
    pub fn is_current(&self) -> bool {
        self.valid.load(Ordering::Acquire) && self.epoch.is_valid()
    }
    pub fn retire(&self) {
        self.retired.store(true, Ordering::Release);
        self.wake.notify();
    }
    pub fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }
    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
        self.wake.notify();
    }
}

impl Drop for ManagedCaptureLease {
    fn drop(&mut self) {
        self.wake.notify();
    }
}
