use super::*;
use crate::{ContentStoreError, IsbImageAdmissionError, IsbSubmitFailure};

impl OwnedCopyOwner {
    pub(super) fn propose_offer(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        now: u64,
        instant: Instant,
        other_pending: usize,
    ) -> Result<(), ShellTransportError> {
        crate::shell_work_cost::result(crate::shell_work_cost::Stage::Admission, 0, || {
            let Some((offer, deadline)) = transport
                .files_mut()
                .and_then(|files| files.export().owned_pending_offer())
            else {
                self.waiting = None;
                return Ok(());
            };
            if self.waiting.is_none_or(|waiting| waiting.offer != offer) {
                self.waiting = Some(Waiting {
                    offer,
                    deadline,
                    decision: Decision::AwaitingAdmission,
                });
            }
            let waiting = self.waiting.expect("current pending offer");
            let available = transport
                .files_mut()
                .ok_or(ShellTransportError::NotConnected)?
                .export()
                .owned_admission_available(epochs, worker.pending());
            let reason = match waiting.decision {
                Decision::Refusing(reason) => Some(reason),
                Decision::AwaitingAdmission if instant >= waiting.deadline => {
                    Some(ContentReason::Timeout)
                }
                Decision::AwaitingAdmission
                    if self.active.as_ref().map(|active| active.identity)
                        != Some(offer.identity) =>
                {
                    Some(ContentReason::Stale)
                }
                Decision::AwaitingAdmission if available == Err(sophia_9p::Errno::ESTALE) => {
                    Some(ContentReason::Stale)
                }
                Decision::AwaitingAdmission if available.is_err() => Some(ContentReason::Malformed),
                Decision::AwaitingAdmission
                    if self.pending() + other_pending >= 4 || available == Ok(false) =>
                {
                    None
                }
                Decision::AwaitingAdmission => {
                    let active = self.active.as_mut().expect("matching admitted ring");
                    match self.jobs.reserve(
                        epochs,
                        worker,
                        &active.ring,
                        offer.request,
                        now,
                        waiting.deadline,
                    ) {
                        Ok(job) => {
                            // Reservation checks fixed geometry, slot ownership and
                            // allocation against authority; only then take the read
                            // ticket. No source bytes are read on this owner thread.
                            let source = offer
                                .request
                                .pixel_bytes()
                                .ok_or_else(|| {
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidInput,
                                        "slot payload extent",
                                    )
                                })
                                .and_then(|bytes| active.ring.offer(offer.request.slot, bytes));
                            *self
                                .work
                                .iter_mut()
                                .find(|entry| entry.is_none())
                                .expect("global owner credit") = Some(Work::Working {
                                identity: offer.identity,
                                job,
                                withdrawn: false,
                                handed_off: source.is_ok(),
                            });
                            transport
                                .files_mut()
                                .expect("offer export remains current")
                                .export_mut()
                                .owned_take_offer();
                            self.waiting = None;
                            let submitted = match source {
                                Ok(source) => match self.jobs.submit(worker, job, source) {
                                    Ok(()) => true,
                                    Err((_, source)) => {
                                        drop(source);
                                        false
                                    }
                                },
                                Err(_) => false,
                            };
                            if !submitted {
                                self.jobs.withdraw(
                                    epochs,
                                    worker,
                                    job,
                                    IsbImageWithdrawal::Cancelled,
                                    now,
                                )?;
                            }
                            return Ok(());
                        }
                        Err(IsbImageAdmissionError::Worker(IsbSubmitFailure::Capacity))
                        | Err(IsbImageAdmissionError::Resource(ContentStoreError::Budget)) => None,
                        Err(IsbImageAdmissionError::Allocation(error)) => Some(error.reason()),
                        Err(IsbImageAdmissionError::Resource(ContentStoreError::Stale)) => {
                            Some(ContentReason::Stale)
                        }
                        Err(IsbImageAdmissionError::Resource(ContentStoreError::Revoked)) => {
                            Some(ContentReason::Revoked)
                        }
                        Err(_) => Some(ContentReason::Malformed),
                    }
                }
            };
            if let Some(reason) = reason {
                self.waiting.as_mut().expect("pending refusal").decision =
                    Decision::Refusing(reason);
                let files = transport
                    .files_mut()
                    .ok_or(ShellTransportError::NotConnected)?;
                let before = files.export().owned_publication_sequence();
                let publication = files.export_mut().owned_refuse_offer(reason);
                if before != files.export().owned_publication_sequence() {
                    files.image_publication_pending();
                }
                match publication {
                    Ok(()) => self.waiting = None,
                    // The fixed refusal is retained; do not wake again merely
                    // because the already-decided deadline is in the past.
                    Err(sophia_9p::Errno::EAGAIN) => {}
                    Err(error) => return Err(export_error("offer refusal", error)),
                }
            }
            Ok(())
        })
    }
}
