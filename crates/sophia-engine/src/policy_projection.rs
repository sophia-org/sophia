use std::collections::{BTreeMap, BTreeSet};

use sophia_protocol::{
    OutputId, POLICY_INDICATOR_STATE_MASK, POLICY_MAX_INDICATORS, POLICY_MAX_INDICATORS_PER_OUTPUT,
    POLICY_MAX_OUTPUT_STATUSES, POLICY_MAX_SURFACES, POLICY_OUTPUT_STATUS_FOCUS_MASK,
    PolicyOutputProjection, PolicyProjectionIndicator, PolicyProjectionOutcome,
    PolicyProjectionOutputStatus, PolicyProjectionProposal, PolicyProjectionRequest,
    PolicyRequestCause, PolicySceneSnapshot, Rect, SurfaceId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyProjectionError {
    InvalidSceneGeneration,
    InvalidOutput,
    DuplicateOutput,
    InvalidOutputGeometry,
    ExcessiveOutputs,
    InvalidSurface,
    DuplicateSurface,
    /// A scene surface arrived with nothing to place.
    EmptySceneSurfaceGeometry {
        surface: SurfaceId,
        geometry: Rect,
    },
    /// A placement does not fit the output it was placed on.
    ///
    /// It carries all three rectangles because the interesting case is a
    /// projection that was correct when the policy wrote it: an output that
    /// shrinks under it -- a mirror group re-optimized onto its smaller head,
    /// say -- makes yesterday's placement too large today, and the numbers are
    /// the only way to tell that from a policy proposing nonsense.
    InvalidSurfaceGeometry {
        surface: SurfaceId,
        geometry: Rect,
        work_area: Rect,
        bounds: Rect,
        fullscreen: bool,
    },
    InvalidSurfaceConstraints,
    InvalidPresentationState,
    InvalidTransientOwner,
    InvalidFocus,
    ExcessiveSurfaces,
    ConnectionAlreadyActive,
    InvalidConnectionEpoch,
    NoActiveConnection,
    RequestAlreadyPending,
    NoAffectedOutputs,
    UnknownAffectedOutput,
    InvalidRequestCause,
    RequestIdExhausted,
    LegacyAdapterState,
    InvalidIndicator,
    DuplicateIndicator,
    DuplicateIndicatorSlot,
    ExcessiveIndicators,
    InvalidOutputStatus,
    DuplicateOutputStatus,
}

impl core::fmt::Display for PolicyProjectionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PolicyProjectionError {}

/// Engine-owned canonical projection state.
///
/// Validation constructs a complete candidate map before replacing committed
/// state. Rejection, timeout, and policy loss therefore cannot expose a partial
/// multi-output update.
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyProjectionReducer {
    scene: PolicySceneSnapshot,
    committed: BTreeMap<OutputId, PolicyOutputProjection>,
    active_epoch: Option<u64>,
    greatest_epoch: u64,
    next_request_id: u64,
    policy_generation: u64,
    outstanding: Option<PolicyProjectionRequest>,
    commit_serial: u64,
    indicators: BTreeMap<OutputId, Vec<PolicyProjectionIndicator>>,
    output_statuses: BTreeMap<OutputId, PolicyProjectionOutputStatus>,
    indicator_publication_generation: u64,
    tab_groups: Vec<sophia_protocol::PolicyTabGroup>,
    presentation: Option<sophia_protocol::PolicyPresentation>,
    greatest_presentation_generation: u64,
    greatest_presentation_target: u64,
}

/// What the indicator chrome renders, and the identity consumers compare it by.
///
/// It deliberately carries no commit serial. Consumers compare the whole record
/// to decide whether anything changed, and a serial that advances on every policy
/// commit made every commit look like new indicator content: a guaranteed raster
/// cache miss, full strip damage, and a cancelled in-flight indicator click. The
/// generation advances when this content changes, which is the only thing a
/// consumer needs to know.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyIndicatorPublication {
    pub tab_groups: Vec<sophia_protocol::PolicyTabGroup>,
    pub generation: u64,
    pub connection_epoch: Option<u64>,
    pub indicators: Vec<PolicyProjectionIndicator>,
    pub output_statuses: Vec<PolicyProjectionOutputStatus>,
}

