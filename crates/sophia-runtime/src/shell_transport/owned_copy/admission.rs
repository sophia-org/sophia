use super::*;
use crate::{ContentStoreError, IsbImageAdmissionError, IsbSubmitFailure};

/// A combined frame's half of the submission: the hooks that issued its
/// ticket, the renderer inbox, and the combined credit's reservation.
#[cfg(all(
    feature = "experimental-native-frame-transaction",
    any(test, feature = "experimental-combined-frame-job")
))]
pub(super) struct CombinedSubmit<'a, 'h, P> {
    pub(super) hooks: &'a mut (dyn crate::CombinedFrameHooks<P> + 'h),
    pub(super) ticket: u64,
    pub(super) inbox: crate::CombinedInbox<P>,
    pub(super) reservation: crate::CombinedReservation,
}

impl OwnedCopyOwner {
    pub(super) fn propose_offer<P: Send + 'static>(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
        now: u64,
        instant: Instant,
        other_pending: usize,
    ) -> Result<(), ShellTransportError> {
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
                if self.active.as_ref().map(|active| active.identity) != Some(offer.identity) =>
            {
                Some(ContentReason::Stale)
            }
            Decision::AwaitingAdmission if available == Err(sophia_9p::Errno::ESTALE) => {
                Some(ContentReason::Stale)
            }
            Decision::AwaitingAdmission if available.is_err() => Some(ContentReason::Malformed),
            #[cfg(feature = "experimental-native-frame-transaction")]
            Decision::AwaitingAdmission
                if transport.capabilities
                    & sophia_protocol::shell_files::SHELL_FRAME_CAPABILITY
                    != 0
                    && epochs
                        .active_candidates(offer.identity.grant)
                        .is_none_or(|store| {
                            store.require_demand_mode(offer.request.output).is_err()
                        }) =>
            {
                // Legacy uploads cannot bypass an open or still-draining stream.
                Some(ContentReason::Stale)
            }
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
                        transport
                            .files_mut()
                            .expect("offer export remains current")
                            .export_mut()
                            .owned_take_offer();
                        self.waiting = None;
                        self.submit_reserved_copy(
                            offer,
                            job,
                            epochs,
                            worker,
                            now,
                            #[cfg(all(
                                feature = "experimental-native-frame-transaction",
                                any(test, feature = "experimental-combined-frame-job")
                            ))]
                            None,
                        )?;
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
            self.waiting.as_mut().expect("pending refusal").decision = Decision::Refusing(reason);
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
    }

    pub(super) fn submit_reserved_copy<P: Send + 'static>(
        &mut self,
        offer: Offer,
        job: u64,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
        now: u64,
        #[cfg(all(
            feature = "experimental-native-frame-transaction",
            any(test, feature = "experimental-combined-frame-job")
        ))]
        combined: Option<CombinedSubmit<'_, '_, P>>,
    ) -> Result<(), ShellTransportError> {
        // Admission owns response, worker and source slots before this read ticket.
        let active = self.active.as_mut().expect("matching admitted ring");
        let source = offer
            .request
            .pixel_bytes()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "slot payload extent")
            })
            .and_then(|bytes| active.ring.offer(offer.request.slot, bytes));
        *self
            .work
            .iter_mut()
            .find(|entry| entry.is_none())
            .expect("copy admission reserved a work slot") = Some(Work::Working {
            identity: offer.identity,
            job,
            withdrawn: false,
            handed_off: source.is_ok(),
        });
        #[cfg(all(
            feature = "experimental-native-frame-transaction",
            any(test, feature = "experimental-combined-frame-job")
        ))]
        if let Some(combined) = combined {
            // Only the send differs from the ordinary steps around it (R1).
            let submitted = self.send_combined(worker, job, source, combined);
            if !submitted {
                self.jobs
                    .withdraw(epochs, worker, job, IsbImageWithdrawal::Cancelled, now)?;
            }
            return Ok(());
        }
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
            self.jobs
                .withdraw(epochs, worker, job, IsbImageWithdrawal::Cancelled, now)?;
        }
        Ok(())
    }
}

#[cfg(all(
    feature = "experimental-native-frame-transaction",
    any(test, feature = "experimental-combined-frame-job")
))]
impl OwnedCopyOwner {
    /// The combined send. It ends the ticket exactly once: `submitted` when
    /// the validator took the job, otherwise `cancel`, after which the caller
    /// withdraws the job as a refused send. A plan-build failure is a refused
    /// send. The ring ticket and the plan are dropped here, on the owner.
    fn send_combined<P: Send + 'static>(
        &mut self,
        worker: &mut IsbValidationWorker<P>,
        job: u64,
        source: std::io::Result<crate::OwnedRingRead>,
        combined: CombinedSubmit<'_, '_, P>,
    ) -> bool {
        let CombinedSubmit {
            hooks,
            ticket,
            inbox,
            reservation,
        } = combined;
        let plan = match &source {
            Ok(_) if reservation.job() == job => self
                .jobs
                .combined_admitted(job)
                .and_then(|admitted| hooks.plan(ticket, &admitted)),
            _ => None,
        };
        let (Ok(source), Some(plan)) = (source, plan) else {
            hooks.cancel(ticket);
            return false;
        };
        match self
            .jobs
            .submit_combined(worker, &reservation, source, plan, inbox)
        {
            Ok(returned) => {
                hooks.submitted(ticket, reservation, returned);
                true
            }
            Err((_, source, plan)) => {
                drop(source);
                drop(plan);
                hooks.cancel(ticket);
                false
            }
        }
    }
}
