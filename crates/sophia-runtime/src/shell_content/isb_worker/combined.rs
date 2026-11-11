//! Combined frame jobs: a validated frame moves from the validator straight to
//! the renderer's one-place inbox, and the owner settles its validation credit
//! by exact identity. The validator moves the plan and never inspects it.
use super::*;
use crate::OwnedRingRead;

pub(super) type RouteSlot<P> = Option<Route<P>>;
pub(super) type PendingSlot = Option<Arc<Identity>>;

pub(super) struct Identity {
    forwarded: AtomicBool,
}

/// The owner's half of one combined validation credit.
pub struct CombinedReservation {
    job: u64,
    identity: Arc<Identity>,
}

impl CombinedReservation {
    pub fn job(&self) -> u64 {
        self.job
    }

    /// After the move: false never proves the renderer did not receive it.
    pub fn was_forwarded(&self) -> bool {
        self.identity.forwarded.load(Ordering::Acquire)
    }
}

/// The renderer's bounded inbox for one output and the wake that tells it a
/// frame arrived. Only a successful move rings it.
pub struct CombinedInbox<P> {
    sender: mpsc::SyncSender<ForwardedValidation<P>>,
    ring: sophia_wake::Notifier,
}

impl<P> CombinedInbox<P> {
    pub fn new(
        sender: mpsc::SyncSender<ForwardedValidation<P>>,
        ring: sophia_wake::Notifier,
    ) -> Self {
        Self { sender, ring }
    }
}

/// A validated frame and its plan, owned by whoever holds it.
pub struct ForwardedValidation<P> {
    job: u64,
    identity: Arc<Identity>,
    pixels: ValidatedIsbPixels,
    plan: P,
}

/// Returns to the owner to settle the credit; it grants nothing else.
pub struct CombinedToken {
    job: u64,
    identity: Arc<Identity>,
}

impl<P> ForwardedValidation<P> {
    pub fn job(&self) -> u64 {
        self.job
    }

    pub fn into_parts(self) -> (CombinedToken, ValidatedIsbPixels, P) {
        let token = CombinedToken {
            job: self.job,
            identity: self.identity,
        };
        (token, self.pixels, self.plan)
    }

    pub fn from_parts(token: CombinedToken, pixels: ValidatedIsbPixels, plan: P) -> Self {
        Self {
            job: token.job,
            identity: token.identity,
            pixels,
            plan,
        }
    }
}

pub(super) struct Route<P> {
    identity: Arc<Identity>,
    plan: P,
    inbox: CombinedInbox<P>,
    owner_plan: mpsc::SyncSender<P>,
    #[cfg(test)]
    checkpoint: Option<support::Checkpoint>,
}

pub(super) fn take_route<P>(mut job: Job<P>) -> (Job<P>, Option<Route<P>>) {
    #[cfg_attr(not(test), allow(unused_mut))]
    let mut route = job.combined.take();
    #[cfg(test)]
    support::park(route.as_mut());
    (job, route)
}

pub(super) fn forward<P>(
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
        inbox,
        owner_plan,
        ..
    } = route;
    let pixels = match result {
        Ok(pixels) => pixels,
        Err(error) => {
            return_plan(owner_plan, plan);
            return Some(Err(error));
        }
    };
    let forwarded = ForwardedValidation {
        job,
        identity: Arc::clone(&identity),
        pixels,
        plan,
    };
    match inbox.sender.try_send(forwarded) {
        Ok(()) => {
            identity.forwarded.store(true, Ordering::Release);
            inbox.ring.notify();
            None
        }
        // No move occurred: preserve the real validation result on the old route.
        Err(mpsc::TrySendError::Full(returned) | mpsc::TrySendError::Disconnected(returned)) => {
            return_plan(owner_plan, returned.plan);
            Some(Ok(returned.pixels))
        }
    }
}

fn return_plan<P>(owner: mpsc::SyncSender<P>, plan: P) {
    match owner.try_send(plan) {
        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
        // A private one-place slot, a sole sender, and exactly one return.
        Err(mpsc::TrySendError::Full(_)) => panic!("plan return slot written twice"),
    }
}

