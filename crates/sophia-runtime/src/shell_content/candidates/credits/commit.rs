//! Complete scene intake over the existing stream, candidate and copy owners.
use super::*;

/// Internal SFT input, not an allocated wire record or negotiated opener.
#[derive(Debug)]
pub struct ContentFrameCommit {
    pub transaction: TransactionId,
    pub scene: ContentCandidate,
    pub image: Option<crate::OwnedCopyImageRequest>,
    pub retire: Option<ContentRetireOnPresent>,
}

/// Whether complete input remains queued, was accepted, or received a reserved refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentFrameAdmission {
    Deferred,
    Accepted { copy_job: Option<u64> },
    Refused(ContentCandidateError),
}

#[derive(Default)]
pub(crate) struct FrameAdmissionReservation {
    pub copy_job: Option<u64>,
    pub commit: Option<FrameCommitState>,
}

pub(crate) struct FrameCommitState {
    pub retire: Option<(ContentRetireOnPresent, ContentRetireToken)>,
}

impl ContentFrameCommit {
    pub(crate) fn begin(&self) -> ContentCandidateBegin {
        ContentCandidateBegin {
            grant: self.scene.grant,
            candidate_generation: self.scene.candidate_generation,
            output: self.scene.output,
            facts_generation: self.scene.facts_generation,
            pacing_permit: self.scene.pacing_permit,
            interaction_generation: self.scene.interaction_generation,
            surface_count: self.scene.surfaces.len().try_into().unwrap_or(u32::MAX),
            placement_count: self.scene.placements.len().try_into().unwrap_or(u32::MAX),
            target_count: self.scene.targets.len().try_into().unwrap_or(u32::MAX),
        }
    }
}

impl ContentCandidateStore {
    pub(crate) fn validate_frame_scene(
        &self,
        commit: &ContentFrameCommit,
    ) -> Result<(), ContentCandidateError> {
        let scene = &commit.scene;
        let bytes = 40usize
            .saturating_add(scene.surfaces.len().saturating_mul(64))
            .saturating_add(scene.placements.len().saturating_mul(32))
            .saturating_add(scene.targets.len().saturating_mul(48));
        if scene.surfaces.len() != 1
            || scene.placements.len() > self.limits.max_candidate_placements as usize
            || scene.targets.len() > self.limits.max_candidate_targets as usize
            || bytes > self.limits.max_candidate_bytes as usize
            || !valid_rows(
                &scene.surfaces,
                &scene.placements,
                &scene.targets,
                self.limits.max_margin_logical,
                self.profile,
            )
            || commit.image.is_some_and(|image| {
                !scene
                    .placements
                    .iter()
                    .any(|p| p.resource == image.resource)
            })
        {
            return Err(ContentCandidateError::Malformed);
        }
        if commit.retire.is_some_and(|retire| {
            retire.retire_transaction == commit.transaction
                || scene
                    .placements
                    .iter()
                    .any(|p| p.resource == retire.resource)
        }) {
            return Err(ContentCandidateError::Stale);
        }
        Ok(())
    }

    pub(crate) fn install_frame_scene(&mut self, scene: ContentCandidate) {
        let assembly = self
            .assemblies
            .get_mut(&scene.output)
            .expect("admitted complete scene");
        assert!(assembly.commit.is_some(), "complete input reservation");
        assembly.surfaces = scene.surfaces;
        assembly.placements = scene.placements;
        assembly.targets = scene.targets;
    }

    /// A complete scene ready for fresh validation, not a renderer submission.
    pub fn ready_frame_commit(&self) -> Option<(ContentOutputId, u64)> {
        self.assemblies.iter().find_map(|(output, assembly)| {
            (assembly.commit.is_some() && assembly.copy.is_none())
                .then_some((*output, assembly.begin.candidate_generation))
        })
    }

    /// Validate complete intent against the owner's current facts and allocations.
    /// Copy settlement only removes the read gate; it never supplies this context.
    ///
    /// # Errors
    /// Returns Budget while copying, Stale for a missing/non-complete input,
    /// or the ordinary candidate validation error. A rejected scene retains
    /// its reserved terminal and private-image cleanup obligations.
    pub fn finish_frame_commit(
        &mut self,
        output: ContentOutputId,
        generation: u64,
        context: ContentCandidateContext<'_>,
        resources: &ContentResourceStore,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        let assembly = self
            .assemblies
            .get(&output)
            .ok_or(ContentCandidateError::Stale)?;
        if assembly.commit.is_none() || assembly.begin.candidate_generation != generation {
            return Err(ContentCandidateError::Stale);
        }
        let end = ContentCandidateEnd {
            grant: assembly.begin.grant,
            candidate_generation: generation,
            surface_count: assembly.begin.surface_count,
            placement_count: assembly.begin.placement_count,
            target_count: assembly.begin.target_count,
        };
        self.end_inner(
            assembly.transaction,
            end,
            context,
            CandidateAuthority::Legacy,
            resources,
            now,
        )
    }
}
