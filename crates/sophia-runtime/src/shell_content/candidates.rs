#[cfg(not(feature = "experimental-native-frame-transaction"))]
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet};

use sophia_protocol::*;

use super::{
    ContentAllocationSnapshot, ContentResourceLease, ContentResourceStore, ContentStoreError,
};

#[cfg(feature = "experimental-native-frame-transaction")]
use super::{ContentRetireOnPresent, ContentRetireOwner, ContentRetireToken};

mod admission;
#[cfg(feature = "experimental-native-frame-transaction")]
mod credits;
mod demands;
mod presentation;
#[cfg(feature = "experimental-native-frame-transaction")]
use credits::CandidateEvents;
#[cfg(feature = "experimental-native-frame-transaction")]
pub use credits::{
    ContentFrameAdmission, ContentFrameCommit, ContentFrameCreditReturn,
    ContentFrameRequestAdmission, ContentFrameStreamEvent, ContentFrameStreamRecord,
};
#[cfg(feature = "experimental-native-frame-transaction")]
pub(crate) use credits::{FrameAdmissionReservation, FrameCommitState, FrameCopyBinding};
#[cfg(not(feature = "experimental-native-frame-transaction"))]
type CandidateEvents = VecDeque<ContentCandidateEvent>;
mod native;
mod native_close;
mod persistent;
#[cfg(feature = "experimental-native-frame-transaction")]
mod retire_on_present;
mod validation;
use super::ContentStoreProfile;
use demands::StandingDemand;
pub use native::{NativeLauncherCandidateBinding, NativeLauncherCandidateContext};
use persistent::CandidateAuthority;
pub use persistent::PersistentCatalogCandidateBinding;
use validation::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentCandidateError {
    Stale,
    Budget,
    Malformed,
    Incomplete,
    Timeout,
    AllocationLost,
    RendererFailed,
    ClockRegression,
    Cancelled,
}

impl ContentCandidateError {
    const fn reason(self) -> ContentReason {
        match self {
            Self::Stale => ContentReason::Stale,
            Self::Budget => ContentReason::Budget,
            Self::Malformed => ContentReason::Malformed,
            Self::Incomplete => ContentReason::Incomplete,
            Self::Timeout => ContentReason::Timeout,
            Self::AllocationLost => ContentReason::AllocationLost,
            Self::RendererFailed => ContentReason::RendererFailed,
            Self::ClockRegression => ContentReason::Malformed,
            Self::Cancelled => ContentReason::Cancelled,
        }
    }
}

