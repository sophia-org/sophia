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
    /// Counts since the lock or the provider connection began, by allocation
    /// (diagnostic only).
    pacing: BTreeMap<u64, PacingEntry>,
}

#[derive(Clone, Copy, Default)]
struct PacingEntry {
    /// The output the allocation's last candidate named.
    output: Option<OutputId>,
    /// The allocation generation its last demand or candidate carried.
    allocation_generation: Option<u64>,
    counts: SessionLockPacingCounts,
}

/// How far each allocation's frames got through demand, permit, candidate and
/// outcome since the lock or the provider connection began.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionLockPacingCounts {
    pub demands: u64,
    pub permits: u64,
    pub candidates: u64,
    pub presented: u64,
    pub superseded: u64,
    pub rejected: u64,
}

/// One allocation's pacing as it stands. A provider that stopped asking shows
/// no demand and nothing in flight; a candidate that never retired shows the
/// same in-flight generation in every sample. The lock, connection and
/// allocation generation tell a new lock, a reconnect or a recycled
/// allocation (counts restart) from an allocation that made no progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLockPacing {
    pub lock_epoch: Option<u64>,
    pub connection_epoch: Option<u64>,
    pub allocation_id: u64,
    pub allocation_generation: Option<u64>,
    pub output: Option<OutputId>,
    pub demand_held: bool,
    pub in_flight_generation: Option<u64>,
    pub counts: SessionLockPacingCounts,
}

/// The diagnostic pacing sample (SOPHIA_DIAGNOSTIC_LOCK_PACING) is on only for
/// exactly "1".
pub fn session_lock_pacing_enabled(opt_in: Option<&str>) -> bool {
    opt_in == Some("1")
}

pub fn session_lock_pacing_record(pacing: &SessionLockPacing) -> String {
    let counts = pacing.counts;
    format!(
        "sophia_live_lock_pacing schema=1 lock_epoch={} connection_epoch={} allocation={} allocation_generation={} output={} demand={} in_flight_generation={} demands={} permits={} candidates={} presented={} superseded={} rejected={}",
        optional(pacing.lock_epoch),
        optional(pacing.connection_epoch),
        pacing.allocation_id,
        optional(pacing.allocation_generation),
        optional(pacing.output.map(OutputId::raw)),
        if pacing.demand_held { "held" } else { "none" },
        optional(pacing.in_flight_generation),
        counts.demands,
        counts.permits,
        counts.candidates,
        counts.presented,
        counts.superseded,
        counts.rejected,
    )
}

fn optional(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| value.to_string())
}

/// Which provider command a full or closed queue dropped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockCommandKind {
    Permit,
    Outcome,
    Other,
}

/// Commands the provider never received. Each drop is counted; the first and
/// every power of two of each kind is recorded, so a flood stays legible.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionLockCommandDrops {
    pub permits: u64,
    pub outcomes: u64,
    pub others: u64,
}

impl SessionLockCommandDrops {
    pub fn dropped(&mut self, kind: SessionLockCommandKind) -> Option<String> {
        let (count, name) = match kind {
            SessionLockCommandKind::Permit => (&mut self.permits, "permit"),
            SessionLockCommandKind::Outcome => (&mut self.outcomes, "outcome"),
            SessionLockCommandKind::Other => (&mut self.others, "other"),
        };
        *count = count.saturating_add(1);
        count.is_power_of_two().then(|| {
            format!(
                "sophia_live_lock_provider schema=1 status=command_dropped kind={name} dropped={count}"
            )
        })
    }
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
        self.pacing.clear();
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
        self.pacing.clear();
        changed
    }

    fn entry(&mut self, allocation: u64) -> &mut PacingEntry {
        self.pacing.entry(allocation).or_default()
    }

    /// Every allocation seen since the lock or connection began, as it stands.
    pub fn pacing(&self) -> Vec<SessionLockPacing> {
        self.pacing
            .iter()
            .map(|(&allocation_id, entry)| SessionLockPacing {
                lock_epoch: self.epoch.map(SessionLockEpoch::raw),
                connection_epoch: self.connection,
                allocation_id,
                allocation_generation: entry.allocation_generation,
                output: entry.output,
                demand_held: self.demands.contains_key(&allocation_id),
                in_flight_generation: self
                    .in_flight
                    .values()
                    .find(|candidate| candidate.allocation_id == allocation_id)
                    .map(|candidate| candidate.candidate_generation),
                counts: entry.counts,
            })
            .collect()
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
            let entry = self.entry(demand.allocation_id);
            entry.allocation_generation = Some(demand.allocation_generation);
            entry.counts.demands = entry.counts.demands.saturating_add(1);
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
            let counts = &mut self.entry(demand.allocation_id).counts;
            counts.permits = counts.permits.saturating_add(1);
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
            let counts = &mut self.entry(candidate.allocation_id).counts;
            counts.rejected = counts.rejected.saturating_add(1);
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
        let entry = self.entry(candidate.allocation_id);
        entry.output = Some(output);
        entry.allocation_generation = Some(candidate.allocation_generation);
        entry.counts.candidates = entry.counts.candidates.saturating_add(1);
        if let Some(previous) = self.in_flight.insert(output, candidate) {
            let counts = &mut self.entry(previous.allocation_id).counts;
            counts.superseded = counts.superseded.saturating_add(1);
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
        let counts = &mut self.entry(candidate.allocation_id).counts;
        counts.presented = counts.presented.saturating_add(1);
        Some(outcome(&candidate, LockCandidateStatus::Presented))
    }
}
