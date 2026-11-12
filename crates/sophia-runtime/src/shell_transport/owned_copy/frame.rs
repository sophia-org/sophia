use super::*;
use crate::{ContentCandidateError as Error, ContentFrameAdmission as Admission};
use sophia_protocol::shell_files::{SHELL_FRAME_CAPABILITY, ShellFrameRecord};

/// How far one frame-commit visit got after the request was peeked.
enum Step {
    /// Nothing more this visit; today's path returns without flushing.
    Stop,
    /// The admission ran; an accepted copy still needs its submission.
    Flush(Option<(Offer, u64)>),
}

impl OwnedCopyOwner {
    pub(super) fn propose_frame_commit<P: Send + 'static>(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
        now: u64,
        instant: Instant,
        other_pending: usize,
        #[cfg(any(test, feature = "experimental-combined-frame-job"))] mut hooks: Option<
            &mut (dyn crate::CombinedFrameHooks<P> + '_),
        >,
    ) -> Result<(), ShellTransportError> {
        // The worker outlives disconnect to settle old jobs, not to admit new input.
        if transport.capabilities & SHELL_FRAME_CAPABILITY == 0
            || transport.content_grant().is_none()
        {
            return Ok(());
        }
        let files = transport
            .files_mut()
            .ok_or(ShellTransportError::NotConnected)?;
        let Some(ShellFrameRecord::Commit(frame)) = files.export().peek_frame_request() else {
            return Ok(());
        };
        let offer = frame.image.map(Offer::from);
        let grant = frame.candidate.grant;
        // Only a copy-bearing frame can take the combined path.
        #[cfg(any(test, feature = "experimental-combined-frame-job"))]
        let ticket = match (hooks.as_mut(), offer) {
            (Some(hooks), Some(_)) => hooks.reserve(frame),
            _ => None,
        };
        #[cfg(any(test, feature = "experimental-combined-frame-job"))]
        let mut reservation = None;
        let step = self.admit_peeked_commit(
            transport,
            epochs,
            worker,
            offer,
            grant,
            now,
            instant,
            other_pending,
            #[cfg(any(test, feature = "experimental-combined-frame-job"))]
            ticket.is_some().then_some(&mut reservation),
        );
        #[cfg(any(test, feature = "experimental-combined-frame-job"))]
        if let Some((id, inbox)) = ticket {
            let hooks = hooks.expect("a ticket came from the hooks");
            match &step {
                Ok(Step::Flush(Some((offer, job)))) => {
                    let combined = admission::CombinedSubmit {
                        hooks,
                        ticket: id,
                        inbox,
                        reservation: reservation
                            .expect("an accepted combined copy took the combined credit"),
                    };
                    self.submit_reserved_copy(*offer, *job, epochs, worker, now, Some(combined))?;
                }
                // Refusals, deferrals and errors: no job owns the inbox.
                _ => hooks.cancel(id),
            }
            return match step? {
                Step::Stop => Ok(()),
                Step::Flush(_) => transport.flush_content_frame_events(epochs, now),
            };
        }
        match step? {
            Step::Stop => Ok(()),
            Step::Flush(copy) => {
                if let Some((offer, job)) = copy {
                    self.submit_reserved_copy(
                        offer,
                        job,
                        epochs,
                        worker,
                        now,
                        #[cfg(any(test, feature = "experimental-combined-frame-job"))]
                        None,
                    )?;
                }
                transport.flush_content_frame_events(epochs, now)
            }
        }
    }

    fn admit_peeked_commit<P: Send + 'static>(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker<P>,
        offer: Option<Offer>,
        grant: sophia_protocol::ContentGrant,
        now: u64,
        instant: Instant,
        other_pending: usize,
        #[cfg(any(test, feature = "experimental-combined-frame-job"))] combined: Option<
            &mut Option<crate::CombinedReservation>,
        >,
    ) -> Result<Step, ShellTransportError> {
        let files = transport
            .files_mut()
            .ok_or(ShellTransportError::NotConnected)?;
        let timeout = files
            .export()
            .image_validation_timeout()
            .ok_or(ShellTransportError::MissingCapability)?;
        if transport.content_grant != Some(grant) || transport.store_grant != grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let deadline = instant
            .checked_add(timeout)
            .ok_or(ShellTransportError::WrongContentRecord)?;
        let room = (0..=4)
            .rev()
            .find(|room| transport.control_capacity_available(epochs, *room))
            .unwrap_or(0);
        let mut source_error = None;
        let mut reservation = None;
        if let Some(offer) = offer {
            if self.active.as_ref().map(|active| active.identity) != Some(offer.identity) {
                return Err(Error::Stale.into());
            }
            let files = transport.files_mut().expect("current file owner");
            let available = files
                .export()
                .owned_offer_available(offer, epochs, worker.pending())
                .map_err(|error| export_error("frame source", error))?;
            if self.pending() + other_pending >= 4 || !available {
                source_error = Some(Error::Budget);
            } else if room >= 4 {
                match files.export_mut().reserve_owned_offer(offer) {
                    Ok(Some(held)) => reservation = Some(held),
                    Ok(None) => source_error = Some(Error::Budget),
                    Err(sophia_9p::Errno::EAGAIN) => return Ok(Step::Stop),
                    Err(sophia_9p::Errno::ESTALE) => return Err(Error::Stale.into()),
                    Err(sophia_9p::Errno::EALREADY) => source_error = Some(Error::Stale),
                    Err(_) => source_error = Some(Error::Malformed),
                }
            }
        }
        let result = transport
            .files_mut()
            .expect("current file owner")
            .export_mut()
            .with_frame_commit(|input| {
                self.jobs.admit_frame_commit_with_source(
                    epochs,
                    worker,
                    self.active.as_ref().map(|active| &active.ring),
                    input,
                    room,
                    source_error,
                    now,
                    deadline,
                    #[cfg(any(test, feature = "experimental-combined-frame-job"))]
                    combined,
                )
            });
        assert!(
            !matches!(result, Ok(Some(Admission::Accepted { copy_job: Some(_) })))
                || reservation.is_some(),
            "accepted copy owns both source response reservations"
        );
        if let Some(held) = reservation {
            let files = transport
                .files_mut()
                .expect("no I/O during frame reservation");
            if matches!(result, Ok(Some(Admission::Accepted { copy_job: Some(_) }))) {
                files.export_mut().commit_owned_offer(held);
            } else {
                files.export_mut().cancel_owned_offer(held);
            }
        }
        Ok(Step::Flush(match result? {
            Some(Admission::Accepted {
                copy_job: Some(job),
            }) => Some((offer.expect("copy admission has an exact source"), job)),
            _ => None,
        }))
    }
}