/// A fully validated reducer successor held outside authoritative state until
/// frontend configuration and renderable content have settled.
#[derive(Clone, Debug, PartialEq)]
pub struct StagedPolicyProjection {
    candidate: PolicyProjectionReducer,
    connection_epoch: u64,
    request_id: u64,
    scene_generation: u64,
    commit_serial: u64,
}

impl StagedPolicyProjection {
    /// Immutable publication for session admission before ordinary layout or
    /// reducer state becomes authoritative.
    pub fn presentation_publication(&self) -> Option<(u64, &sophia_protocol::PolicyPresentation)> {
        self.candidate.presentation_publication()
    }

    pub fn projections(&self) -> Vec<PolicyOutputProjection> {
        self.candidate.committed()
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    pub const fn scene_generation(&self) -> u64 {
        self.scene_generation
    }
}

impl PolicyProjectionReducer {
    pub fn new(scene: PolicySceneSnapshot) -> Result<Self, PolicyProjectionError> {
        validate_scene(&scene)?;
        let committed = committed_from_scene(&scene);
        Ok(Self {
            scene,
            committed,
            active_epoch: None,
            greatest_epoch: 0,
            next_request_id: 1,
            policy_generation: 1,
            outstanding: None,
            commit_serial: 0,
            indicators: BTreeMap::new(),
            tab_groups: Vec::new(),
            presentation: None,
            greatest_presentation_generation: 0,
            greatest_presentation_target: 0,
            output_statuses: BTreeMap::new(),
            indicator_publication_generation: 0,
        })
    }

    pub fn connect(&mut self, connection_epoch: u64) -> Result<(), PolicyProjectionError> {
        if self.active_epoch.is_some() {
            return Err(PolicyProjectionError::ConnectionAlreadyActive);
        }
        if connection_epoch == 0 || connection_epoch <= self.greatest_epoch {
            return Err(PolicyProjectionError::InvalidConnectionEpoch);
        }
        self.active_epoch = Some(connection_epoch);
        self.greatest_epoch = connection_epoch;
        self.clear_indicator_publication();
        self.presentation = None;
        self.greatest_presentation_generation = 0;
        self.greatest_presentation_target = 0;
        Ok(())
    }

    pub fn disconnect(&mut self, connection_epoch: u64) -> PolicyProjectionOutcome {
        if self.active_epoch != Some(connection_epoch) {
            return PolicyProjectionOutcome::Disconnected;
        }
        self.active_epoch = None;
        self.outstanding = None;
        self.clear_indicator_publication();
        self.presentation = None;
        PolicyProjectionOutcome::Disconnected
    }

    pub fn issue_request(
        &mut self,
        affected_outputs: Vec<OutputId>,
    ) -> Result<PolicyProjectionRequest, PolicyProjectionError> {
        self.issue_request_with_cause(affected_outputs, PolicyRequestCause::SceneChanged)
    }

