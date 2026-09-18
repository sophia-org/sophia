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

impl ContentCandidateStore {
    pub(crate) fn reject_closed_native_begin(
        &mut self,
        transaction: TransactionId,
        begin: NativeLauncherCandidateBegin,
        opening: NativeLauncherOpening,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(begin.content.grant)?;
        if self.profile != ContentStoreProfile::NativeLauncher
            || !transaction.is_valid()
            || begin.opening != opening.opening
            || begin.content.output != opening.output
            || begin.catalog_generation != opening.catalog_generation
            || begin.content.candidate_generation == 0
        {
            return Err(ContentCandidateError::Stale);
        }
        if begin.content.candidate_generation <= self.last_candidate_generation {
            return Ok(());
        }
        if self.control_occupancy() >= self.limits.max_control_records as usize {
            return Err(ContentCandidateError::Budget);
        }
        self.last_candidate_generation = begin.content.candidate_generation;
        self.outcome(
            transaction,
            begin.content.candidate_generation,
            begin.content.output,
            3,
            ContentReason::Cancelled,
            0,
            0,
            0,
        );
        Ok(())
    }
    pub(crate) fn closed_native_tail(
        &self,
        grant: ContentGrant,
        generation: u64,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(grant)?;
        if self.profile != ContentStoreProfile::NativeLauncher
            || generation == 0
            || generation > self.last_candidate_generation
        {
            return Err(ContentCandidateError::Stale);
        }
        Ok(())
    }
    pub(crate) fn reject_closed_native_demand(
        &mut self,
        transaction: TransactionId,
        demand: ContentFrameDemand,
        opening: NativeLauncherOpening,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(demand.grant)?;
        if self.profile != ContentStoreProfile::NativeLauncher
            || !transaction.is_valid()
            || demand.output != opening.output
            || demand.demand_id == 0
        {
            return Err(ContentCandidateError::Stale);
        }
        if demand.demand_id <= self.last_demand_id {
            return Ok(());
        }
        if self.control_occupancy() >= self.limits.max_control_records as usize {
            return Err(ContentCandidateError::Budget);
        }
        self.last_demand_id = demand.demand_id;
        self.push(
            transaction,
            ShellContentRecord::FramePermit(ContentFramePermit {
                grant: demand.grant,
                output: demand.output,
                demand_id: demand.demand_id,
                permit_id: 0,
                state: 3,
                reason: ContentReason::Cancelled as u16,
                ttl_ms: 0,
                max_candidate_bytes: 0,
            }),
        );
        Ok(())
    }
    pub(crate) fn closed_native_cancel(
        &self,
        cancel: ContentFrameDemandCancel,
        opening: NativeLauncherOpening,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(cancel.grant)?;
        if cancel.output != opening.output
            || cancel.demand_id == 0
            || cancel.demand_id > self.last_demand_id
            || cancel.permit_id > self.last_permit_id
        {
            return Err(ContentCandidateError::Stale);
        }
        Ok(())
    }
}
