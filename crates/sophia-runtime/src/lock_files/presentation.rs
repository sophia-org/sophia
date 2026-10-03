//! Frame demands, permits and candidates for the current lock object. The
//! export checks a candidate against what Session published and permitted;
//! Session checks it again against what it presents. Every demand and every
//! forwarded candidate holds one journal credit until Session answers it.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use sophia_9p::Errno;
use sophia_protocol::lock_files::*;

use super::reason;
use super::resources::Resources;

struct Permit {
    pacing_permit: u64,
    allocation_generation: u64,
    lock_epoch: u64,
    expires: Instant,
}

/// What a candidate would do, decided before anything changes.
pub(super) enum CandidatePlan {
    /// Forwarded to Session, holding a credit for its outcome.
    Forward(LockCandidate),
    /// Refused here; its outcome is journaled with the receipt.
    Reject(LockCandidateOutcome),
}

#[derive(Default)]
pub(super) struct Presentation {
    /// One standing demand per allocation, each holding a credit.
    demands: BTreeMap<u64, LockFrameDemand>,
    /// One unexpired permit per allocation.
    permits: BTreeMap<u64, Permit>,
    /// Forwarded candidates by allocation and candidate generation, each
    /// holding a credit until Session reports a terminal outcome.
    in_flight: BTreeMap<(u64, u64), LockCandidate>,
    /// The newest candidate generation accepted for each allocation.
    generations: BTreeMap<u64, u64>,
    next_permit: u64,
}

fn allocation(lock: &LockObject, allocation_id: u64) -> Option<&LockAllocation> {
    lock.phase
        .covers()
        .then(|| {
            lock.allocations
                .iter()
                .find(|allocation| allocation.allocation_id == allocation_id)
        })
        .flatten()
}

impl Presentation {
    /// Credits held by standing demands and forwarded candidates.
    pub(super) fn credits(&self) -> usize {
        self.demands.len() + self.in_flight.len()
    }

    pub(super) fn expire(&mut self, now: Instant) {
        self.permits.retain(|_, permit| permit.expires > now);
    }

    pub(super) fn plan_candidate(
        &self,
        candidate: LockCandidate,
        lock: &LockObject,
        resources: &Resources,
        now: Instant,
    ) -> Result<CandidatePlan, Errno> {
        if self
            .generations
            .get(&candidate.allocation_id)
            .is_some_and(|newest| candidate.candidate_generation <= *newest)
        {
            return Err(Errno::EINVAL);
        }
        let reject = |reason| {
            CandidatePlan::Reject(LockCandidateOutcome {
                transaction: candidate.transaction,
                lock_epoch: candidate.lock_epoch,
                output_id: candidate.output_id,
                allocation_id: candidate.allocation_id,
                candidate_generation: candidate.candidate_generation,
                status: LockCandidateStatus::Rejected,
                reason,
            })
        };
        if !lock.phase.covers() || candidate.lock_epoch != lock.lock_epoch {
            return Ok(reject(reason::STALE_LOCK));
        }
        let Some(target) = allocation(lock, candidate.allocation_id).filter(|target| {
            target.allocation_generation == candidate.allocation_generation
                && target.output_id == candidate.output_id
                && target.output_generation == candidate.output_generation
        }) else {
            return Ok(reject(reason::STALE_ALLOCATION));
        };
        let Some(image) = resources.image(candidate.resource) else {
            return Ok(reject(reason::UNKNOWN_RESOURCE));
        };
        if image.width_px != target.pixel_width || image.height_px != target.pixel_height {
            return Ok(reject(reason::SIZE_MISMATCH));
        }
        let permitted = self
            .permits
            .get(&candidate.allocation_id)
            .is_some_and(|permit| {
                permit.pacing_permit == candidate.pacing_permit
                    && permit.allocation_generation == candidate.allocation_generation
                    && permit.lock_epoch == candidate.lock_epoch
                    && permit.expires > now
            });
        if !permitted {
            return Ok(reject(reason::PERMIT));
        }
        Ok(CandidatePlan::Forward(candidate))
    }

    /// Applies a journaled plan; `Forward` spends the allocation's permit.
    pub(super) fn apply_candidate(&mut self, plan: &CandidatePlan) {
        let candidate = match plan {
            CandidatePlan::Forward(candidate) => candidate,
            CandidatePlan::Reject(outcome) => {
                self.generations
                    .insert(outcome.allocation_id, outcome.candidate_generation);
                return;
            }
        };
        self.permits.remove(&candidate.allocation_id);
        self.generations
            .insert(candidate.allocation_id, candidate.candidate_generation);
        self.in_flight.insert(
            (candidate.allocation_id, candidate.candidate_generation),
            *candidate,
        );
    }

