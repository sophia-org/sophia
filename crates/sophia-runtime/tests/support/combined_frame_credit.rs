//! Validation-only combined credit seam; no rendered lease or retirement proof.
#![cfg(test)]

use super::*;

struct Reservation {
    job: u64,
    identity: Arc<()>,
}

pub(super) struct Route {
    identity: Arc<()>,
    sender: mpsc::SyncSender<ReturnedValidation>,
}

struct ReturnedValidation {
    job: u64,
    identity: Arc<()>,
    pixels: ValidatedIsbPixels,
}

pub(super) fn take_route(mut job: Job) -> (Job, Option<Route>) {
    let route = job.combined.take();
    (job, route)
}

pub(super) fn forward(
    route: Option<Route>,
    job: u64,
    result: Result<ValidatedIsbPixels, IsbValidationFailure>,
) -> Option<Result<ValidatedIsbPixels, IsbValidationFailure>> {
    let Some(route) = route else {
        return Some(result);
    };
    let pixels = match result {
        Ok(pixels) => pixels,
        Err(error) => return Some(Err(error)),
    };
    let returned = ReturnedValidation {
        job,
        identity: route.identity,
        pixels,
    };
    match route.sender.try_send(returned) {
        Ok(()) => None,
        // No move occurred: preserve the real validation result on the old route.
        Err(mpsc::TrySendError::Full(returned) | mpsc::TrySendError::Disconnected(returned)) => {
            Some(Ok(returned.pixels))
        }
    }
}

impl IsbValidationWorker {
    fn reserve_combined(
        &mut self,
        bytes: usize,
        deadline: Instant,
    ) -> Result<Reservation, IsbSubmitFailure> {
        let job = self.reserve(bytes, deadline)?;
        let identity = Arc::new(());
        self.pending
            .iter_mut()
            .flatten()
            .find(|pending| pending.id == job)
            .expect("reserve created this pending row")
            .combined = Some(Arc::clone(&identity));
        Ok(Reservation { job, identity })
    }

    fn submit_combined(
        &mut self,
        reservation: &Reservation,
        source: OwnedFd,
        sender: mpsc::SyncSender<ReturnedValidation>,
    ) -> Result<(), (IsbSubmitFailure, OwnedFd)> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.id == reservation.job
                && !pending.submitted
                && pending
                    .combined
                    .as_ref()
                    .is_some_and(|identity| Arc::ptr_eq(identity, &reservation.identity))
        }) else {
            return Err((IsbSubmitFailure::Invalid, source));
        };
        let job = Job {
            id: pending.id,
            source: Source::Isb(source),
            bytes: pending.bytes,
            deadline: pending.deadline,
            cancelled: Arc::clone(&pending.cancelled),
            combined: Some(Route {
                identity: Arc::clone(&reservation.identity),
                sender,
            }),
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
                Err((failure, job.source.into_isb()))
            }
        }
    }

    fn settle_combined(
        &mut self,
        reservation: Reservation,
        returned: ReturnedValidation,
    ) -> Result<ValidatedIsbPixels, (Reservation, ReturnedValidation)> {
        if reservation.job != returned.job
            || !Arc::ptr_eq(&reservation.identity, &returned.identity)
        {
            return Err((reservation, returned));
        }
        let Some(slot) = self.pending.iter_mut().find(|slot| {
            slot.as_ref().is_some_and(|pending| {
                pending.id == reservation.job
                    && pending.submitted
                    && pending
                        .combined
                        .as_ref()
                        .is_some_and(|identity| Arc::ptr_eq(identity, &reservation.identity))
            })
        }) else {
            return Err((reservation, returned));
        };
        *slot = None;
        Ok(returned.pixels)
    }
}

#[path = "combined_frame_credit/tests.rs"]
mod tests;
