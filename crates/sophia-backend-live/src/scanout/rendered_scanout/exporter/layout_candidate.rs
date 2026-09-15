use super::correlation::LiveRendererFrameCorrelation;
use sophia_engine::DirectScanoutVerdict;
use sophia_renderer_live::{LiveDirectScanoutBuffer, LiveRendererScanoutBufferDescriptor};
use std::time::Instant;

#[derive(Default)]
pub(super) struct RetainedLayoutCandidate {
    candidate: Option<PendingLayoutSource>,
}

pub(super) struct RetainedLayoutSource {
    pub buffer: LiveDirectScanoutBuffer,
    pub original: LiveRendererFrameCorrelation,
}

struct PendingLayoutSource {
    source: RetainedLayoutSource,
    deadline: Instant,
    fallback: Option<LiveRendererFrameCorrelation>,
}

impl RetainedLayoutCandidate {
    /// Capture a structurally checked source; every new offer or owner-context change invalidates it.
    pub fn capture(
        &mut self,
        source: LiveDirectScanoutBuffer,
        original: LiveRendererFrameCorrelation,
        now: Instant,
        deadline: Instant,
    ) -> bool {
        self.invalidate();
        if now >= deadline
            || original.request.is_some()
            || original.trace.is_none()
            || original.direct_scanout != Some(DirectScanoutVerdict::Eligible)
            || !source.descriptor.is_valid_scanout_buffer()
        {
            return false;
        }
        self.candidate = Some(PendingLayoutSource {
            source: RetainedLayoutSource {
                buffer: source,
                original,
            },
            deadline,
            fallback: None,
        });
        true
    }

    /// Preserve the source format in the ordinary fallback render, before assigning its request.
    pub fn output_format(
        &mut self,
        fallback: LiveRendererFrameCorrelation,
        now: Instant,
    ) -> Option<u32> {
        self.expire(now);
        let candidate = self.candidate.as_ref()?;
        (candidate.fallback.is_none()
            && fallback.native == candidate.source.original.native
            && fallback.trace == candidate.source.original.trace
            && fallback.direct_scanout
                == Some(DirectScanoutVerdict::CompositionRequired("refused")))
        .then_some(candidate.source.buffer.descriptor.format)
    }

    /// Called for the owned fallback at inline render or after its worker submission succeeds.
    pub fn bind(&mut self, fallback: LiveRendererFrameCorrelation, now: Instant) -> bool {
        self.expire(now);
        let Some(candidate) = self.candidate.as_mut() else {
            return false;
        };
        if candidate.fallback.is_some()
            || fallback.native != candidate.source.original.native
            || fallback.trace != candidate.source.original.trace
            || fallback.direct_scanout != Some(DirectScanoutVerdict::CompositionRequired("refused"))
        {
            return false;
        }
        candidate.fallback = Some(fallback);
        true
    }

    /// A deferred owned frame may be resubmitted, without extending the original deadline.
    pub fn unbind_deferred(
        &mut self,
        completed: LiveRendererFrameCorrelation,
        now: Instant,
    ) -> bool {
        self.expire(now);
        let Some(candidate) = self.candidate.as_mut() else {
            return false;
        };
        if candidate.fallback != Some(completed) {
            return false;
        }
        candidate.fallback = None;
        true
    }

    pub fn take(
        &mut self,
        completed: LiveRendererFrameCorrelation,
        descriptor: LiveRendererScanoutBufferDescriptor,
        now: Instant,
    ) -> Option<RetainedLayoutSource> {
        self.expire(now);
        if self.candidate.as_ref()?.fallback != Some(completed) {
            return None;
        }
        let candidate = self.candidate.take()?;
        let original = candidate.source.buffer.descriptor;
        (descriptor.is_valid_scanout_buffer()
            && descriptor.format == original.format
            && descriptor.size == original.size)
            .then_some(candidate.source)
    }

    pub fn invalidate(&mut self) {
        self.candidate = None;
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        if self
            .candidate
            .as_ref()
            .is_some_and(|candidate| now >= candidate.deadline)
        {
            self.invalidate();
            true
        } else {
            false
        }
    }
}
