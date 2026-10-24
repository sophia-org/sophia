//! Exact, cancellable physical preparation; no source-completion authority.
use crate::{SnapshotBudget, SnapshotEpoch, SnapshotError as Detail, SnapshotReservation};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

/// Held by the allocation, including its idle, import and GPU keepalives.
#[derive(Clone, Debug)]
pub struct ManagedCaptureCharge(pub(crate) Arc<crate::custody::SnapshotCharge>);

impl ManagedCaptureCharge {
    pub fn bytes(&self) -> u64 {
        self.0.bytes
    }
    pub fn belongs_to(&self, budget: &Arc<SnapshotBudget>) -> bool {
        Arc::ptr_eq(&self.0.budget, budget)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedCaptureInfo {
    pub allocation_id: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub bound: u64,
}

#[derive(Debug)]
struct Preparation {
    info: PreparedCaptureInfo,
    budget: Arc<SnapshotBudget>,
    epoch: Arc<SnapshotEpoch>,
    charge: Option<ManagedCaptureCharge>,
    consumed: AtomicBool,
    wake: sophia_wake::Notifier,
}

impl Drop for Preparation {
    fn drop(&mut self) {
        // Consumption already removed the prepared entry on its owner turn.
        if !self.consumed.load(Ordering::Acquire) {
            self.wake.notify();
        }
    }
}

/// One claim on one prepared allocation. Dropping it wakes the owning worker.
#[derive(Debug)]
#[must_use]
pub struct PreparedCaptureDestination(Arc<Preparation>);

#[derive(Debug)]
pub struct PreparedCaptureWatch(Weak<Preparation>);

impl PreparedCaptureWatch {
    pub fn is_live(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|p| p.epoch.is_valid() && !p.consumed.load(Ordering::Acquire))
    }
    pub fn matches(&self, token: &PreparedCaptureDestination) -> bool {
        Weak::ptr_eq(&self.0, &Arc::downgrade(&token.0)) && self.is_live()
    }
}

impl PreparedCaptureDestination {
    pub fn new(
        info: PreparedCaptureInfo,
        budget: Arc<SnapshotBudget>,
        epoch: Arc<SnapshotEpoch>,
        charge: Option<ManagedCaptureCharge>,
        wake: sophia_wake::Notifier,
    ) -> Result<(Self, PreparedCaptureWatch), Detail> {
        if !epoch.is_valid()
            || info.allocation_id == 0
            || info.width == 0
            || info.height == 0
            || info.bytes == 0
            || info.bytes > info.bound
            || charge
                .as_ref()
                .is_some_and(|c| !c.belongs_to(&budget) || c.bytes() != info.bytes)
        {
            return Err(Detail::InvalidRendererImageId);
        }
        let state = Arc::new(Preparation {
            info,
            budget,
            epoch,
            charge,
            consumed: AtomicBool::new(false),
            wake,
        });
        let watch = PreparedCaptureWatch(Arc::downgrade(&state));
        Ok((Self(state), watch))
    }
    pub fn info(&self) -> &PreparedCaptureInfo {
        &self.0.info
    }
    pub fn epoch(&self) -> &Arc<SnapshotEpoch> {
        &self.0.epoch
    }
    pub fn budget(&self) -> &Arc<SnapshotBudget> {
        &self.0.budget
    }
    pub fn notifier(&self) -> sophia_wake::Notifier {
        self.0.wake.clone()
    }
    pub fn is_current(&self) -> bool {
        self.0.epoch.is_valid()
    }
    pub fn consume(&self) -> bool {
        self.is_current()
            && self
                .0
                .consumed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
    pub fn reserve(
        self,
        budget: &Arc<SnapshotBudget>,
    ) -> Result<SnapshotReservation, (Detail, Self)> {
        if !self.is_current()
            || !Arc::ptr_eq(budget, &self.0.budget)
            || self.0.consumed.load(Ordering::Acquire)
        {
            return Err((Detail::InvalidRendererImageId, self));
        }
        let charge = match &self.0.charge {
            Some(charge) => charge.0.clone(),
            None => match budget.reserve(self.0.info.bound) {
                Ok(charge) => charge,
                Err(error) => return Err((error, self)),
            },
        };
        Ok(SnapshotReservation {
            charge,
            prepared: Some(self),
            managed_bound: false,
        })
    }
}

#[derive(Debug)]
pub enum CaptureDestination {
    Exported(u64),
    Managed(PreparedCaptureDestination),
}

impl CaptureDestination {
    pub fn byte_bound(&self) -> u64 {
        match self {
            Self::Exported(bound) => *bound,
            Self::Managed(token) => token.info().bound,
        }
    }
    pub fn reserve(
        self,
        budget: &Arc<SnapshotBudget>,
    ) -> Result<SnapshotReservation, (Detail, Self)> {
        match self {
            Self::Exported(bound) => budget
                .reserve_capture(bound)
                .map_err(|e| (e, Self::Exported(bound))),
            Self::Managed(token) => token
                .reserve(budget)
                .map_err(|(e, token)| (e, Self::Managed(token))),
        }
    }
}

impl SnapshotReservation {
    /// Reconcile actual storage once. Reuse carries the existing allocation's
    /// charge; it neither reserves again nor refunds credit on logical retirement.
    pub fn retain_managed(
        mut self,
        budget: &Arc<SnapshotBudget>,
        epoch: &Arc<SnapshotEpoch>,
        bytes: u64,
    ) -> Result<ManagedCaptureCharge, (Detail, Self)> {
        match self.bind_managed(budget, epoch, bytes) {
            Ok(charge) => Ok(charge),
            Err(error) => Err((error, self)),
        }
    }

    /// Attach accounting to the exact prepared allocation before any GPU work.
    /// The request retains the same credit through uncertain source custody.
    pub fn bind_managed(
        &mut self,
        budget: &Arc<SnapshotBudget>,
        epoch: &Arc<SnapshotEpoch>,
        bytes: u64,
    ) -> Result<ManagedCaptureCharge, Detail> {
        let valid = self.prepared.as_ref().is_some_and(|p| {
            p.is_current()
                && Arc::ptr_eq(p.epoch(), epoch)
                && Arc::ptr_eq(p.budget(), budget)
                && p.0.consumed.load(Ordering::Acquire)
                && p.info().bytes == bytes
                && bytes > 0
                && bytes <= self.byte_bound()
        }) && self.belongs_to(budget);
        if !valid {
            return Err(Detail::InvalidRendererImageId);
        }
        if self.managed_bound {
            return if self.charge.bytes == bytes {
                Ok(ManagedCaptureCharge(self.charge.clone()))
            } else {
                Err(Detail::InvalidRendererImageId)
            };
        }
        let reused = self.prepared.as_ref().and_then(|p| p.0.charge.as_ref());
        if let Some(old) = reused {
            if !Arc::ptr_eq(&old.0, &self.charge) || old.bytes() != bytes {
                return Err(Detail::InvalidRendererImageId);
            }
        } else {
            let Some(charge) = Arc::get_mut(&mut self.charge) else {
                return Err(Detail::InvalidRendererImageId);
            };
            let mut usage = budget.usage.lock().unwrap_or_else(|e| e.into_inner());
            usage.1 -= charge.bytes - bytes;
            charge.bytes = bytes;
        }
        self.managed_bound = true;
        Ok(ManagedCaptureCharge(self.charge.clone()))
    }
}