    /// Issues a request without collapsing its initiating event. Equal action
    /// tokens with distinct activation serials remain separate ordered work.
    pub fn issue_request_with_cause(
        &mut self,
        affected_outputs: Vec<OutputId>,
        cause: PolicyRequestCause,
    ) -> Result<PolicyProjectionRequest, PolicyProjectionError> {
        let Some(connection_epoch) = self.active_epoch else {
            return Err(PolicyProjectionError::NoActiveConnection);
        };
        if self.outstanding.is_some() {
            return Err(PolicyProjectionError::RequestAlreadyPending);
        }
        if affected_outputs.is_empty() {
            return Err(PolicyProjectionError::NoAffectedOutputs);
        }
        let live_outputs = self
            .scene
            .outputs
            .iter()
            .map(|output| output.output)
            .collect::<BTreeSet<_>>();
        let unique = affected_outputs.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != affected_outputs.len() {
            return Err(PolicyProjectionError::DuplicateOutput);
        }
        if !unique.is_subset(&live_outputs) {
            return Err(PolicyProjectionError::UnknownAffectedOutput);
        }
        validate_request_cause(cause, &self.scene.surfaces)?;
        if let PolicyRequestCause::PointerFocus { output, .. } = cause
            && !unique.contains(&output)
        {
            return Err(PolicyProjectionError::InvalidRequestCause);
        }
        if let PolicyRequestCause::OutputAction {
            output,
            output_generation,
            ..
        } = cause
            && (!unique.contains(&output)
                || !self
                    .scene
                    .outputs
                    .iter()
                    .any(|o| o.output == output && o.generation == output_generation))
        {
            return Err(PolicyProjectionError::InvalidRequestCause);
        }
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(PolicyProjectionError::RequestIdExhausted)?;
        if !self.presentation_cause_is_current(cause)
            || matches!(cause, PolicyRequestCause::PresentationAction { identity, .. }
                if !affected_outputs.contains(&identity.output))
        {
            return Err(PolicyProjectionError::InvalidRequestCause);
        }
        let request = PolicyProjectionRequest {
            connection_epoch,
            request_id,
            scene_generation: self.scene.generation,
            policy_generation: self.policy_generation,
            affected_outputs,
            cause,
        };
        self.outstanding = Some(request.clone());
        Ok(request)
    }

    pub fn apply_proposal(
        &mut self,
        proposal: &PolicyProjectionProposal,
    ) -> PolicyProjectionOutcome {
        let Some(request) = self.outstanding.take() else {
            return if self.active_epoch == Some(proposal.connection_epoch) {
                PolicyProjectionOutcome::RejectedStale
            } else {
                PolicyProjectionOutcome::Disconnected
            };
        };
        if self.active_epoch != Some(proposal.connection_epoch)
            || proposal.connection_epoch != request.connection_epoch
        {
            return PolicyProjectionOutcome::Disconnected;
        }
        if proposal.request_id != request.request_id
            || proposal.base_generation != request.scene_generation
            || request.scene_generation != self.scene.generation
            || !self.presentation_cause_is_current(request.cause)
        {
            return PolicyProjectionOutcome::RejectedStale;
        }
        if !proposal.transaction.is_valid() || !crate::policy_translation_groups_valid(proposal) {
            return PolicyProjectionOutcome::RejectedInvalid;
        }
        let Ok(candidate) = self.validated_candidate(&request, proposal) else {
            return PolicyProjectionOutcome::RejectedInvalid;
        };
        let Ok((indicators, output_statuses)) = self.validated_descriptors(&request, proposal)
        else {
            return PolicyProjectionOutcome::RejectedInvalid;
        };
        let Ok(tab_groups) =
            crate::validate_policy_tab_groups(&self.scene, &candidate, &self.tab_groups, proposal)
        else {
            return PolicyProjectionOutcome::RejectedInvalid;
        };
        let tabs_changed = self.tab_groups != tab_groups;
        if self
            .validate_presentation(proposal.presentation.as_ref())
            .is_err()
        {
            return PolicyProjectionOutcome::RejectedInvalid;
        }
        if self.presentation != proposal.presentation
            && self
                .presentation
                .iter()
                .chain(proposal.presentation.iter())
                .flat_map(|p| &p.outputs)
                .any(|o| !request.affected_outputs.contains(&o.output))
        {
            return PolicyProjectionOutcome::RejectedInvalid;
        }
        self.commit_presentation(proposal.presentation.clone());
        self.tab_groups = tab_groups;
        // The commit serial advances unconditionally: it guards settlement
        // staleness and must count every commit. The publication generation
        // advances only when the published content moved, so a commit that
        // changes layout without touching the indicators leaves the chrome, its
        // raster cache, and any in-flight capture undisturbed.
        let publication_changed = tabs_changed
            || self.indicators != indicators
            || self.output_statuses != output_statuses;
        self.committed = candidate;
        self.scene.active_output = proposal.active_output;
        self.indicators = indicators;
        self.output_statuses = output_statuses;
        sync_scene_projection(&mut self.scene, &self.committed);
        self.commit_serial = self.commit_serial.saturating_add(1);
        if publication_changed {
            self.indicator_publication_generation =
                self.indicator_publication_generation.saturating_add(1);
        }
        PolicyProjectionOutcome::Committed
    }

