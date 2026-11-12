//! A candidate cannot settle while its existing image job still reads the source.
use super::*;

/// The exact frame-stream candidate a copy job was admitted for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameCopyBinding {
    pub key: FrameStreamKey,
    pub generation: u64,
}

/// Where a copy job's candidate stands, read from existing state only.
#[cfg(any(test, feature = "experimental-combined-frame-job"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameCandidateState {
    /// Its assembly is open and the copy gate still names this job.
    Assembling,
    Pending,
    Submitted,
    /// A later generation holds the output, or the stream is closed.
    Ended,
    /// None of the above; the caller treats this as live until its deadline.
    Unknown,
}

pub(crate) struct FrameCopyState {
    pub job: u64,
    pub stopped: Option<ContentCandidateError>,
}

impl ContentCandidateStore {
    /// Terminal outcomes are emitted, not kept, so absence from these tables
    /// alone never reports `Ended`.
    #[cfg(any(test, feature = "experimental-combined-frame-job"))]
    pub fn frame_candidate_state(
        &self,
        binding: &FrameCopyBinding,
        job: u64,
    ) -> FrameCandidateState {
        if binding.key.grant != self.limits.grant {
            return FrameCandidateState::Unknown;
        }
        let output = binding.key.output;
        let assembly = self.assemblies.get(&output);
        if assembly.is_some_and(|assembly| {
            assembly.begin.candidate_generation == binding.generation
                && assembly.copy.as_ref().is_some_and(|copy| copy.job == job)
        }) {
            return FrameCandidateState::Assembling;
        }
        let exact = |candidate: Option<&Candidate>| {
            candidate.is_some_and(|c| c.begin.candidate_generation == binding.generation)
        };
        if exact(self.pending.get(&output)) {
            return FrameCandidateState::Pending;
        }
        if exact(self.submitted.get(&output)) {
            return FrameCandidateState::Submitted;
        }
        // Generations strictly increase, and each output holds at most one
        // assembly and one pending entry, so a later one ends this binding.
        let later = assembly
            .map(|a| a.begin.candidate_generation)
            .into_iter()
            .chain(
                self.pending
                    .get(&output)
                    .map(|c| c.begin.candidate_generation),
            )
            .chain(
                self.submitted
                    .get(&output)
                    .map(|c| c.begin.candidate_generation),
            )
            .any(|generation| generation > binding.generation);
        let open = self
            .streams
            .entries
            .get(&output)
            .and_then(|entry| entry.ledger.key())
            == Some(binding.key);
        if later || self.revoked || !open {
            FrameCandidateState::Ended
        } else {
            FrameCandidateState::Unknown
        }
    }

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