impl From<ContentStoreError> for ContentCandidateError {
    fn from(value: ContentStoreError) -> Self {
        match value {
            ContentStoreError::Stale | ContentStoreError::Revoked => Self::Stale,
            ContentStoreError::Budget => Self::Budget,
            ContentStoreError::Malformed => Self::Malformed,
            ContentStoreError::Incomplete => Self::Incomplete,
            ContentStoreError::ClockRegression => Self::ClockRegression,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContentCandidateContext<'a> {
    pub output: ContentOutputId,
    pub facts_generation: u64,
    pub interaction_generation: u64,
    pub allocations: &'a [ContentAllocationSnapshot],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCandidateEvent {
    pub transaction: TransactionId,
    pub record: ShellContentRecord,
    /// The retire intent a queued Presented owns. A clone neither holds nor
    /// commits it: the store keeps the token until the event is taken, and
    /// commit is by owner identity, once, in the store.
    #[cfg(feature = "experimental-native-frame-transaction")]
    pub retire: Option<ContentRetireOwner>,
    /// Stream identity on every SFT outcome; only terminals can return a credit.
    #[cfg(feature = "experimental-native-frame-transaction")]
    pub frame_credit: Option<ContentFrameCreditReturn>,
}

struct Permit {
    transaction: TransactionId,
    demand_id: u64,
    permit_id: u64,
    deadline: u64,
}

struct Assembly {
    #[cfg(feature = "experimental-native-frame-transaction")]
    copy: Option<credits::FrameCopyState>,
    #[cfg(feature = "experimental-native-frame-transaction")]
    commit: Option<FrameCommitState>,
    native_launcher: Option<NativeLauncherCandidateBinding>,
    persistent_catalog: Option<PersistentCatalogCandidateBinding>,
    transaction: TransactionId,
    begin: ContentCandidateBegin,
    deadline: u64,
    next_ordinal: u32,
    surfaces: Vec<ContentSurface>,
    placements: Vec<ContentPlacement>,
    targets: Vec<ContentTarget>,
}

struct CandidateValidation {
    resources: Vec<(ContentResourceId, ContentResourceLease)>,
    deadline: u64,
}

struct Candidate {
    native_launcher: Option<NativeLauncherCandidateBinding>,
    persistent_catalog: Option<PersistentCatalogCandidateBinding>,
    transaction: TransactionId,
    begin: ContentCandidateBegin,
    surfaces: Vec<ContentSurface>,
    placements: Vec<ContentPlacement>,
    targets: Vec<ContentTarget>,
    resources: Vec<(ContentResourceId, ContentResourceLease)>,
    deadline: u64,
    prepared: bool,
    /// Reserved in the resource store under this candidate's owner identity.
    #[cfg(feature = "experimental-native-frame-transaction")]
    retire: Option<(ContentRetireOnPresent, ContentRetireToken)>,
}

/// Renderer-facing immutable candidate. Its resource leases remain real owners
/// until the renderer drops this value after native retirement.
#[derive(Clone)]
pub struct ContentRenderBundle {
    pub native_launcher: Option<NativeLauncherCandidateBinding>,
    pub persistent_catalog: Option<PersistentCatalogCandidateBinding>,
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub candidate_generation: u64,
    pub interaction_generation: u64,
    pub surfaces: Vec<ContentSurface>,
    pub placements: Vec<ContentPlacement>,
    pub targets: Vec<ContentTarget>,
    resources: Vec<(ContentResourceId, ContentResourceLease)>,
}

impl ContentRenderBundle {
    pub fn resource(&self, id: ContentResourceId) -> Option<&ContentResourceLease> {
        self.resources
            .iter()
            .find_map(|(candidate, lease)| (*candidate == id).then_some(lease))
    }
}

/// Bounded candidate admission for one content grant.
///
/// It owns pacing permits, incomplete assemblies, accepted pending candidates
/// and submitted non-cancellable candidates. It does not claim that a submitted
/// bundle was presented; only `presented` may emit that outcome, after the
/// caller has observed native retirement.
pub struct ContentCandidateStore {
    profile: ContentStoreProfile,
    limits: ContentLimits,
    permits: BTreeMap<ContentOutputId, Permit>,
    demands: BTreeMap<ContentOutputId, StandingDemand>,
    assemblies: BTreeMap<ContentOutputId, Assembly>,
    pending: BTreeMap<ContentOutputId, Candidate>,
    submitted: BTreeMap<ContentOutputId, Candidate>,
    events: CandidateEvents,
    #[cfg(feature = "experimental-native-frame-transaction")]
    streams: credits::CreditStreams,
    /// Retire tokens owned by queued Presented events.
    #[cfg(feature = "experimental-native-frame-transaction")]
    presented_retire: Vec<(ContentRetireOwner, ContentRetireToken)>,
    /// Reserved output records for obligations which have not settled yet.
    response_credits: usize,
    last_candidate_generation: u64,
    last_demand_id: u64,
    last_permit_id: u64,
    last_now: u64,
    revoked: bool,
}

impl ContentCandidateStore {
    pub(crate) fn first_submitted_identity(&self) -> Option<(ContentOutputId, u64)> {
        self.submitted
            .first_key_value()
            .map(|(output, candidate)| (*output, candidate.begin.candidate_generation))
    }

    pub(crate) fn add_accounting(&self, value: &mut super::ContentEpochAccounting) {
        value.candidates += self.assemblies.len() + self.pending.len() + self.submitted.len();
        value.permits += self.permits.len();
        value.demands += self.demands.len();
        value.response_records += self.control_occupancy();
    }

    pub(crate) fn control_occupancy(&self) -> usize {
        self.events.len() + self.response_credits
    }

    fn open_candidate_slots(&self) -> usize {
        let open = self.permits.len() + self.assemblies.len();
        #[cfg(feature = "experimental-native-frame-transaction")]
        let open = open + self.streams.entries.len()
            - self
                .assemblies
                .keys()
                .filter(|output| self.streams.entries.contains_key(output))
                .count();
        open
    }

    pub fn new(limits: ContentLimits) -> Result<Self, ContentCandidateError> {
        Self::with_profile(limits, ContentStoreProfile::Legacy)
    }

    pub fn with_profile(
        limits: ContentLimits,
        profile: ContentStoreProfile,
    ) -> Result<Self, ContentCandidateError> {
        limits
            .validate()
            .map_err(|_| ContentCandidateError::Malformed)?;
        Ok(Self {
            profile,
            limits,
            permits: BTreeMap::new(),
            demands: BTreeMap::new(),
            assemblies: BTreeMap::new(),
            pending: BTreeMap::new(),
            submitted: BTreeMap::new(),
            events: CandidateEvents::default(),
            #[cfg(feature = "experimental-native-frame-transaction")]
            streams: credits::CreditStreams::default(),
            #[cfg(feature = "experimental-native-frame-transaction")]
            presented_retire: Vec::new(),
            response_credits: 0,
            last_candidate_generation: 0,
            last_demand_id: 0,
            last_permit_id: 0,
            last_now: 0,
            revoked: false,
        })
    }

    /// Taking a queued Presented ends its hold on any retire intent; only a
    /// commit before the take applies it.
    pub fn take_event(&mut self) -> Option<ContentCandidateEvent> {
        #[cfg(feature = "experimental-native-frame-transaction")]
        if self.events.front()?.frame_credit.is_some() {
            return None;
        }
        let event = self.events.pop_front()?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        if let Some(owner) = event.retire {
            self.presented_retire.retain(|(held, _)| *held != owner);
        }
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.prune_closed_streams();
        Some(event)
    }

    pub fn pending_event(&self) -> Option<&ContentCandidateEvent> {
        self.events.front()
    }

    /// All queued control records, including an experimental stream head.
    pub fn pending_event_count(&self) -> usize {
        self.events.len()
    }

    pub fn pending_candidate_count(&self) -> usize {
        self.pending.len()
    }

    pub fn next_pending_candidate(&self) -> Option<(ContentOutputId, u64)> {
        self.next_pending_candidate_for(|_| true)
    }

    /// Read-only selection skips temporarily unavailable outputs without
    /// consuming or superseding their exact pending candidate.
    pub fn next_pending_candidate_for(
        &self,
        mut available: impl FnMut(ContentOutputId) -> bool,
    ) -> Option<(ContentOutputId, u64)> {
        self.pending.iter().find_map(|(output, candidate)| {
            available(*output).then_some((*output, candidate.begin.candidate_generation))
        })
    }

    pub fn submitted_candidate_count(&self) -> usize {
        self.submitted.len()
    }

    pub fn assembling_output(&self, candidate_generation: u64) -> Option<ContentOutputId> {
        self.assemblies.iter().find_map(|(output, assembly)| {
            (assembly.begin.candidate_generation == candidate_generation).then_some(*output)
        })
    }

    pub fn quiescent(&self) -> bool {
        #[cfg(feature = "experimental-native-frame-transaction")]
        if !self.streams.quiescent() {
            return false;
        }
        self.permits.is_empty()
            && self.demands.is_empty()
            && self.assemblies.is_empty()
            && self.pending.is_empty()
            && self.submitted.is_empty()
    }

    fn time(&mut self, now: u64) -> Result<(), ContentCandidateError> {
        if now < self.last_now {
            #[cfg(feature = "experimental-native-frame-transaction")]
            self.poison_frame_streams(now);
            return Err(ContentCandidateError::ClockRegression);
        }
        self.last_now = now;
        Ok(())
    }

    fn check_grant(&self, grant: ContentGrant) -> Result<(), ContentCandidateError> {
        if self.revoked || grant != self.limits.grant {
            Err(ContentCandidateError::Stale)
        } else {
            Ok(())
        }
    }

    fn push(&mut self, transaction: TransactionId, record: ShellContentRecord) {
        self.events.push_back(ContentCandidateEvent {
            transaction,
            record,
            #[cfg(feature = "experimental-native-frame-transaction")]
            retire: None,
            #[cfg(feature = "experimental-native-frame-transaction")]
            frame_credit: None,
        });
    }

    fn outcome(
        &mut self,
        transaction: TransactionId,
        generation: u64,
        output: ContentOutputId,
        kind: u16,
        reason: ContentReason,
        presentation_epoch: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
    ) {
        #[cfg(feature = "experimental-native-frame-transaction")]
        let frame_credit =
            self.frame_candidate_key(output, generation)
                .map(|key| ContentFrameCreditReturn {
                    key,
                    expires_msec: None,
                });
        self.push(
            transaction,
            ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
                grant: self.limits.grant,
                candidate_generation: generation,
                output,
                kind,
                reason: reason as u16,
                presentation_epoch,
                work_area_generation,
                wm_commit_generation,
            }),
        );
        #[cfg(feature = "experimental-native-frame-transaction")]
        {
            self.events.back_mut().expect("outcome queued").frame_credit = frame_credit;
        }
    }