    /// Validates a proposal against a clone, preserving the authoritative
    /// reducer and its outstanding request until frontend settlement.
    pub fn stage_proposal(
        &self,
        proposal: &PolicyProjectionProposal,
    ) -> Result<StagedPolicyProjection, PolicyProjectionOutcome> {
        let mut candidate = self.clone();
        let outcome = candidate.apply_proposal(proposal);
        if outcome != PolicyProjectionOutcome::Committed {
            return Err(outcome);
        }
        Ok(StagedPolicyProjection {
            candidate,
            connection_epoch: proposal.connection_epoch,
            request_id: proposal.request_id,
            scene_generation: proposal.base_generation,
            commit_serial: self.commit_serial,
        })
    }

    /// Revalidates a staged successor without changing authoritative reducer
    /// state. The owner uses this immediately before atomically settling the
    /// corresponding frontend layout and reducer successor.
    pub fn revalidate_staged(&self, staged: &StagedPolicyProjection) -> PolicyProjectionOutcome {
        if self.active_epoch != Some(staged.connection_epoch) {
            return PolicyProjectionOutcome::Disconnected;
        }
        let exact_request = self
            .outstanding
            .as_ref()
            .is_some_and(|request| request.request_id == staged.request_id);
        if !exact_request
            || self.scene.generation != staged.scene_generation
            || self.commit_serial != staged.commit_serial
        {
            return PolicyProjectionOutcome::RejectedStale;
        }
        PolicyProjectionOutcome::Committed
    }

    /// Promotes a staged successor only if no connection, scene, request, or
    /// earlier frontend settlement has superseded its validation base.
    pub fn commit_staged(&mut self, staged: StagedPolicyProjection) -> PolicyProjectionOutcome {
        if self.active_epoch != Some(staged.connection_epoch) {
            return PolicyProjectionOutcome::Disconnected;
        }
        let exact_request = self
            .outstanding
            .as_ref()
            .is_some_and(|request| request.request_id == staged.request_id);
        if !exact_request {
            return PolicyProjectionOutcome::RejectedStale;
        }
        if self.scene.generation != staged.scene_generation
            || self.commit_serial != staged.commit_serial
        {
            self.outstanding = None;
            return PolicyProjectionOutcome::RejectedStale;
        }
        *self = staged.candidate;
        PolicyProjectionOutcome::Committed
    }

    pub fn timeout(&mut self, request_id: u64) -> PolicyProjectionOutcome {
        if self
            .outstanding
            .as_ref()
            .is_some_and(|request| request.request_id == request_id)
        {
            self.outstanding = None;
            PolicyProjectionOutcome::TimedOut
        } else {
            PolicyProjectionOutcome::RejectedStale
        }
    }

