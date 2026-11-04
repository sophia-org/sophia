use sophia_protocol::*;

use super::{ContentAllocationSnapshot, ContentCandidateError, ContentCandidateStore, allocation};

#[derive(Clone)]
pub(super) struct StandingDemand {
    pub(super) transaction: TransactionId,
    pub(super) request: ContentFrameDemand,
}

impl ContentCandidateStore {
    /// Coalesce one client request per output. A withdrawal cannot be replaced
    /// by lower-priority dirty or animation work.
    pub fn demand(
        &mut self,
        transaction: TransactionId,
        request: ContentFrameDemand,
        outputs: &[ContentOutputId],
        allocations: &[ContentAllocationSnapshot],
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(request.grant)?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.require_demand_mode(request.output)?;
        if !transaction.is_valid()
            || request.output.id == 0
            || request.output.generation == 0
            || !outputs.contains(&request.output)
            || request.demand_id <= self.last_demand_id
            || !(1..=3).contains(&request.reason)
            || (request.allocation != ContentAllocationId::default()
                && allocation(allocations, request.allocation)?.output != request.output)
        {
            return Err(ContentCandidateError::Stale);
        }
        let replacing = self.demands.contains_key(&request.output);
        if !replacing
            && self.events.len() + self.response_credits + 1
                > self.limits.max_control_records as usize
        {
            return Err(ContentCandidateError::Budget);
        }
        if self
            .demands
            .get(&request.output)
            .is_some_and(|current| current.request.reason == 3 && request.reason != 3)
        {
            return Err(ContentCandidateError::Stale);
        }
        self.last_demand_id = request.demand_id;
        if !replacing {
            self.response_credits += 1;
        }
        self.demands.insert(
            request.output,
            StandingDemand {
                transaction,
                request,
            },
        );
        Ok(())
    }

    pub fn next_demand(&self) -> Option<(TransactionId, ContentFrameDemand)> {
        self.demands
            .values()
            .find(|demand| demand.request.reason == 3)
            .or_else(|| self.demands.values().next())
            .map(|demand| (demand.transaction, demand.request.clone()))
    }

    pub fn grant_demand(
        &mut self,
        transaction: TransactionId,
        output: ContentOutputId,
        permit_id: u64,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        let demand = self
            .demands
            .remove(&output)
            .ok_or(ContentCandidateError::Stale)?;
        self.response_credits -= 1;
        match self.grant_permit(
            transaction,
            output,
            demand.request.demand_id,
            permit_id,
            now,
        ) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.response_credits += 1;
                self.demands.insert(output, demand);
                Err(error)
            }
        }
    }

    pub fn cancel_demand(
        &mut self,
        transaction: TransactionId,
        cancel: ContentFrameDemandCancel,
    ) -> Result<(), ContentCandidateError> {
        self.check_grant(cancel.grant)?;
        #[cfg(feature = "experimental-native-frame-transaction")]
        self.require_demand_mode(cancel.output)?;
        if !transaction.is_valid() {
            return Err(ContentCandidateError::Stale);
        }
        let cancelled = if cancel.permit_id == 0 {
            let standing = self
                .demands
                .get(&cancel.output)
                .filter(|demand| demand.request.demand_id == cancel.demand_id)
                .is_some();
            if standing {
                self.demands.remove(&cancel.output);
                self.response_credits -= 1;
            }
            standing.then_some(0)
        } else {
            let permit_id = self
                .permits
                .get(&cancel.output)
                .filter(|permit| {
                    permit.demand_id == cancel.demand_id && permit.permit_id == cancel.permit_id
                })
                .map(|permit| permit.permit_id);
            if permit_id.is_some() {
                self.permits.remove(&cancel.output);
                self.response_credits -= 2;
            }
            permit_id
        }
        .ok_or(ContentCandidateError::Stale)?;
        self.push(
            transaction,
            ShellContentRecord::FramePermit(ContentFramePermit {
                grant: self.limits.grant,
                output: cancel.output,
                demand_id: cancel.demand_id,
                permit_id: cancelled,
                state: 3,
                reason: ContentReason::Cancelled as u16,
                ttl_ms: 0,
                max_candidate_bytes: 0,
            }),
        );
        Ok(())
    }
}