    /// Install one Engine-issued, one-use permit. The caller owns demand
    /// coalescing; this method owns the permit's freshness and slot credit.
    pub fn grant_permit(
        &mut self,
        transaction: TransactionId,
        output: ContentOutputId,
        demand_id: u64,
        permit_id: u64,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(self.limits.grant)?;
        self.time(now)?;
        self.expire(now)?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.require_demand_mode(output)?;
        if !transaction.is_valid()
            || output.id == 0
            || output.generation == 0
            || demand_id == 0
            || permit_id <= self.last_permit_id
            || self.permits.contains_key(&output)
            || self.assemblies.contains_key(&output)
            || (self.pending.len() >= self.limits.max_pending_candidates_total as usize
                && !self.pending.contains_key(&output))
        {
            return Err(ContentCandidateError::Stale);
        }
        let open = self.open_candidate_slots();
        if open >= self.limits.max_open_candidates_total as usize {
            return Err(ContentCandidateError::Budget);
        }
        // One immediate grant, one possible Prepared and one terminal result.
        // Replacing pending work consumes one of its two reserved records for
        // Superseded and releases the other before reserving the new pair.
        let replacing = self.pending.contains_key(&output);
        let occupied = self.events.len() + self.response_credits - usize::from(replacing) * 2;
        let needed = 3 + usize::from(replacing);
        if occupied + needed > self.limits.max_control_records as usize {
            return Err(ContentCandidateError::Budget);
        }
        let deadline = now
            .checked_add(u64::from(self.limits.permit_timeout_ms))
            .ok_or(ContentCandidateError::Malformed)?;
        self.last_permit_id = permit_id;
        if let Some(candidate) = self.pending.remove(&output) {
            self.response_credits -= 2;
            self.outcome(
                candidate.transaction,
                candidate.begin.candidate_generation,
                output,
                4,
                ContentReason::Superseded,
                0,
                0,
                0,
            );
        }
        self.response_credits += 2;
        self.permits.insert(
            output,
            Permit {
                transaction,
                demand_id,
                permit_id,
                deadline,
            },
        );
        self.push(
            transaction,
            ShellContentRecord::FramePermit(ContentFramePermit {
                grant: self.limits.grant,
                output,
                demand_id,
                permit_id,
                state: 1,
                reason: 0,
                ttl_ms: self.limits.permit_timeout_ms,
                max_candidate_bytes: self.limits.max_candidate_bytes,
            }),
        );
        Ok(())
    }