    pub fn observe_scene(
        &mut self,
        scene: PolicySceneSnapshot,
    ) -> Result<(), PolicyProjectionError> {
        validate_scene(&scene)?;
        if scene.generation <= self.scene.generation {
            return Err(PolicyProjectionError::InvalidSceneGeneration);
        }
        let surfaces = scene
            .surfaces
            .iter()
            .map(|surface| (surface.surface, surface))
            .collect::<BTreeMap<_, _>>();
        let output_ids = scene
            .outputs
            .iter()
            .map(|output| output.output)
            .collect::<BTreeSet<_>>();
        let mut committed = BTreeMap::new();
        for output in &scene.outputs {
            let mut projection =
                self.committed
                    .get(&output.output)
                    .cloned()
                    .unwrap_or(PolicyOutputProjection {
                        output: output.output,
                        placements: Vec::new(),
                        focus: output.focus,
                    });
            projection.placements.retain_mut(|placement| {
                let Some(surface) = surfaces.get(&placement.surface) else {
                    return false;
                };
                placement.surface_generation = surface.generation;
                true
            });
            let visible = projection
                .placements
                .iter()
                .map(|placement| placement.surface)
                .collect::<BTreeSet<_>>();
            if projection.focus.is_some_and(|focus| {
                !visible.contains(&focus)
                    || !surfaces
                        .get(&focus)
                        .is_some_and(|surface| surface.capabilities.focusable)
            }) {
                projection.focus = None;
            }
            committed.insert(output.output, projection);
        }
        let mut placed = committed
            .values()
            .flat_map(|projection| projection.placements.iter())
            .map(|placement| placement.surface)
            .collect::<BTreeSet<_>>();
        for surface in &scene.surfaces {
            let Some(output) = surface.current_output else {
                continue;
            };
            if placed.insert(surface.surface) {
                committed
                    .get_mut(&output)
                    .expect("validated current output exists")
                    .placements
                    .push(placement_from_snapshot(surface));
            }
        }
        debug_assert_eq!(
            committed.keys().copied().collect::<BTreeSet<_>>(),
            output_ids
        );
        let tabs_before = self.tab_groups.len();
        self.tab_groups.retain(|g| {
            output_ids.contains(&g.output) && g.members.iter().all(|s| surfaces.contains_key(s))
        });
        self.revalidate_presentation_scene(&scene);
        self.scene = scene;
        self.committed = committed;
        let before = (self.indicators.len(), self.output_statuses.len());
        self.indicators
            .retain(|output, _| output_ids.contains(output));
        self.output_statuses
            .retain(|output, _| output_ids.contains(output));
        if tabs_before != self.tab_groups.len()
            || before != (self.indicators.len(), self.output_statuses.len())
        {
            self.indicator_publication_generation =
                self.indicator_publication_generation.saturating_add(1);
        }
        sync_scene_projection(&mut self.scene, &self.committed);
        Ok(())
    }

    pub const fn scene(&self) -> &PolicySceneSnapshot {
        &self.scene
    }

    pub fn committed(&self) -> Vec<PolicyOutputProjection> {
        self.committed.values().cloned().collect()
    }

    pub const fn outstanding(&self) -> Option<&PolicyProjectionRequest> {
        self.outstanding.as_ref()
    }

    pub const fn commit_serial(&self) -> u64 {
        self.commit_serial
    }

    /// Advances the latest policy-private generation acknowledged by future
    /// complete projection requests.
    pub fn admit_policy_generation(
        &mut self,
        generation: u64,
    ) -> Result<(), PolicyProjectionError> {
        if generation <= self.policy_generation {
            return Err(PolicyProjectionError::InvalidSceneGeneration);
        }
        self.policy_generation = generation;
        Ok(())
    }

    pub fn indicator_publication(&self) -> PolicyIndicatorPublication {
        PolicyIndicatorPublication {
            tab_groups: self.tab_groups.clone(),
            generation: self.indicator_publication_generation,
            connection_epoch: self.active_epoch,
            indicators: self.indicators.values().flatten().cloned().collect(),
            output_statuses: self.output_statuses.values().cloned().collect(),
        }
    }

    fn clear_indicator_publication(&mut self) {
        self.tab_groups.clear();
        self.indicators.clear();
        self.output_statuses.clear();
        self.indicator_publication_generation =
            self.indicator_publication_generation.saturating_add(1);
    }