    /// A demand for a current allocation; `true` when it adds a credit
    /// rather than replacing a standing demand.
    pub(super) fn plan_demand(
        &self,
        demand: LockFrameDemand,
        lock: &LockObject,
    ) -> Result<bool, Errno> {
        let current = lock.lock_epoch == demand.lock_epoch
            && allocation(lock, demand.allocation_id)
                .is_some_and(|target| target.allocation_generation == demand.allocation_generation);
        if !current {
            return Err(Errno::ESTALE);
        }
        Ok(!self.demands.contains_key(&demand.allocation_id))
    }

    pub(super) fn apply_demand(&mut self, demand: LockFrameDemand) {
        self.demands.insert(demand.allocation_id, demand);
    }

    /// Session's answer to a standing demand: a permit Session may pace.
    pub(super) fn plan_permit(
        &self,
        allocation_id: u64,
        demand_id: u64,
        expires_after: Duration,
    ) -> Result<LockFramePermit, Errno> {
        let demand = self
            .demands
            .get(&allocation_id)
            .filter(|demand| demand.demand_id == demand_id)
            .ok_or(Errno::EINVAL)?;
        let expires_after_ms = u32::try_from(expires_after.as_millis())
            .ok()
            .filter(|ms| (1..=250).contains(ms))
            .ok_or(Errno::EINVAL)?;
        Ok(LockFramePermit {
            lock_epoch: demand.lock_epoch,
            allocation_id,
            allocation_generation: demand.allocation_generation,
            demand_id,
            pacing_permit: self.next_permit.checked_add(1).ok_or(Errno::ENOSPC)?,
            expires_after_ms,
        })
    }

    pub(super) fn apply_permit(&mut self, permit: LockFramePermit, now: Instant) {
        self.demands.remove(&permit.allocation_id);
        self.next_permit = permit.pacing_permit;
        self.permits.insert(
            permit.allocation_id,
            Permit {
                pacing_permit: permit.pacing_permit,
                allocation_generation: permit.allocation_generation,
                lock_epoch: permit.lock_epoch,
                expires: now + Duration::from_millis(permit.expires_after_ms.into()),
            },
        );
    }

    /// Whether Session's outcome names a forwarded candidate. A terminal
    /// outcome spends its credit; `prepared` does not.
    pub(super) fn plan_outcome(&self, outcome: &LockCandidateOutcome) -> Result<bool, Errno> {
        self.in_flight
            .get(&(outcome.allocation_id, outcome.candidate_generation))
            .filter(|candidate| {
                candidate.transaction == outcome.transaction
                    && candidate.lock_epoch == outcome.lock_epoch
                    && candidate.output_id == outcome.output_id
            })
            .map(|_| outcome.status != LockCandidateStatus::Prepared)
            .ok_or(Errno::EINVAL)
    }

    pub(super) fn apply_outcome(&mut self, outcome: &LockCandidateOutcome) {
        if outcome.status != LockCandidateStatus::Prepared {
            self.in_flight
                .remove(&(outcome.allocation_id, outcome.candidate_generation));
        }
    }

    /// A new lock object: demands and permits for allocations it no longer
    /// grants lapse, and forwarded candidates for them are revoked. Returns
    /// the revocations to journal, each spending its candidate's credit.
    pub(super) fn plan_publication(&self, lock: &LockObject) -> Vec<LockCandidateOutcome> {
        self.in_flight
            .values()
            .filter(|candidate| {
                candidate.lock_epoch != lock.lock_epoch
                    || allocation(lock, candidate.allocation_id).is_none_or(|target| {
                        target.allocation_generation != candidate.allocation_generation
                    })
            })
            .map(|candidate| LockCandidateOutcome {
                transaction: candidate.transaction,
                lock_epoch: candidate.lock_epoch,
                output_id: candidate.output_id,
                allocation_id: candidate.allocation_id,
                candidate_generation: candidate.candidate_generation,
                status: LockCandidateStatus::Revoked,
                reason: reason::STALE_LOCK,
            })
            .collect()
    }

    /// Returns how many demand credits lapsed with the old allocations.
    pub(super) fn apply_publication(
        &mut self,
        lock: &LockObject,
        revoked: &[LockCandidateOutcome],
    ) -> usize {
        for outcome in revoked {
            self.in_flight
                .remove(&(outcome.allocation_id, outcome.candidate_generation));
        }
        let current = |allocation_id: u64, generation: u64, epoch: u64| {
            epoch == lock.lock_epoch
                && allocation(lock, allocation_id)
                    .is_some_and(|target| target.allocation_generation == generation)
        };
        let before = self.demands.len();
        self.demands
            .retain(|id, demand| current(*id, demand.allocation_generation, demand.lock_epoch));
        self.permits
            .retain(|id, permit| current(*id, permit.allocation_generation, permit.lock_epoch));
        before - self.demands.len()
    }
}
