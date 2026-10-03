//! What the lock provider's images become on screen (t294): Session's side
//! of frame demands, candidates and their outcomes.
//!
//! Custody has already checked every candidate against the lock object, its
//! allocation, the image's size and its permit. This keeps what Engine draws:
//! one placement per output for the current lock, the candidate waiting to
//! be seen on every head, and the demands waiting for a permit. A permit is
//! granted only while the allocation has nothing in flight, so presentation
//! itself paces the provider at its slowest head.
use std::collections::BTreeMap;
use std::sync::Arc;

use sophia_engine::{
    SessionLockEpoch, SessionLockImage, SessionLockImageIdentity, SessionLockImagePlacement,
};
use sophia_protocol::OutputId;
use sophia_protocol::lock_files::{
    LockCandidate, LockCandidateOutcome, LockCandidateStatus, LockFrameDemand, LockResourceId,
};

/// The contract's reason for an outcome nothing went wrong with.
const NONE: u16 = 0;

struct Resource {
    width_px: u32,
    height_px: u32,
    pixels: Arc<[u8]>,
}

#[derive(Default)]
pub struct SessionLockFrames {
    epoch: Option<SessionLockEpoch>,
    connection: Option<u64>,
    resources: BTreeMap<LockResourceId, Resource>,
    shown: BTreeMap<OutputId, SessionLockImagePlacement>,
    /// The candidate each output shows but every head has not yet retired.
    in_flight: BTreeMap<OutputId, LockCandidate>,
    /// Standing demands by allocation, waiting for a permit.
    demands: BTreeMap<u64, LockFrameDemand>,
}

fn outcome(candidate: &LockCandidate, status: LockCandidateStatus) -> LockCandidateOutcome {
    LockCandidateOutcome {
        transaction: candidate.transaction,
        lock_epoch: candidate.lock_epoch,
        output_id: candidate.output_id,
        allocation_id: candidate.allocation_id,
        candidate_generation: candidate.candidate_generation,
        status,
        reason: NONE,
    }
}

impl SessionLockFrames {
    /// The images Engine draws over the fill, one per output.
    pub fn images(&self) -> Arc<BTreeMap<OutputId, SessionLockImagePlacement>> {
        Arc::new(self.shown.clone())
    }

    /// A new lock, or none: nothing drawn for an earlier lock is shown for
    /// this one. Custody revokes the candidates it had forwarded.
    pub fn lock(&mut self, epoch: Option<SessionLockEpoch>) -> bool {
        if self.epoch == epoch {
            return false;
        }
        self.epoch = epoch;
        let changed = !self.shown.is_empty();
        self.shown.clear();
        self.in_flight.clear();
        self.demands.clear();
        changed
    }

    /// A provider connection began. Its predecessor's images, resources and
    /// demands are gone with it.
    pub fn connected(&mut self, connection_epoch: u64) -> bool {
        self.connection = Some(connection_epoch);
        self.forget()
    }

    /// The provider left. Returns whether the drawn images changed.
    pub fn disconnected(&mut self, connection_epoch: u64) -> bool {
        if self.connection != Some(connection_epoch) {
            return false;
        }
        self.connection = None;
        self.forget()
    }

    fn forget(&mut self) -> bool {
        let changed = !self.shown.is_empty();
        self.resources.clear();
        self.shown.clear();
        self.in_flight.clear();
        self.demands.clear();
        changed
    }

    fn current(&self, connection_epoch: u64) -> bool {
        self.connection == Some(connection_epoch)
    }

    pub fn resource_ready(
        &mut self,
        connection_epoch: u64,
        resource: LockResourceId,
        width_px: u32,
        height_px: u32,
        pixels: Arc<[u8]>,
    ) {
        if self.current(connection_epoch) {
            self.resources.insert(
                resource,
                Resource {
                    width_px,
                    height_px,
                    pixels,
                },
            );
        }
    }

    /// A retired resource is never placed again; one already on screen stays
    /// there until a candidate replaces it, holding its own pixels.
    pub fn resource_retired(&mut self, connection_epoch: u64, resource: LockResourceId) {
        if self.current(connection_epoch) {
            self.resources.remove(&resource);
        }
    }

    pub fn demand(&mut self, connection_epoch: u64, demand: LockFrameDemand) {
        if self.current(connection_epoch) {
            self.demands.insert(demand.allocation_id, demand);
        }
    }

    /// Demands that may be permitted now: their allocation shows nothing
    /// unretired. Each is taken; custody owes the provider one permit each.
    pub fn permits(&mut self) -> Vec<LockFrameDemand> {
        let busy = |demand: &LockFrameDemand| {
            self.in_flight
                .values()
                .any(|candidate| candidate.allocation_id == demand.allocation_id)
        };
        let ready: Vec<_> = self
            .demands
            .values()
            .filter(|demand| !busy(demand))
            .copied()
            .collect();
        for demand in &ready {
            self.demands.remove(&demand.allocation_id);
        }
        ready
    }

    /// Places a candidate's image over its output. Returns the outcomes owed
    /// now: an earlier candidate on that output that no head showed yet is
    /// superseded, and one that cannot be placed is rejected. `true` when the
    /// drawn images changed.
    pub fn candidate(
        &mut self,
        connection_epoch: u64,
        candidate: LockCandidate,
    ) -> (bool, Vec<LockCandidateOutcome>) {
        let current_lock = self
            .epoch
            .is_some_and(|epoch| epoch.raw() == candidate.lock_epoch);
        let resource = self.resources.get(&candidate.resource);
        let (Some(resource), true, true) = (resource, self.current(connection_epoch), current_lock)
        else {
            return (
                false,
                vec![outcome(&candidate, LockCandidateStatus::Rejected)],
            );
        };
        let output = OutputId::from_raw(candidate.output_id);
        let image = SessionLockImage {
            identity: SessionLockImageIdentity {
                output,
                connection_epoch,
                resource_id: candidate.resource.id,
                resource_generation: candidate.resource.generation,
            },
            width_px: resource.width_px,
            height_px: resource.height_px,
            pixels: Arc::clone(&resource.pixels),
        };
        let mut owed = Vec::new();
        if let Some(previous) = self.in_flight.insert(output, candidate) {
            owed.push(outcome(&previous, LockCandidateStatus::Superseded));
        }
        self.shown.insert(
            output,
            SessionLockImagePlacement {
                image,
                generation: candidate.candidate_generation,
            },
        );
        (true, owed)
    }

    /// Outputs with a candidate waiting to be seen on every head.
    pub fn waiting(&self) -> impl Iterator<Item = OutputId> + '_ {
        self.in_flight.keys().copied()
    }

    /// What every head of `output` retired: `Presented` once it is the
    /// candidate in flight.
    pub fn presented(
        &mut self,
        output: OutputId,
        shown: Option<(SessionLockImageIdentity, u64)>,
    ) -> Option<LockCandidateOutcome> {
        let candidate = self.in_flight.get(&output)?;
        let (identity, generation) = shown?;
        let matches = identity.output == output
            && Some(identity.connection_epoch) == self.connection
            && identity.resource_id == candidate.resource.id
            && identity.resource_generation == candidate.resource.generation
            && generation == candidate.candidate_generation;
        if !matches {
            return None;
        }
        let candidate = self.in_flight.remove(&output)?;
        Some(outcome(&candidate, LockCandidateStatus::Presented))
    }
}