    fn validated_candidate(
        &self,
        request: &PolicyProjectionRequest,
        proposal: &PolicyProjectionProposal,
    ) -> Result<BTreeMap<OutputId, PolicyOutputProjection>, PolicyProjectionError> {
        let affected = request
            .affected_outputs
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let proposed = proposal
            .outputs
            .iter()
            .map(|output| output.output)
            .collect::<BTreeSet<_>>();
        if proposed.len() != proposal.outputs.len() || proposed != affected {
            return Err(PolicyProjectionError::DuplicateOutput);
        }
        let live_output_ids = self
            .scene
            .outputs
            .iter()
            .map(|output| output.output)
            .collect::<BTreeSet<_>>();
        if !proposal.active_output.is_valid()
            || !live_output_ids.contains(&proposal.active_output)
            || (proposal.active_output != self.scene.active_output
                && (!affected.contains(&self.scene.active_output)
                    || !affected.contains(&proposal.active_output)))
        {
            return Err(PolicyProjectionError::InvalidOutput);
        }
        let live_outputs = self
            .scene
            .outputs
            .iter()
            .map(|output| (output.output, (output.bounds, output.work_area)))
            .collect::<BTreeMap<_, _>>();
        let live_surfaces = self
            .scene
            .surfaces
            .iter()
            .map(|surface| (surface.surface, surface))
            .collect::<BTreeMap<_, _>>();
        let mut candidate = self.committed.clone();
        for output in &proposal.outputs {
            let (bounds, work_area) = live_outputs
                .get(&output.output)
                .copied()
                .ok_or(PolicyProjectionError::InvalidOutput)?;
            validate_output_projection(output, bounds, work_area, &live_surfaces)?;
            candidate.insert(output.output, output.clone());
        }
        let mut surfaces = BTreeSet::new();
        let mut count = 0usize;
        for output in candidate.values() {
            count = count
                .checked_add(output.placements.len())
                .ok_or(PolicyProjectionError::ExcessiveSurfaces)?;
            if count > POLICY_MAX_SURFACES {
                return Err(PolicyProjectionError::ExcessiveSurfaces);
            }
            for placement in &output.placements {
                if !surfaces.insert(placement.surface) {
                    return Err(PolicyProjectionError::DuplicateSurface);
                }
            }
        }
        Ok(candidate)
    }

    fn validated_descriptors(
        &self,
        request: &PolicyProjectionRequest,
        proposal: &PolicyProjectionProposal,
    ) -> Result<IndicatorsAndStatusesByOutput, PolicyProjectionError> {
        if proposal.indicators.len() > POLICY_MAX_INDICATORS
            || proposal.output_statuses.len() > POLICY_MAX_OUTPUT_STATUSES
        {
            return Err(PolicyProjectionError::ExcessiveIndicators);
        }
        let affected = request
            .affected_outputs
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut indicators = self.indicators.clone();
        let mut statuses = self.output_statuses.clone();
        for output in &affected {
            indicators.remove(output);
            statuses.remove(output);
        }

        let mut identities = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for indicator in &proposal.indicators {
            if !affected.contains(&indicator.output)
                || indicator.indicator == 0
                || indicator.state_bits & !POLICY_INDICATOR_STATE_MASK != 0
                || indicator.label.is_empty()
                || indicator.label.len() > 32
                || indicator.label.chars().any(char::is_control)
                || indicator.action.is_some_and(|action| !action.is_valid())
            {
                return Err(PolicyProjectionError::InvalidIndicator);
            }
            if !identities.insert((indicator.output, indicator.indicator)) {
                return Err(PolicyProjectionError::DuplicateIndicator);
            }
            if !slots.insert((indicator.output, indicator.slot)) {
                return Err(PolicyProjectionError::DuplicateIndicatorSlot);
            }
            let output_indicators = indicators.entry(indicator.output).or_default();
            if output_indicators.len() == POLICY_MAX_INDICATORS_PER_OUTPUT {
                return Err(PolicyProjectionError::ExcessiveIndicators);
            }
            output_indicators.push(indicator.clone());
        }
        for output_indicators in indicators.values_mut() {
            output_indicators.sort_by_key(|indicator| indicator.slot);
        }

        for status in &proposal.output_statuses {
            if !affected.contains(&status.output)
                || status.focus_bits & !POLICY_OUTPUT_STATUS_FOCUS_MASK != 0
                || status.layout.is_empty()
                || status.layout.len() > 32
                || status.layout.chars().any(char::is_control)
            {
                return Err(PolicyProjectionError::InvalidOutputStatus);
            }
            if statuses.insert(status.output, status.clone()).is_some() {
                return Err(PolicyProjectionError::DuplicateOutputStatus);
            }
        }
        Ok((indicators, statuses))
    }
}

mod presentation;
mod validation;

use validation::*;

/// Indicators and output status, each grouped by the output they describe.
type IndicatorsAndStatusesByOutput = (
    BTreeMap<OutputId, Vec<PolicyProjectionIndicator>>,
    BTreeMap<OutputId, PolicyProjectionOutputStatus>,
);