impl<P: Send + 'static> IsbValidationWorker<P> {
    pub fn for_combined_plans(max_jobs: usize, max_bytes: usize) -> io::Result<Self> {
        Self::spawn(
            max_jobs,
            max_bytes,
            super::super::ISB_PIXEL_VALIDATION_BYTES,
        )
    }

    pub fn reserve_combined(
        &mut self,
        bytes: usize,
        deadline: Instant,
    ) -> Result<CombinedReservation, IsbSubmitFailure> {
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
        Ok(CombinedReservation { job, identity })
    }

    /// On success the receiver returns the plan if validation fails or the
    /// inbox refuses the move. On refusal the caller gets both back.
    pub fn submit_combined(
        &mut self,
        reservation: &CombinedReservation,
        source: OwnedFd,
        plan: P,
        inbox: CombinedInbox<P>,
    ) -> Result<mpsc::Receiver<P>, (IsbSubmitFailure, OwnedFd, P)> {
        let (job, returned_plan) = self
            .combined_job(reservation, Source::Isb(source), plan, inbox)
            .map_err(|(failure, source, plan)| (failure, source.into_isb(), plan))?;
        self.send_combined(job)
            .map(|()| returned_plan)
            .map_err(|(failure, source, plan)| (failure, source.into_isb(), plan))
    }

    pub fn submit_ring_combined(
        &mut self,
        reservation: &CombinedReservation,
        source: OwnedRingRead,
        plan: P,
        inbox: CombinedInbox<P>,
    ) -> Result<mpsc::Receiver<P>, (IsbSubmitFailure, OwnedRingRead, P)> {
        let into_ring = |source| match source {
            Source::Ring(read) => read,
            _ => unreachable!("ring submission returns its own source"),
        };
        if reservation_bytes(self, reservation) != Some(source.pixel_bytes()) {
            return Err((IsbSubmitFailure::Invalid, source, plan));
        }
        let (job, returned_plan) = self
            .combined_job(reservation, Source::Ring(source), plan, inbox)
            .map_err(|(failure, source, plan)| (failure, into_ring(source), plan))?;
        self.send_combined(job)
            .map(|()| returned_plan)
            .map_err(|(failure, source, plan)| (failure, into_ring(source), plan))
    }

    fn combined_job(
        &self,
        reservation: &CombinedReservation,
        source: Source,
        plan: P,
        inbox: CombinedInbox<P>,
    ) -> Result<(Job<P>, mpsc::Receiver<P>), (IsbSubmitFailure, Source, P)> {
        let Some(pending) = self.pending.iter().flatten().find(|pending| {
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
            source,
            bytes: pending.bytes,
            deadline: pending.deadline,
            cancelled: Arc::clone(&pending.cancelled),
            combined: Some(Route {
                identity: Arc::clone(&reservation.identity),
                plan,
                inbox,
                owner_plan,
                #[cfg(test)]
                checkpoint: None,
            }),
        };
        Ok((job, returned_plan))
    }

    fn send_combined(&mut self, job: Job<P>) -> Result<(), (IsbSubmitFailure, Source, P)> {
        let id = job.id;
        match self.sender.try_send(job) {
            Ok(()) => {
                self.pending
                    .iter_mut()
                    .flatten()
                    .find(|pending| pending.id == id)
                    .expect("combined_job matched this pending row")
                    .submitted = true;
                Ok(())
            }
            Err(error) => {
                let (failure, job) = match error {
                    mpsc::TrySendError::Full(job) => (IsbSubmitFailure::Capacity, job),
                    mpsc::TrySendError::Disconnected(job) => (IsbSubmitFailure::Closed, job),
                };
                let route = job.combined.expect("combined submission owns its plan");
                Err((failure, job.source, route.plan))
            }
        }
    }

    /// Clears the credit only for the exact live row this token names.
    pub fn settle_combined_token(
        &mut self,
        reservation: CombinedReservation,
        token: CombinedToken,
    ) -> Result<(), (CombinedReservation, CombinedToken)> {
        if reservation.job != token.job || !Arc::ptr_eq(&reservation.identity, &token.identity) {
            return Err((reservation, token));
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
            return Err((reservation, token));
        };
        *slot = None;
        Ok(())
    }

    pub fn settle_combined(
        &mut self,
        reservation: CombinedReservation,
        returned: ForwardedValidation<P>,
    ) -> Result<(ValidatedIsbPixels, P), (CombinedReservation, ForwardedValidation<P>)> {
        let (token, pixels, plan) = returned.into_parts();
        match self.settle_combined_token(reservation, token) {
            Ok(()) => Ok((pixels, plan)),
            Err((reservation, token)) => Err((
                reservation,
                ForwardedValidation::from_parts(token, pixels, plan),
            )),
        }
    }
}

fn reservation_bytes<P: Send + 'static>(
    worker: &IsbValidationWorker<P>,
    reservation: &CombinedReservation,
) -> Option<usize> {
    worker
        .pending
        .iter()
        .flatten()
        .find(|pending| pending.id == reservation.job)
        .map(|pending| pending.bytes)
}

#[cfg(test)]
#[path = "../../../tests/support/combined_frame_credit.rs"]
mod support;