    /// A valid Begin consumes the permit and creates a terminal-outcome debt.
    pub fn begin(
        &mut self,
        transaction: TransactionId,
        begin: ContentCandidateBegin,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::Legacy {
            return Err(ContentCandidateError::Malformed);
        }
        self.begin_inner(transaction, begin, None, None, None, now)
    }

    pub fn chunk(
        &mut self,
        transaction: TransactionId,
        chunk: ContentCandidateChunk,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::Legacy {
            return Err(ContentCandidateError::Malformed);
        }
        self.chunk_inner(transaction, chunk, now)
    }

    fn chunk_inner(
        &mut self,
        transaction: TransactionId,
        chunk: ContentCandidateChunk,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(chunk.grant)?;
        self.time(now)?;
        self.expire(now)?;
        let output = self
            .assemblies
            .iter()
            .find_map(|(output, assembly)| {
                (assembly.begin.candidate_generation == chunk.candidate_generation)
                    .then_some(*output)
            })
            .ok_or(ContentCandidateError::Stale)?;
        let assembly = self.assemblies.get_mut(&output).expect("selected assembly");
        #[cfg(feature = "experimental-native-frame-transaction")]
        if assembly.commit.is_some() {
            return Err(ContentCandidateError::Stale);
        }
        #[cfg(feature = "experimental-native-frame-transaction")]
        if let Some(error) = assembly.copy.as_ref().and_then(|copy| copy.stopped) {
            return Err(error);
        }
        let next_surfaces = assembly.surfaces.len() + chunk.surfaces.len();
        let next_placements = assembly.placements.len() + chunk.placements.len();
        let next_targets = assembly.targets.len() + chunk.targets.len();
        let data_bytes = 40usize
            .saturating_add(
                assembly
                    .native_launcher
                    .map_or(0, |binding| 108 + 2 * binding.rows().len()),
            )
            .saturating_add(next_surfaces.saturating_mul(64))
            .saturating_add(usize::from(assembly.persistent_catalog.is_some()) * 8)
            .saturating_add(next_placements.saturating_mul(32))
            .saturating_add(next_targets.saturating_mul(48));
        if !transaction.is_valid()
            || chunk.candidate_generation == 0
            || chunk.chunk_ordinal != assembly.next_ordinal
            || next_surfaces > assembly.begin.surface_count as usize
            || next_placements > assembly.begin.placement_count as usize
            || next_targets > assembly.begin.target_count as usize
            || data_bytes > self.limits.max_candidate_bytes as usize
            || !valid_chunk_rows(&chunk, self.limits.max_margin_logical, self.profile)
        {
            self.reject_assembly(output, ContentCandidateError::Malformed);
            return Err(ContentCandidateError::Malformed);
        }
        assembly.next_ordinal += 1;
        assembly.surfaces.extend(chunk.surfaces);
        assembly.placements.extend(chunk.placements);
        assembly.targets.extend(chunk.targets);
        Ok(())
    }

