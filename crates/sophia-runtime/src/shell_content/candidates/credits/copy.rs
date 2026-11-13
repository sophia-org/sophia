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
    /// A later generation holds the output, the store is revoked, or the
    /// exact stream is closing, closed or replaced.
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
    /// alone never reports `Ended`. A live state needs the stream's exact key;
    /// a closing stream keeps its key, so `Ended` also needs the phase.
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
        let ledger = self.streams.entries.get(&output).map(|entry| &entry.ledger);
        let same_key = ledger.and_then(|ledger| ledger.key()) == Some(binding.key);
        let assembly = self.assemblies.get(&output);
        let exact = |candidate: Option<&Candidate>| {
            same_key
                && candidate.is_some_and(|c| c.begin.candidate_generation == binding.generation)
        };
        if same_key
            && assembly.is_some_and(|assembly| {
                assembly.begin.candidate_generation == binding.generation
                    && assembly.copy.as_ref().is_some_and(|copy| copy.job == job)
            })
        {
            return FrameCandidateState::Assembling;
        }
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
        let open =
            same_key && ledger.is_some_and(|ledger| ledger.phase() == FrameStreamPhase::Open);
        if later || self.revoked || !open {
            FrameCandidateState::Ended
        } else {
            FrameCandidateState::Unknown
        }
    }

    /// The exact stream's ledger phase and the transaction its close
    /// notice carries (the opening's until a Close is admitted), read from
    /// existing state only; `None` unless `key` is its output's current
    /// stream key. A test-support observer (G3-5d d-2b3a's Close witness).
    #[cfg(all(
        feature = "experimental-combined-frame-job",
        any(test, feature = "test-support")
    ))]
    pub fn frame_stream_fact(
        &self,
        key: FrameStreamKey,
    ) -> Option<(FrameStreamPhase, TransactionId)> {
        let entry = self.streams.entries.get(&key.output)?;
        (entry.ledger.key() == Some(key))
            .then_some((entry.ledger.phase(), entry.notice_transaction))
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
