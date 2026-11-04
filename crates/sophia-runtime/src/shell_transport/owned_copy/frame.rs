use super::*;
use crate::{ContentCandidateError as Error, ContentFrameAdmission as Admission};
use sophia_protocol::shell_files::{SHELL_FRAME_CAPABILITY, ShellFrameRecord};

impl OwnedCopyOwner {
    pub(super) fn propose_frame_commit(
        &mut self,
        transport: &mut ShellComponentTransport,
        epochs: &mut ContentEpochRegistry,
        worker: &mut IsbValidationWorker,
        now: u64,
        instant: Instant,
        other_pending: usize,
    ) -> Result<(), ShellTransportError> {
        if transport.capabilities & SHELL_FRAME_CAPABILITY == 0 {
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
                    Err(sophia_9p::Errno::EAGAIN) => return Ok(()),
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
        if let Some(Admission::Accepted {
            copy_job: Some(job),
        }) = result?
        {
            let offer = offer.expect("copy admission has an exact source");
            self.submit_reserved_copy(offer, job, epochs, worker, now)?;
        }
        transport.flush_content_frame_events(epochs, now)
    }
}
