//! Old opening records share the real FIFO and high-water owners after reopen.
use super::{content::NativeContentRecord, *};

impl ShellComponentTransport {
    /// Called in receive order under the ordinary per-visit budget. A retained
    /// closed identity authorizes refusal only, never current admission.
    pub(super) fn service_previous_native_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: &NativeContentRecord,
        context: crate::ContentCandidateContext<'_>,
    ) -> Result<bool, ShellTransportError> {
        let Some(closed) = self.native_control.closed else {
            return Ok(false);
        };
        match record {
            NativeContentRecord::Allocation(request) if request.opening == closed.opening => {
                epochs
                    .allocations_mut(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?
                    .reject_closed_native_request(transaction, *request, closed)?;
            }
            NativeContentRecord::Begin(begin) if begin.opening == closed.opening => {
                epochs
                    .active_candidates_mut(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?
                    .reject_closed_native_begin(transaction, begin.clone(), closed)?;
            }
            NativeContentRecord::Chunk(chunk) => {
                let candidates = epochs
                    .active_candidates(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?;
                if candidates
                    .assembling_output(chunk.candidate_generation)
                    .is_some()
                {
                    return Ok(false);
                }
                candidates.closed_native_tail(chunk.grant, chunk.candidate_generation)?;
            }
            NativeContentRecord::End(end) => {
                let candidates = epochs
                    .active_candidates(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?;
                if candidates
                    .assembling_output(end.candidate_generation)
                    .is_some()
                {
                    return Ok(false);
                }
                candidates.closed_native_tail(end.grant, end.candidate_generation)?;
            }
            NativeContentRecord::Demand(demand)
                if demand.output == closed.output
                    && demand.allocation != ContentAllocationId::default()
                    && !context
                        .allocations
                        .iter()
                        .any(|v| v.allocation == demand.allocation) =>
            {
                epochs
                    .active_candidates_mut(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?
                    .reject_closed_native_demand(transaction, demand.clone(), closed)?;
            }
            // Cancel carries no opening. The ordinary store first checks exact
            // current demand/permit; its stale path is handled there separately.
            _ => return Ok(false),
        }
        Ok(true)
    }
}
