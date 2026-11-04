//! A candidate cannot settle while its existing image job still reads the source.
use super::*;

#[derive(Debug)]
pub(crate) struct FrameCopyBinding {
    pub key: FrameStreamKey,
    pub generation: u64,
}

pub(crate) struct FrameCopyState {
    pub job: u64,
    pub stopped: Option<ContentCandidateError>,
}

impl ContentCandidateStore {
    pub(crate) fn frame_stream_key(&self, output: ContentOutputId) -> Option<FrameStreamKey> {
        self.streams.entries.get(&output)?.ledger.key()
    }

    pub(crate) fn frame_copy_binding(&self, output: ContentOutputId) -> FrameCopyBinding {
        FrameCopyBinding {
            key: self.streams.entries[&output]
                .ledger
                .key()
                .expect("admitted stream"),
            generation: self.assemblies[&output].begin.candidate_generation,
        }
    }

    /// Called only with the settlement retained by the paired copy-job owner.
    pub(crate) fn finish_frame_copy(
        &mut self,
        binding: &FrameCopyBinding,
        job: u64,
        failure: Option<ContentCandidateError>,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if binding.key.grant != self.limits.grant {
            return Err(ContentCandidateError::Stale);
        }
        if self.revoked {
            return Ok(());
        }
        let assembly = self
            .assemblies
            .get(&binding.key.output)
            .ok_or(ContentCandidateError::Stale)?;
        if assembly.begin.candidate_generation != binding.generation
            || self
                .streams
                .entries
                .get(&binding.key.output)
                .and_then(|entry| entry.ledger.key())
                != Some(binding.key)
            || assembly.copy.as_ref().is_none_or(|copy| copy.job != job)
        {
            return Err(ContentCandidateError::Stale);
        }
        // Reject a bad clock before releasing the handoff owner; retry is safe.
        self.time(now)?;
        let assembly = self
            .assemblies
            .get_mut(&binding.key.output)
            .expect("validated copy");
        let copy = assembly.copy.take().expect("validated copy job");
        let failure = copy
            .stopped
            .or(failure)
            .or_else(|| (now >= assembly.deadline).then_some(ContentCandidateError::Timeout));
        if let Some(error) = failure {
            self.reject_assembly(binding.key.output, error);
        }
        Ok(())
    }
}
