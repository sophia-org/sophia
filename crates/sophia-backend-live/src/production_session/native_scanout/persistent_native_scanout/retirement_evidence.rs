//! Evidence follows the existing generation owner; it grants no permission.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeOutputCohort {
    pub presentation: sophia_engine::OutputPresentationCohort,
    clocks: BTreeMap<sophia_engine::RenderHeadId, crate::LiveNativeRetirementClock>,
}

impl From<sophia_engine::OutputPresentationCohort> for NativeOutputCohort {
    fn from(presentation: sophia_engine::OutputPresentationCohort) -> Self {
        Self {
            presentation,
            clocks: BTreeMap::new(),
        }
    }
}

impl NativeOutputCohort {
    /// Called only after the exact head/frame physical completion was accepted.
    pub fn record_clock(
        &mut self,
        head: sophia_engine::RenderHeadId,
        sample: Option<crate::LiveNativeRetirementClock>,
    ) {
        assert!(
            self.presentation
                .required_heads()
                .any(|member| member == head)
        );
        if let Some(sample) = sample {
            // One immutable observation per member, not its latest global clock.
            let previous = self.clocks.entry(head).or_insert(sample);
            assert_eq!(
                *previous, sample,
                "cohort head changed its retirement clock"
            );
        }
        debug_assert!(self.clocks.len() <= sophia_engine::MAX_HEADS_PER_OUTPUT);
    }

    pub fn clocks(&self) -> crate::LiveNativeRetirementClocks {
        crate::LiveNativeRetirementClocks::from_evidence(self.clocks.values().copied())
    }
}

// Preserve the existing Engine reducer as the only permission authority.
impl std::ops::Deref for NativeOutputCohort {
    type Target = sophia_engine::OutputPresentationCohort;
    fn deref(&self) -> &Self::Target {
        &self.presentation
    }
}
impl std::ops::DerefMut for NativeOutputCohort {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.presentation
    }
}
