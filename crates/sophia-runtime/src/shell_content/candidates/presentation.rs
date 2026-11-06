use super::*;

impl ContentCandidateStore {
    /// `prepared` means the complete bundle was actually handed to rendering.
    pub fn prepared(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.prepare(
            output,
            candidate_generation,
            work_area_generation,
            wm_commit_generation,
            now,
            true,
        )
    }

    /// The transport has checked the explicitly negotiated terminal-only mode.
    #[cfg(feature = "experimental-native-frame-transaction")]
    pub(crate) fn prepared_without_notice(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if !self.stream_owns(output, candidate_generation) {
            return Err(ContentCandidateError::Stale);
        }
        self.prepare(
            output,
            candidate_generation,
            work_area_generation,
            wm_commit_generation,
            now,
            false,
        )
    }

    fn prepare(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
        now: u64,
        publish_notice: bool,
    ) -> Result<(), ContentCandidateError> {
        self.time(now)?;
        let candidate = self
            .submitted
            .get_mut(&output)
            .ok_or(ContentCandidateError::Stale)?;
        if candidate.begin.candidate_generation != candidate_generation
            || candidate.prepared
            || work_area_generation == 0
            || wm_commit_generation == 0
        {
            return Err(ContentCandidateError::Stale);
        }
        candidate.prepared = true;
        candidate.deadline = now
            .checked_add(u64::from(self.limits.presentation_timeout_ms))
            .ok_or(ContentCandidateError::Malformed)?;
        let transaction = candidate.transaction;
        if !self.revoked {
            self.response_credits -= 1;
            if publish_notice {
                self.outcome(
                    transaction,
                    candidate_generation,
                    output,
                    1,
                    ContentReason::None,
                    0,
                    work_area_generation,
                    wm_commit_generation,
                );
            }
        }
        Ok(())
    }

    /// Emit Presented only after the caller has observed native retirement.
    /// Legacy terminal entry point; stream candidates require an explicit clock.
    pub fn presented(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        presentation_epoch: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
    ) -> Result<(), ContentCandidateError> {
        #[cfg(feature = "experimental-native-frame-transaction")]
        if self.stream_owns(output, candidate_generation) {
            return Err(ContentCandidateError::Stale);
        }
        self.presented_inner(
            output,
            candidate_generation,
            presentation_epoch,
            work_area_generation,
            wm_commit_generation,
        )
    }

    /// Queue a terminal using the owner's current clock; output transfer returns credit.
    #[cfg(feature = "experimental-native-frame-transaction")]
    pub fn presented_at(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        presentation_epoch: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if !self.revoked {
            self.time(now)?;
        }
        self.presented_inner(
            output,
            candidate_generation,
            presentation_epoch,
            work_area_generation,
            wm_commit_generation,
        )
    }

    fn presented_inner(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        presentation_epoch: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
    ) -> Result<(), ContentCandidateError> {
        let candidate = self
            .submitted
            .get(&output)
            .ok_or(ContentCandidateError::Stale)?;
        if candidate.begin.candidate_generation != candidate_generation
            || !candidate.prepared
            || presentation_epoch == 0
            || work_area_generation == 0
            || wm_commit_generation == 0
        {
            return Err(ContentCandidateError::Stale);
        }
        let candidate = self
            .submitted
            .remove(&output)
            .expect("validated submission");
        if !self.revoked {
            self.response_credits -= 1;
            self.outcome(
                candidate.transaction,
                candidate_generation,
                output,
                2,
                ContentReason::None,
                presentation_epoch,
                work_area_generation,
                wm_commit_generation,
            );
            // The queued Presented now owns the intent; only its transfer to
            // the output commits it.
            #[cfg(feature = "experimental-native-frame-transaction")]
            if let Some((_, token)) = candidate.retire {
                let owner = self.retire_owner(output, candidate_generation);
                self.events.back_mut().expect("Presented queued").retire = Some(owner);
                self.presented_retire.push((owner, token));
            }
        }
        Ok(())
    }

    /// Legacy terminal entry point; stream candidates require an explicit clock.
    pub fn renderer_failed(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
    ) -> Result<(), ContentCandidateError> {
        #[cfg(feature = "experimental-native-frame-transaction")]
        if self.stream_owns(output, candidate_generation) {
            return Err(ContentCandidateError::Stale);
        }
        self.renderer_failed_inner(output, candidate_generation)
    }

    /// Queue a terminal using the owner's current clock; output transfer returns credit.
    #[cfg(feature = "experimental-native-frame-transaction")]
    pub fn renderer_failed_at(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if !self.revoked {
            self.time(now)?;
        }
        self.renderer_failed_inner(output, candidate_generation)
    }

    fn renderer_failed_inner(
        &mut self,
        output: ContentOutputId,
        candidate_generation: u64,
    ) -> Result<(), ContentCandidateError> {
        let candidate = self
            .submitted
            .get(&output)
            .ok_or(ContentCandidateError::Stale)?;
        if candidate.begin.candidate_generation != candidate_generation {
            return Err(ContentCandidateError::Stale);
        }
        let candidate = self
            .submitted
            .remove(&output)
            .expect("validated submission");
        if !self.revoked {
            self.response_credits -= if candidate.prepared { 1 } else { 2 };
            self.outcome(
                candidate.transaction,
                candidate_generation,
                output,
                3,
                ContentReason::RendererFailed,
                0,
                0,
                0,
            );
        }
        Ok(())
    }
}
