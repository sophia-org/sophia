//! Validation-only combined credit seam; no rendered lease or retirement proof.
#![cfg(test)]

use super::*;

struct Reservation {
    job: u64,
    identity: Arc<Identity>,
}

pub(super) struct Identity {
    forwarded: AtomicBool,
}

impl Reservation {
    fn was_forwarded(&self) -> bool {
        self.identity.forwarded.load(Ordering::Acquire)
    }
}

// Only the test instantiation is concrete; transport code cannot inspect P.
pub(super) struct ProbePlan {
    id: u64,
    descriptor: OwnedFd,
    lifetime: Arc<()>,
}

pub(super) struct Route<P: Send + 'static> {
    identity: Arc<Identity>,
    plan: P,
    sender: mpsc::SyncSender<ReturnedValidation<P>>,
    owner_plan: mpsc::SyncSender<P>,
}

struct ReturnedValidation<P: Send + 'static> {
    job: u64,
    identity: Arc<Identity>,
    pixels: ValidatedIsbPixels,
    plan: P,
}

pub(super) fn take_route(mut job: Job) -> (Job, Option<Route<ProbePlan>>) {
    let route = job.combined.take();
    (job, route)
}

pub(super) fn forward<P: Send + 'static>(
    route: Option<Route<P>>,
    job: u64,
    result: Result<ValidatedIsbPixels, IsbValidationFailure>,
) -> Option<Result<ValidatedIsbPixels, IsbValidationFailure>> {
    let Some(route) = route else {
        return Some(result);
    };
    let Route {
        identity,
        plan,
        sender,
        owner_plan,
    } = route;
    let pixels = match result {
        Ok(pixels) => pixels,
        Err(error) => {
            return_plan(owner_plan, plan);
            return Some(Err(error));
        }
    };
    let returned = ReturnedValidation {
        job,
        identity: Arc::clone(&identity),
        pixels,
        plan,
    };
    match sender.try_send(returned) {
        Ok(()) => {
            // After the move: false never proves the renderer did not receive it.
            identity.forwarded.store(true, Ordering::Release);
            None
        }
        // No move occurred: preserve the real validation result on the old route.
        Err(mpsc::TrySendError::Full(returned) | mpsc::TrySendError::Disconnected(returned)) => {
            return_plan(owner_plan, returned.plan);
            Some(Ok(returned.pixels))
        }
    }
}

fn return_plan<P: Send + 'static>(owner: mpsc::SyncSender<P>, plan: P) {
    match owner.try_send(plan) {
        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
        // A private one-place slot, a sole sender, and exactly one return.
        Err(mpsc::TrySendError::Full(_)) => panic!("plan return slot written twice"),
    }
}

impl IsbValidationWorker {
    fn reserve_combined(
        &mut self,
        bytes: usize,
        deadline: Instant,
    ) -> Result<Reservation, IsbSubmitFailure> {
        let job = self.reserve(bytes, deadline)?;
        let identity = Arc::new(Identity {
            forwarded: AtomicBool::new(false),
        });
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
        plan: ProbePlan,
        sender: mpsc::SyncSender<ReturnedValidation<ProbePlan>>,
    ) -> Result<mpsc::Receiver<ProbePlan>, (IsbSubmitFailure, OwnedFd, ProbePlan)> {
        let Some(pending) = self.pending.iter_mut().flatten().find(|pending| {
            pending.id == reservation.job
                && !pending.submitted
                && pending
                    .combined
                    .as_ref()
                    .is_some_and(|identity| Arc::ptr_eq(identity, &reservation.identity))
        }) else {
            return Err((IsbSubmitFailure::Invalid, source, plan));
        };
        let (owner_plan, returned_plan) = mpsc::sync_channel(1);
        let job = Job {
            id: pending.id,
            source: Source::Isb(source),
            bytes: pending.bytes,
            deadline: pending.deadline,
            cancelled: Arc::clone(&pending.cancelled),
            combined: Some(Route {
                identity: Arc::clone(&reservation.identity),
                plan,
                sender,
                owner_plan,
            }),
        };
        match self.sender.try_send(job) {
            Ok(()) => {
                pending.submitted = true;
                Ok(returned_plan)
            }
            Err(error) => {
                let (failure, job) = match error {
                    mpsc::TrySendError::Full(job) => (IsbSubmitFailure::Capacity, job),
                    mpsc::TrySendError::Disconnected(job) => (IsbSubmitFailure::Closed, job),
                };
                let route = job.combined.expect("combined submission owns its plan");
                Err((failure, job.source.into_isb(), route.plan))
            }
        }
    }

    fn settle_combined(
        &mut self,
        reservation: Reservation,
        returned: ReturnedValidation<ProbePlan>,
    ) -> Result<(ValidatedIsbPixels, ProbePlan), (Reservation, ReturnedValidation<ProbePlan>)> {
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
        Ok((returned.pixels, returned.plan))
    }
}

#[path = "combined_frame_credit/tests.rs"]
mod tests;

#[path = "combined_frame_credit/payload_tests.rs"]
mod payload_tests;
