//! W2 prototype: End admits a candidate with its retire intent, and the
//! store reports which intents its candidates and queued Presented still own.
use super::*;

impl ContentCandidateStore {
    pub(super) fn retire_owner(
        &self,
        output: ContentOutputId,
        candidate_generation: u64,
    ) -> ContentRetireOwner {
        ContentRetireOwner {
            grant: self.limits.grant,
            output,
            candidate_generation,
        }
    }

    /// W2 prototype End: the candidate and its retire intent are admitted, or
    /// rejected, together. The candidate may not draw what it retires.
    pub fn end_with_retire(
        &mut self,
        transaction: TransactionId,
        end: ContentCandidateEnd,
        retire: ContentRetireOnPresent,
        context: ContentCandidateContext<'_>,
        resources: &mut ContentResourceStore,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        // The same family guard as `end`, before any time or state moves.
        if self.profile != ContentStoreProfile::Legacy {
            return Err(ContentCandidateError::Malformed);
        }
        self.check_grant(end.grant)?;
        self.time(now)?;
        self.expire(now)?;
        let output = self.end_output(end.candidate_generation)?;
        let owner = self.retire_owner(output, end.candidate_generation);
        let result = self
            .validate_end(
                transaction,
                &end,
                context,
                CandidateAuthority::Legacy,
                resources,
                output,
            )
            .and_then(|mut candidate| {
                if retire.retire_transaction == candidate.transaction
                    || candidate
                        .resources
                        .iter()
                        .any(|(id, _)| *id == retire.resource)
                {
                    return Err(ContentCandidateError::Stale);
                }
                let token = ContentRetireToken::default();
                resources.reserve_retire(owner, retire, &token)?;
                candidate.retire = Some((retire, token));
                Ok(candidate)
            });
        match result {
            Ok(candidate) => {
                self.assemblies.remove(&output);
                self.pending.insert(output, candidate);
                Ok(())
            }
            Err(error) => {
                self.reject_assembly(output, error);
                Err(error)
            }
        }
    }
}