    pub fn end(
        &mut self,
        transaction: TransactionId,
        end: ContentCandidateEnd,
        context: ContentCandidateContext<'_>,
        resources: &ContentResourceStore,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::Legacy {
            return Err(ContentCandidateError::Malformed);
        }
        self.end_inner(
            transaction,
            end,
            context,
            CandidateAuthority::Legacy,
            resources,
            now,
        )
    }

    fn end_inner(
        &mut self,
        transaction: TransactionId,
        end: ContentCandidateEnd,
        context: ContentCandidateContext<'_>,
        authority: CandidateAuthority<'_>,
        resources: &ContentResourceStore,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(end.grant)?;
        self.time(now)?;
        self.expire(now)?;
        let output = self.end_output(end.candidate_generation)?;
        let result = self.validate_end(transaction, &end, context, authority, resources, output);
        match result {
            Ok(validation) => {
                let candidate = self.take_validated_candidate(output, validation);
                self.pending.insert(output, candidate);
                Ok(())
            }
            Err(error) => {
                self.reject_assembly(output, error);
                Err(error)
            }
        }
    }

    fn end_output(&self, generation: u64) -> Result<ContentOutputId, ContentCandidateError> {
        let (output, assembly) = self
            .assemblies
            .iter()
            .find(|(_, assembly)| assembly.begin.candidate_generation == generation)
            .ok_or(ContentCandidateError::Stale)?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        if assembly.copy.is_some() {
            // Waiting for handoff must not reject or poison the assembly.
            return Err(ContentCandidateError::Budget);
        }
        #[cfg(not(feature = "experimental-native-frame-transaction"))]
        let _ = assembly;
        Ok(*output)
    }

