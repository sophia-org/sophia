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

    /// Whether `token` came back from this reservation's own submission.
    pub fn owns(&self, token: &CombinedToken) -> bool {
        self.job == token.job && Arc::ptr_eq(&self.identity, &token.identity)
    }
}

/// The renderer's bounded inbox for one output and the wake that tells it a
/// frame arrived. Only a successful move rings it. The wake is a slot because
/// the renderer attaches its command wake from its own thread, after the
/// facade that hands out inboxes may already exist.
pub struct CombinedInbox<P> {
    sender: mpsc::SyncSender<ForwardedValidation<P>>,
    ring: sophia_wake::WakeSlot,
}

impl<P> CombinedInbox<P> {
    pub fn new(
        sender: mpsc::SyncSender<ForwardedValidation<P>>,
        ring: sophia_wake::WakeSlot,
    ) -> Self {
        Self { sender, ring }
    }
}

/// A validated frame and its plan, owned by whoever holds it.
pub struct ForwardedValidation<P> {
    job: u64,
    identity: Arc<Identity>,
    facts: JobFacts,
    pixels: ValidatedIsbPixels,
    plan: P,
}

/// Returns to the owner to settle the credit; it grants nothing else. It
/// also carries the runtime's own facts about the job, which only the
/// envelope exposes.
pub struct CombinedToken {
    job: u64,
    identity: Arc<Identity>,
    facts: JobFacts,
}

/// Set by the runtime from its own job row, never from the plan.
pub(super) struct JobFacts {
    /// The validation job's one-way stop flag; the renderer only reads it.
    stop: Arc<AtomicBool>,
    /// The host-derived description admitted for this job; None for a sealed
    /// ISB submission, which has no owned-copy row.
    description: Option<sophia_protocol::ContentResourceBegin>,
}

#[cfg(test)]
impl JobFacts {
    /// For fixtures that build an envelope or route by hand.
    pub(super) fn unstopped() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            description: None,
        }
    }
}

impl<P> ForwardedValidation<P> {
    pub fn job(&self) -> u64 {
        self.job
    }

    /// Whether the owner stopped this job. False never proves it is still
    /// wanted: a stop can land just after the read.
    pub fn stopped(&self) -> bool {
        self.facts.stop.load(Ordering::Acquire)
    }

    /// The description the runtime admitted for this exact job.
    pub fn description(&self) -> Option<&sophia_protocol::ContentResourceBegin> {
        self.facts.description.as_ref()
    }

    pub fn into_parts(self) -> (CombinedToken, ValidatedIsbPixels, P) {
        let token = CombinedToken {
            job: self.job,
            identity: self.identity,
            facts: self.facts,
        };
        (token, self.pixels, self.plan)
    }

    pub fn from_parts(token: CombinedToken, pixels: ValidatedIsbPixels, plan: P) -> Self {
        Self {
            job: token.job,
            identity: token.identity,
            facts: token.facts,
            pixels,
            plan,
        }
    }
}

pub(super) struct Route<P> {
    identity: Arc<Identity>,
    facts: JobFacts,
    plan: P,
    inbox: CombinedInbox<P>,
    owner_plan: mpsc::SyncSender<P>,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) checkpoint: Option<hold::Checkpoint>,
}

pub(super) fn take_route<P>(mut job: Job<P>) -> (Job<P>, Option<Route<P>>) {
    #[cfg_attr(not(any(test, feature = "test-support")), allow(unused_mut))]
    let mut route = job.combined.take();
    #[cfg(any(test, feature = "test-support"))]
    hold::park(route.as_mut());
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
        facts,
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
        facts,
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

/// A combined job ready to send, and the receiver its plan returns on.
type PreparedJob<P> = (Job<P>, mpsc::Receiver<P>);

impl<P: Send + 'static> IsbValidationWorker<P> {
    pub fn for_combined_plans(max_jobs: usize, max_bytes: usize) -> io::Result<Self> {
        Self::spawn(
            max_jobs,
            max_bytes,
            super::super::ISB_PIXEL_VALIDATION_BYTES,
        )
    }

    /// `for_combined_plans` with the published validation chunk budget, as
    /// `with_validation_budget` takes it.
    pub fn for_combined_plans_with_budget(
        max_jobs: usize,
        max_bytes: usize,
        validation_bytes: usize,
    ) -> io::Result<Self> {
        Self::spawn(max_jobs, max_bytes, validation_bytes)
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
            .combined_job(reservation, Source::Isb(source), None, plan, inbox)
            .map_err(|(failure, source, plan)| (failure, source.into_isb(), plan))?;
        self.send_combined(job)
            .map(|()| returned_plan)
            .map_err(|(failure, source, plan)| (failure, source.into_isb(), plan))
    }

    /// `description` is the owned-copy row's admitted description; the
    /// renderer checks the plan against it.
    pub fn submit_ring_combined(
        &mut self,
        reservation: &CombinedReservation,
        source: OwnedRingRead,
        description: sophia_protocol::ContentResourceBegin,
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
            .combined_job(
                reservation,
                Source::Ring(source),
                Some(description),
                plan,
                inbox,
            )
            .map_err(|(failure, source, plan)| (failure, into_ring(source), plan))?;
        self.send_combined(job)
            .map(|()| returned_plan)
            .map_err(|(failure, source, plan)| (failure, into_ring(source), plan))
    }

    fn combined_job(
        &self,
        reservation: &CombinedReservation,
        source: Source,
        description: Option<sophia_protocol::ContentResourceBegin>,
        plan: P,
        inbox: CombinedInbox<P>,
    ) -> Result<PreparedJob<P>, (IsbSubmitFailure, Source, P)> {
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
                facts: JobFacts {
                    stop: Arc::clone(&pending.cancelled),
                    description,
                },
                plan,
                inbox,
                owner_plan,
                #[cfg(any(test, feature = "test-support"))]
                checkpoint: None,
            }),
        };
        Ok((job, returned_plan))
    }

    fn send_combined(&mut self, job: Job<P>) -> Result<(), (IsbSubmitFailure, Source, P)> {
        // Test-support (G3-5d d-2a): an armed hold rides the next combined
        // admission that carries none; a refused send drops it with the job.
        #[cfg(any(test, feature = "test-support"))]
        let job = {
            let mut job = job;
            if let Some(route) = job.combined.as_mut()
                && route.checkpoint.is_none()
            {
                route.checkpoint = self.hooks.armed.take();
            }
            job
        };
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
        if !reservation.owns(&token) {
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

/// Test-support (G3-5d d-2a): the controls' handles on the real validator.
#[cfg(any(test, feature = "test-support"))]
impl<P: Send + 'static> IsbValidationWorker<P> {
    /// The next combined admission that carries no hold of its own parks
    /// before validation until the control releases it.
    pub fn arm_next_combined_checkpoint(&mut self, checkpoint: hold::Checkpoint) {
        self.hooks.armed = Some(checkpoint);
    }

    pub fn validator_progress(&self) -> hold::ValidatorProgress {
        self.hooks.progress.clone()
    }

    pub fn validator_serial(&self) -> u64 {
        self.hooks.serial
    }
}

#[cfg(any(test, feature = "test-support"))]
#[path = "../../../tests/support/combined_frame_hold.rs"]
pub(super) mod hold;

#[cfg(test)]
#[path = "../../../tests/support/combined_frame_credit.rs"]
mod support;
