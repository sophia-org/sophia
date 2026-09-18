//! Close cancels only work which has not crossed the renderer commitment.
use super::*;

impl ContentCandidateStore {
    /// Caller has validated the transport's exact current opening. Permits and
    /// standing demands carry that connection/output authority; candidate row
    /// metadata additionally pins the opening and is checked before mutation.
    pub(crate) fn close_native_opening(
        &mut self,
        opening: NativeLauncherOpening,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(opening.grant)?;
        if self.profile != ContentStoreProfile::NativeLauncher || opening.opening == 0 {
            return Err(ContentCandidateError::Malformed);
        }
        let matches = |output: ContentOutputId, binding: Option<NativeLauncherCandidateBinding>| {
            output == opening.output
                && binding.is_some_and(|v| {
                    v.opening == opening.opening
                        && v.catalog_generation == opening.catalog_generation
                })
        };
        if self
            .assemblies
            .iter()
            .any(|(output, v)| !matches(*output, v.native_launcher))
            || self
                .pending
                .iter()
                .chain(self.submitted.iter())
                .any(|(output, v)| !matches(*output, v.native_launcher))
            || self
                .permits
                .keys()
                .chain(self.demands.keys())
                .any(|output| *output != opening.output)
        {
            return Err(ContentCandidateError::Stale);
        }
        let output = opening.output;
        if let Some(demand) = self.demands.remove(&output) {
            self.response_credits -= 1;
            self.push(
                demand.transaction,
                ShellContentRecord::FramePermit(ContentFramePermit {
                    grant: opening.grant,
                    output,
                    demand_id: demand.request.demand_id,
                    permit_id: 0,
                    state: 3,
                    reason: ContentReason::Cancelled as u16,
                    ttl_ms: 0,
                    max_candidate_bytes: 0,
                }),
            );
        }
        if let Some(permit) = self.permits.remove(&output) {
            self.response_credits -= 2;
            self.push(
                permit.transaction,
                ShellContentRecord::FramePermit(ContentFramePermit {
                    grant: opening.grant,
                    output,
                    demand_id: permit.demand_id,
                    permit_id: permit.permit_id,
                    state: 3,
                    reason: ContentReason::Cancelled as u16,
                    ttl_ms: 0,
                    max_candidate_bytes: 0,
                }),
            );
        }
        if let Some(assembly) = self.assemblies.remove(&output) {
            self.response_credits -= 2;
            self.outcome(
                assembly.transaction,
                assembly.begin.candidate_generation,
                output,
                3,
                ContentReason::Cancelled,
                0,
                0,
                0,
            );
        }
        if let Some(candidate) = self.pending.remove(&output) {
            self.response_credits -= 2;
            self.outcome(
                candidate.transaction,
                candidate.begin.candidate_generation,
                output,
                3,
                ContentReason::Cancelled,
                0,
                0,
                0,
            );
        }
        // Submitted leases, their Prepared/Presented credits and existing FIFO
        // events remain with their real owners. Repeated close emits no copies.
        Ok(())
    }
}