    fn validate_end(
        &self,
        transaction: TransactionId,
        end: &ContentCandidateEnd,
        context: ContentCandidateContext<'_>,
        authority: CandidateAuthority<'_>,
        resources: &ContentResourceStore,
        output: ContentOutputId,
    ) -> Result<CandidateValidation, ContentCandidateError> {
        let assembly = self.assemblies.get(&output).expect("selected assembly");
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.validate_stream_assembly(assembly, &context)?;
        if !transaction.is_valid()
            || context.output != output
            || assembly.begin.facts_generation != context.facts_generation
            || assembly.begin.interaction_generation != context.interaction_generation
        {
            return Err(ContentCandidateError::Stale);
        }
        if end.surface_count != assembly.begin.surface_count
            || end.placement_count != assembly.begin.placement_count
            || end.target_count != assembly.begin.target_count
            || assembly.surfaces.len() != end.surface_count as usize
            || assembly.placements.len() != end.placement_count as usize
            || assembly.targets.len() != end.target_count as usize
        {
            return Err(ContentCandidateError::Incomplete);
        }
        native::validate_binding(
            assembly.native_launcher,
            &assembly.begin,
            &assembly.surfaces,
            &assembly.targets,
            context.allocations,
            match authority {
                CandidateAuthority::Native(current) => Some(current),
                _ => None,
            },
        )?;
        persistent::validate_binding(
            assembly.persistent_catalog,
            &assembly.begin,
            &assembly.surfaces,
            &assembly.targets,
            authority,
        )?;
        validate_surfaces(&assembly.surfaces, context.allocations, output)?;
        validate_targets(&assembly.targets, &assembly.surfaces, context.allocations)?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        let frame_owner = self
            .frame_candidate_key(output, assembly.begin.candidate_generation)
            .map(|_| self.retire_owner(output, assembly.begin.candidate_generation));
        let lease = |resource| {
            #[cfg(feature = "experimental-native-frame-transaction")]
            if let Some(owner) = frame_owner {
                return resources.lease_frame_image(owner, resource);
            }
            resources.lease(self.limits.grant, resource)
        };
        let resource_ids = validate_placements(
            &assembly.placements,
            &assembly.surfaces,
            context.allocations,
            lease,
        )?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        if let Some(owner) = frame_owner {
            resources.validate_frame_images(owner, &resource_ids)?;
        }
        let mut leases = Vec::with_capacity(resource_ids.len());
        for resource in resource_ids {
            leases.push((resource, lease(resource)?));
        }
        Ok(CandidateValidation {
            resources: leases,
            deadline: self
                .last_now
                .checked_add(u64::from(self.limits.preparation_timeout_ms))
                .ok_or(ContentCandidateError::Malformed)?,
        })
    }

    fn take_validated_candidate(
        &mut self,
        output: ContentOutputId,
        validation: CandidateValidation,
    ) -> Candidate {
        let assembly = self.assemblies.remove(&output).expect("validated assembly");
        Candidate {
            native_launcher: assembly.native_launcher,
            persistent_catalog: assembly.persistent_catalog,
            transaction: assembly.transaction,
            begin: assembly.begin,
            surfaces: assembly.surfaces,
            placements: assembly.placements,
            targets: assembly.targets,
            resources: validation.resources,
            deadline: validation.deadline,
            prepared: false,
            #[cfg(feature = "experimental-native-frame-transaction")]
            retire: assembly.commit.and_then(|commit| commit.retire),
        }
    }

    fn reject_assembly(&mut self, output: ContentOutputId, error: ContentCandidateError) {
        #[cfg(feature = "experimental-native-frame-transaction")]
        if let Some(assembly) = self.assemblies.get_mut(&output)
            && let Some(copy) = assembly.copy.as_mut()
        {
            copy.stopped.get_or_insert(error);
            if let Some(commit) = &mut assembly.commit {
                commit.retire = None;
            }
            return;
        }
        if let Some(assembly) = self.assemblies.remove(&output) {
            self.response_credits -= 2;
            self.outcome(
                assembly.transaction,
                assembly.begin.candidate_generation,
                output,
                3,
                error.reason(),
                0,
                0,
                0,
            );
        }
    }

