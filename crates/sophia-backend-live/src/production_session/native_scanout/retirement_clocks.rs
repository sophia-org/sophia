//! Immutable real clock evidence accompanying an already authorized retirement.
//! Empty and single-head paths allocate nothing; mirrored evidence is shared.
use super::{LiveNativePresentClockSample, LiveNativePresentClockSource};

/// A historical event is exact completion evidence, never clock progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveNativeRetirementClock {
    pub sample: LiveNativePresentClockSample,
    pub historical: bool,
}

impl From<LiveNativePresentClockSample> for LiveNativeRetirementClock {
    fn from(sample: LiveNativePresentClockSample) -> Self {
        Self {
            sample,
            historical: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveNativeRetirementClocks(Storage);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum Storage {
    #[default]
    Empty,
    One(LiveNativeRetirementClock),
    Many(std::sync::Arc<[LiveNativeRetirementClock]>),
}

impl LiveNativeRetirementClocks {
    pub fn from_samples(samples: impl IntoIterator<Item = LiveNativePresentClockSample>) -> Self {
        Self::from_evidence(samples.into_iter().map(Into::into))
    }

    pub fn from_evidence(samples: impl IntoIterator<Item = LiveNativeRetirementClock>) -> Self {
        let mut samples = samples.into_iter();
        let Some(first) = samples.next() else {
            return Self::default();
        };
        let Some(second) = samples.next() else {
            return Self(Storage::One(first));
        };
        let mut collected = vec![first, second];
        for sample in samples {
            assert!(
                collected.len()
                    < sophia_engine::MAX_DRM_KMS_OUTPUTS * sophia_engine::MAX_HEADS_PER_OUTPUT,
                "retirement clock evidence exceeded physical head bound"
            );
            collected.push(sample);
        }
        Self(Storage::Many(collected.into()))
    }

    pub fn evidence(&self) -> &[LiveNativeRetirementClock] {
        match &self.0 {
            Storage::Empty => &[],
            Storage::One(sample) => std::slice::from_ref(sample),
            Storage::Many(samples) => samples,
        }
    }

    pub fn sample(
        &self,
        source: LiveNativePresentClockSource,
    ) -> Option<LiveNativePresentClockSample> {
        self.evidence()
            .iter()
            .find(|evidence| evidence.sample.source == source)
            .map(|evidence| evidence.sample)
    }
}
