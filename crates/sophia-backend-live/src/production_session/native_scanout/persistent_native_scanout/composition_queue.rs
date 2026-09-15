//! Queued native compositions own their real lowered frames until handoff.
//!
//! This queue is shared by native offer, service and topology teardown. It does
//! not own a DRM device or invent a presentation completion: the native adapter
//! supplies the existing mirror lifecycle's exact primary-ownership facts.

use super::*;

pub(crate) struct LiveProductionQueuedMirrorGeneration {
    pub(crate) output: OutputId,
    pub(crate) frame: LiveProductionNativeFrameId,
    pub(crate) logical_content_checksum: Option<u64>,
    pub(crate) heads: Vec<LiveProductionQueuedMirrorHeadFrame>,
}

pub(crate) struct LiveProductionQueuedMirrorHeadFrame {
    pub(crate) head_index: usize,
    pub(crate) identity: crate::LiveNativeFrameIdentity,
    pub(crate) content: LiveProductionScanoutContent,
    pub(crate) frame: crate::LiveOwnedMixedCompositionFrame,
    pub(crate) output_damage_snapshot: Option<sophia_engine::OutputFrameDamageSnapshot>,
    pub(crate) cpu_nonzero_pixel_bytes: usize,
}

impl LiveProductionQueuedMirrorGeneration {
    pub(crate) fn requires_retirement(&self) -> bool {
        self.heads
            .iter()
            .any(|head| head.content.requires_retirement())
    }

    pub(crate) fn source(&self) -> &'static str {
        match self.heads.first().map(|head| head.content) {
            Some(LiveProductionScanoutContent::Cpu { .. }) => "cpu",
            Some(LiveProductionScanoutContent::MixedPresent { .. }) => "mixed_present",
            Some(LiveProductionScanoutContent::RetainedMixed { .. }) => "retained_mixed",
            Some(LiveProductionScanoutContent::HeadComposition { .. }) => "head_composition",
            None => "empty",
        }
    }

    pub(crate) fn cpu_checksum(&self) -> Option<u64> {
        match self.heads.first().map(|head| head.content) {
            Some(LiveProductionScanoutContent::Cpu { checksum, .. }) => Some(checksum),
            _ => None,
        }
    }

    pub(crate) fn logical_checksum(&self) -> Option<u64> {
        self.logical_content_checksum
            .or_else(|| self.cpu_checksum())
    }
}

/// Actual lowered pixel owners awaiting renderer handoff or a mirror primary.
/// There is at most one newest deferred generation per configured output.
#[derive(Default)]
pub(crate) struct DeferredNativeCompositions {
    generations: BTreeMap<OutputId, LiveProductionQueuedMirrorGeneration>,
}

pub(crate) enum DeferredCompositionOffer {
    Refused(LiveProductionQueuedMirrorGeneration),
    Install(LiveProductionQueuedMirrorGeneration),
    Deferred {
        replaced: Option<LiveProductionNativeFrameId>,
    },
}