    /// Move one accepted candidate across the non-cancellable renderer seam.
    /// Calling this is the commitment; renderer failure must use
    /// `renderer_failed`, never put the candidate back into pending.
    pub fn begin_submission(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        now: u64,
    ) -> Result<ContentRenderBundle, ContentCandidateError> {
        if self.profile != ContentStoreProfile::Legacy {
            return Err(ContentCandidateError::Malformed);
        }
        self.begin_submission_inner(output, candidate_generation, now)
    }

    fn begin_submission_inner(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        now: u64,
    ) -> Result<ContentRenderBundle, ContentCandidateError> {
        self.time(now)?;
        self.expire(now)?;
        if self.submitted.contains_key(&output) {
            return Err(ContentCandidateError::Budget);
        }
        let candidate = self
            .pending
            .remove(&output)
            .ok_or(ContentCandidateError::Stale)?;
        if candidate.begin.candidate_generation != candidate_generation {
            self.pending.insert(output, candidate);
            return Err(ContentCandidateError::Stale);
        }
        let bundle = ContentRenderBundle {
            native_launcher: candidate.native_launcher,
            persistent_catalog: candidate.persistent_catalog,
            grant: self.limits.grant,
            output,
            candidate_generation,
            interaction_generation: candidate.begin.interaction_generation,
            surfaces: candidate.surfaces.clone(),
            placements: candidate.placements.clone(),
            targets: candidate.targets.clone(),
            resources: candidate.resources.clone(),
        };
        self.submitted.insert(output, candidate);
        Ok(bundle)
    }

    pub fn expire(&mut self, now: u64) -> Result<(), ContentCandidateError> {
        self.time(now)?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.expire_frame_streams(now)?;
        let expired_permits: Vec<_> = self
            .permits
            .iter()
            .filter_map(|(output, permit)| (now >= permit.deadline).then_some(*output))
            .collect();
        for output in expired_permits {
            let permit = self.permits.remove(&output).expect("selected permit");
            self.response_credits -= 2;
            self.push(
                permit.transaction,
                ShellContentRecord::FramePermit(ContentFramePermit {
                    grant: self.limits.grant,
                    output,
                    demand_id: permit.demand_id,
                    permit_id: permit.permit_id,
                    state: 2,
                    reason: ContentReason::Timeout as u16,
                    ttl_ms: 0,
                    max_candidate_bytes: 0,
                }),
            );
        }
        let expired_assemblies: Vec<_> = self
            .assemblies
            .iter()
            .filter_map(|(output, assembly)| (now >= assembly.deadline).then_some(*output))
            .collect();
        for output in expired_assemblies {
            self.reject_assembly(output, ContentCandidateError::Timeout);
        }
        let expired_pending: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(output, candidate)| (now >= candidate.deadline).then_some(*output))
            .collect();
        for output in expired_pending {
            let candidate = self.pending.remove(&output).expect("selected pending");
            self.response_credits -= 2;
            self.outcome(
                candidate.transaction,
                candidate.begin.candidate_generation,
                output,
                3,
                ContentReason::Timeout,
                0,
                0,
                0,
            );
        }
        Ok(())
    }

    /// Revocation kills authority and emits no records to a dead peer. Submitted
    /// leases stay owned until their non-cancellable caller retires them.
    pub fn revoke(&mut self) {
        self.revoked = true;
        self.demands.clear();
        self.permits.clear();
        self.assemblies.clear();
        self.pending.clear();
        self.events.clear();
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.presented_retire.clear();
        #[cfg(feature = "experimental-native-frame-transaction")]
        {
            self.streams = credits::CreditStreams::default();
        }
        // Submitted work remains non-cancellable, but a dead peer is owed no
        // wire record. Its renderer leases still drain through `presented` or
        // `renderer_failed` without reusing old authority.
        self.response_credits = 0;
    }
}
