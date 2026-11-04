use super::*;

impl ContentCandidateStore {
    pub(super) fn begin_inner(
        &mut self,
        transaction: TransactionId,
        begin: ContentCandidateBegin,
        native_launcher: Option<NativeLauncherCandidateBinding>,
        persistent_catalog: Option<PersistentCatalogCandidateBinding>,
        invalid_native: Option<ContentCandidateError>,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(begin.grant)?;
        self.time(now)?;
        self.expire(now)?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        if self.streams.entries.contains_key(&begin.output) {
            if native_launcher.is_some() || persistent_catalog.is_some() {
                return Err(ContentCandidateError::Malformed);
            }
            return self.begin_frame_stream(transaction, begin, invalid_native, now);
        }
        let permit = self
            .permits
            .get(&begin.output)
            .ok_or(ContentCandidateError::Stale)?;
        if !transaction.is_valid() || permit.permit_id != begin.pacing_permit {
            return Err(ContentCandidateError::Stale);
        }
        let invalid = if let Some(error) = invalid_native {
            Some(error)
        } else if now >= permit.deadline
            || begin.candidate_generation <= self.last_candidate_generation
        {
            Some(ContentCandidateError::Stale)
        } else if begin.facts_generation == 0
            || begin.interaction_generation == 0
            || begin.surface_count > self.limits.max_candidate_surfaces
            || begin.placement_count > self.limits.max_candidate_placements
            || begin.target_count > self.limits.max_candidate_targets
        {
            Some(ContentCandidateError::Malformed)
        } else {
            None
        };
        self.permits.remove(&begin.output);
        // A consumed Begin owns this generation even when its contents refuse.
        // Retrying under a fresh permit must not recycle that terminal identity.
        self.last_candidate_generation = self
            .last_candidate_generation
            .max(begin.candidate_generation);
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.observe_stream_generation();
        if let Some(error) = invalid {
            self.response_credits -= 2;
            self.outcome(
                transaction,
                begin.candidate_generation,
                begin.output,
                3,
                error.reason(),
                0,
                0,
                0,
            );
            return Err(error);
        }
        let deadline = now
            .checked_add(u64::from(self.limits.candidate_timeout_ms))
            .ok_or(ContentCandidateError::Malformed)?;
        self.assemblies.insert(
            begin.output,
            Assembly {
                #[cfg(feature = "experimental-native-frame-transaction")]
                copy: None,
                native_launcher,
                persistent_catalog,
                transaction,
                begin,
                deadline,
                next_ordinal: 0,
                surfaces: Vec::new(),
                placements: Vec::new(),
                targets: Vec::new(),
            },
        );
        Ok(())
    }
}