impl DeferredNativeCompositions {
    /// First-frame ownership is in this queue before exporters may be serviced.
    /// Expectations come from the installed topology and returned admission IDs.
    pub(crate) fn validate_first_frames(
        &self,
        expected: &BTreeMap<OutputId, Vec<(usize, crate::LiveNativeFrameIdentity)>>,
    ) -> Result<(), &'static str> {
        if expected.is_empty() {
            return Err("topology has no first-frame targets");
        }
        for (output, heads) in expected {
            let generation = self
                .generations
                .get(output)
                .ok_or("topology first frame is not queue-owned")?;
            if heads.is_empty() || heads.len() != generation.heads.len() {
                return Err("topology first-frame head coverage is incomplete");
            }
            for ((index, identity), owned) in heads.iter().zip(&generation.heads) {
                if identity.output() != *output
                    || identity.frame() != generation.frame.raw()
                    || *index != owned.head_index
                    || *identity != owned.identity
                    || owned.content.frame() != generation.frame
                {
                    return Err("topology first frame does not match installed targets");
                }
            }
        }
        Ok(())
    }

    /// Reserve the complete set of output cells before any renderer handoff.
    /// Refusal returns all offered owners and leaves existing cells unchanged.
    pub(crate) fn admit_batch(
        &mut self,
        generations: Vec<LiveProductionQueuedMirrorGeneration>,
        configured: &BTreeSet<OutputId>,
    ) -> Result<
        BTreeMap<OutputId, LiveProductionNativeFrameId>,
        (&'static str, Vec<LiveProductionQueuedMirrorGeneration>),
    > {
        let mut admitted = BTreeMap::new();
        if generations
            .iter()
            .any(|generation| self.protected(generation.output))
        {
            return Err(("composition queue owns a distinct retirement", generations));
        }
        for generation in &generations {
            if !configured.contains(&generation.output)
                || generation.heads.is_empty()
                || generation.heads.iter().any(|head| {
                    head.content.frame() != generation.frame
                        || head.identity.output() != generation.output
                        || head.identity.frame() != generation.frame.raw()
                })
            {
                return Err((
                    "composition batch has an invalid output or frame identity",
                    generations,
                ));
            }
            if admitted
                .insert(generation.output, generation.frame)
                .is_some()
            {
                return Err(("composition batch repeats an output", generations));
            }
            if self
                .generations
                .get(&generation.output)
                .is_some_and(|old| old.frame >= generation.frame)
            {
                return Err((
                    "composition batch does not advance its queued frame",
                    generations,
                ));
            }
        }
        if self
            .generations
            .keys()
            .any(|output| !configured.contains(output))
        {
            return Err(("composition queue still owns a removed output", generations));
        }
        let resulting = self
            .generations
            .keys()
            .chain(admitted.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        if resulting.len() > crate::LIVE_RENDERED_OUTPUT_CAPACITY {
            return Err(("composition queue output capacity exceeded", generations));
        }
        // All recoverable checks and construction precede mutation. This is
        // CPU ownership transfer only: no device callback or exporter runs.
        let mut staged = generations
            .into_iter()
            .map(|generation| (generation.output, generation))
            .collect();
        self.generations.append(&mut staged);
        Ok(admitted)
    }

    pub(crate) fn protected(&self, output: OutputId) -> bool {
        self.generations
            .get(&output)
            .is_some_and(|generation| generation.requires_retirement())
    }

    pub(crate) fn get(&self, output: OutputId) -> Option<&LiveProductionQueuedMirrorGeneration> {
        self.generations.get(&output)
    }

    pub(crate) fn pending(&self, output: OutputId) -> bool {
        self.generations.contains_key(&output)
    }

    pub(crate) fn offer(
        &mut self,
        generation: LiveProductionQueuedMirrorGeneration,
        active: Option<LiveProductionNativeFrameId>,
        primary_owned: Option<LiveProductionNativeFrameId>,
        active_content: Option<LiveProductionScanoutContent>,
    ) -> DeferredCompositionOffer {
        if self.protected(generation.output) {
            return DeferredCompositionOffer::Refused(generation);
        }
        if reduce_live_production_mirror_generation_queue(active, primary_owned, active_content)
            == LiveProductionMirrorGenerationQueue::DeferUntilPrimarySubmission
        {
            let replaced = self
                .generations
                .insert(generation.output, generation)
                .map(|old| old.frame);
            DeferredCompositionOffer::Deferred { replaced }
        } else {
            // A fresh offer can arrive after the primary becomes ready but
            // before deferred service runs. That newer installation supersedes
            // the old queued owner; service must never reinstall it afterwards.
            self.generations.remove(&generation.output);
            DeferredCompositionOffer::Install(generation)
        }
    }

    pub(crate) fn take_ready(
        &mut self,
        output: OutputId,
        active: Option<LiveProductionNativeFrameId>,
        primary_owned: Option<LiveProductionNativeFrameId>,
        active_content: Option<LiveProductionScanoutContent>,
    ) -> Option<LiveProductionQueuedMirrorGeneration> {
        if reduce_live_production_mirror_generation_queue(active, primary_owned, active_content)
            == LiveProductionMirrorGenerationQueue::DeferUntilPrimarySubmission
        {
            return None;
        }
        self.generations.remove(&output)
    }

    /// The failed installer returns ownership here. A retry must not forget
    /// the refused generation while propagating its error to the owner loop.
    /// A newer retained generation must never be replaced by an older retry.
    pub(crate) fn retain_refused(
        &mut self,
        generation: LiveProductionQueuedMirrorGeneration,
    ) -> Result<(), LiveProductionQueuedMirrorGeneration> {
        if self.protected(generation.output) {
            return Err(generation);
        }
        match self.generations.entry(generation.output) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(generation);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().frame < generation.frame {
                    entry.insert(generation);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn owns(&self, output: OutputId, frame: LiveProductionNativeFrameId) -> bool {
        self.generations
            .get(&output)
            .is_some_and(|generation| generation.frame == frame)
    }

    pub(crate) fn revoke_output(&mut self, output: OutputId) {
        self.generations.remove(&output);
    }

    pub(crate) fn clear(&mut self) {
        self.generations.clear();
    }
}

#[cfg(test)]
#[path = "../../../../tests/support/native_composition_queue.rs"]
mod tests;
