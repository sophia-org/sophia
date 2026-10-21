//! D4 uses the existing worker slots, absolute deadline and wake source.
use super::*;
use crate::OwnedRingRead;

impl IsbValidationWorker {
    /// On refusal return the exact source ticket; it still owes a slot release.
    /// The caller may retry against this reservation or drop the ticket to stop
    /// all source reads, then reconcile its resource and journal obligations.
    pub fn submit_ring_reserved(
        &mut self,
        id: u64,
        source: OwnedRingRead,
    ) -> Result<(), (IsbSubmitFailure, OwnedRingRead)> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.id == id && !pending.submitted && pending.bytes == source.pixel_bytes()
        }) else {
            return Err((IsbSubmitFailure::Invalid, source));
        };
        let job = Job {
            id,
            source: Source::Ring(source),
            bytes: pending.bytes,
            deadline: pending.deadline,
            cancelled: pending.cancelled.clone(),
        };
        match self.sender.try_send(job) {
            Ok(()) => {
                pending.submitted = true;
                Ok(())
            }
            Err(error) => {
                let (failure, job) = match error {
                    mpsc::TrySendError::Full(job) => (IsbSubmitFailure::Capacity, job),
                    mpsc::TrySendError::Disconnected(job) => (IsbSubmitFailure::Closed, job),
                };
                let Source::Ring(source) = job.source else {
                    unreachable!("ring submission returns its own source")
                };
                Err((failure, source))
            }
        }
    }
}
