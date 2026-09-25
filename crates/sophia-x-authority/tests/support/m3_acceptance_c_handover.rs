// The ordered handover and its observers: armed handovers, held capsules,
// retained releases, queue handovers and the adjudication witnesses.
// Included from m3_acceptance.rs beside m3_acceptance_c.rs (t026).

/// Arm the next handover of this delivery on this origin, once.
fn arm_handover(
    registry: &XServerFrontendRouteRegistry,
    delivery: XAuthorityInputDeliveryId,
    seam: HandoverSeam,
) {
    *HANDOVER_WITNESS.lock().unwrap() = None;
    HANDOVER_SEAMS.lock().unwrap().push((
        Arc::as_ptr(&registry.clients) as usize,
        Some(delivery),
        seam,
    ));
}

/// Production's entry into that seam, immediately after the handover returns
/// and before anything is written down about it. Empty unless a case armed
/// this exact origin and this exact delivery.
/// What the release was, read at the handover and before the interruption.
///
/// THE WITNESS COMES FROM THE SEAM. Reading these afterwards compares one
/// post-unwind reading with another and cannot say they are the release that
/// was handed over; taken here, they are what it was at the moment nobody can
/// describe afterwards.
#[derive(Clone)]
struct HandoverWitness {
    delivery: Option<XAuthorityInputDeliveryId>,
    incarnation: sophia_input_authority::HoldIncarnation,
    attempt: Option<sophia_input_authority::AttemptToken>,
    completion: Option<Arc<PrivateDeliveryCompletion>>,
    /// The recipient the release named at the handover, taken whole from the
    /// source obligation it was still answering for.
    endpoint: Option<PrivateEndpointIdentity>,
    reached_client: XServerFrontendClientId,
    reached_window: XResourceId,
    /// Whether the handover itself returned success.
    ///
    /// The seam is after either answer, and the arrangement this case needs is
    /// the one where the capsule was admitted and only the bookkeeping about
    /// it was lost. A full or disconnected queue is a different state, and
    /// one the record can describe.
    handed_over: bool,
}

static HANDOVER_WITNESS: Mutex<Option<HandoverWitness>> = Mutex::new(None);

fn handover_witness() -> Option<HandoverWitness> {
    HANDOVER_WITNESS.lock().unwrap().clone()
}

/// What production hands the seam: one borrowed view of the release and the
/// capsule it just offered, read where both are still true.
pub(crate) struct OrderedHandover<'a> {
    pub(crate) delivery: Option<XAuthorityInputDeliveryId>,
    pub(crate) incarnation: sophia_input_authority::HoldIncarnation,
    pub(crate) attempt: Option<sophia_input_authority::AttemptToken>,
    pub(crate) completion: Option<&'a Arc<PrivateDeliveryCompletion>>,
    /// The recipient the CAPSULE named, which is the one the row has to check
    /// the release against. Taking both sides from the release would compare a
    /// reading with itself and miss a capsule owed to another endpoint.
    pub(crate) endpoint: &'a PrivateEndpointIdentity,
    pub(crate) reached: PrivateReachedResources,
    /// Whether the handover itself returned success.
    pub(crate) handed_over: bool,
}

pub(crate) fn after_ordered_handover(
    registry: &XServerFrontendRouteRegistry,
    handover: &OrderedHandover<'_>,
) {
    let OrderedHandover {
        delivery,
        incarnation,
        attempt,
        completion,
        endpoint,
        reached,
        handed_over,
    } = *handover;
    let origin = Arc::as_ptr(&registry.clients) as usize;
    let seam = {
        let mut seams = HANDOVER_SEAMS.lock().unwrap();
        seams
            .iter()
            .position(|(candidate, wanted, _)| *candidate == origin && *wanted == delivery)
            .map(|at| seams.remove(at).2)
    };
    if let Some(seam) = seam {
        // Written down before the seam runs, because the seam is what makes
        // this moment undescribable afterwards.
        *HANDOVER_WITNESS.lock().unwrap() = Some(HandoverWitness {
            delivery,
            incarnation,
            attempt,
            completion: completion.cloned(),
            endpoint: Some(endpoint.clone()),
            reached_client: reached.client(),
            reached_window: reached.window(),
            handed_over,
        });
        seam();
    }
}

/// Finish one invocation, naming it.
///
/// `LifecycleService::finish` asserts on join evidence a stopped and
/// collected invocation already has; its precondition is actual service exit.
/// Three invocations sharing that helper produced one unlabelled assertion,
/// and it was read as belonging to the wrong one. This says which.
fn finish_labelled(
    what: &str,
    service: LifecycleService,
    custodies: &[Arc<PrivateEvidenceCustody>],
) -> Vec<String> {
    for custody in custodies {
        if custody.ever_started() {
            assert_eq!(
                custody.join().phase(),
                PrivateReapingPhase::Joined,
                "{what}: this invocation must have exited and been collected before it is finished"
            );
        }
    }
    service.finish(custodies)
}

/// What the connection's own ordered home is actually holding.
///
/// READ FROM THE HOME, NOT FROM A SUMMARY. A retained-release list is empty
/// for an axis transient, which is not a held release in terminal dispatch,
/// so comparing that list either side of a visit compares nothing. This reads
/// the home's own standing and the capsule its serving owner still has, with
/// the frame bytes and the progress through them.
#[derive(Clone)]
struct HeldCapsule {
    standing: PrivateHomeStanding,
    serving: bool,
    delivery: Option<XAuthorityInputDeliveryId>,
    frames_owed: usize,
    frame_index: usize,
    frame_bytes: Option<Vec<u8>>,
    sent: Option<usize>,
    blocked_micros: u128,
    /// The finalizer this capsule carried from the debt that owns it, kept as
    /// the original handle so a later reading is compared as the same one.
    finalizer: Option<Arc<PrivateDeliveryFinalizer>>,
    /// The completion that finalizer answers through, and the answer written
    /// into it.
    ///
    /// THE HANDLE ALONE SAYS NOTHING ABOUT THE ANSWER. A finalizer compares
    /// equal to itself across a publication, so a reading that only held the
    /// finalizer would call a capsule unchanged while its delivery had just
    /// been answered underneath it. The cell it guards is kept by handle and
    /// the answer in that cell by value, with the delivery and recipient the
    /// finalizer was minted for.
    finalizer_completion: Option<Arc<PrivateDeliveryCompletion>>,
    finalizer_answer: Option<XAuthorityClientInputDelivery>,
    finalizer_delivery: Option<XAuthorityInputDeliveryId>,
    finalizer_client: Option<XServerFrontendClientId>,
    /// The recipient this capsule names, kept whole. A registration pointer
    /// and the identity over it, not a client number that another origin can
    /// hold at the same time.
    endpoint: Option<PrivateEndpointIdentity>,
    answers_for_this_origin: bool,
    in_flight: bool,
    /// What this connection's teardown established, kept beside the serving
    /// owner rather than inferred from it. A serving owner answers for what it
    /// holds and says nothing about whether the wire was closed or what became
    /// of the worker that was to serve it.
    fence: Option<PrivateHandoverFence>,
    /// The join this home's worker was collected through, as the original
    /// handle, so it can be compared against the custody that owns it.
    worker_join: Option<Arc<PrivateJoinEvidence>>,
    worker_never_started: bool,
    source_poisoned: bool,
}

impl std::fmt::Debug for HeldCapsule {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("HeldCapsule")
            .field("standing", &self.standing)
            .field("serving", &self.serving)
            .field("delivery", &self.delivery)
            .field("frames_owed", &self.frames_owed)
            .field("frame_index", &self.frame_index)
            .field("frame_len", &self.frame_bytes.as_ref().map(Vec::len))
            .field("sent", &self.sent)
            .field("blocked_micros", &self.blocked_micros)
            .field("has_finalizer", &self.finalizer.is_some())
            .field("finalizer_delivery", &self.finalizer_delivery)
            .field("finalizer_client", &self.finalizer_client)
            .field("finalizer_answer", &self.finalizer_answer)
            .field("has_endpoint", &self.endpoint.is_some())
            .field("answers_for_this_origin", &self.answers_for_this_origin)
            .field("in_flight", &self.in_flight)
            .field("fence", &self.fence)
            .field("worker_joined", &self.worker_join.is_some())
            .field("worker_never_started", &self.worker_never_started)
            .field("source_poisoned", &self.source_poisoned)
            .finish()
    }
}

impl HeldCapsule {
    /// Whether two readings are of the same capsule in the same state: the
    /// finalizer by handle, the recipient by its whole identity, the rest by
    /// value including the exact frame bytes still in hand.
    fn same_as(&self, other: &Self) -> bool {
        self.standing == other.standing
            && self.serving == other.serving
            && self.delivery == other.delivery
            && self.frames_owed == other.frames_owed
            && self.frame_index == other.frame_index
            && self.frame_bytes == other.frame_bytes
            && self.sent == other.sent
            && self.blocked_micros == other.blocked_micros
            && self.answers_for_this_origin == other.answers_for_this_origin
            && self.in_flight == other.in_flight
            && self.fence == other.fence
            && self.worker_never_started == other.worker_never_started
            && self.source_poisoned == other.source_poisoned
            && self.finalizer_answer == other.finalizer_answer
            && self.finalizer_delivery == other.finalizer_delivery
            && self.finalizer_client == other.finalizer_client
            && match (&self.finalizer, &other.finalizer) {
                (Some(ours), Some(theirs)) => Arc::ptr_eq(ours, theirs),
                (None, None) => true,
                _ => false,
            }
            && match (&self.finalizer_completion, &other.finalizer_completion) {
                (Some(ours), Some(theirs)) => Arc::ptr_eq(ours, theirs),
                (None, None) => true,
                _ => false,
            }
            && match (&self.worker_join, &other.worker_join) {
                (Some(ours), Some(theirs)) => Arc::ptr_eq(ours, theirs),
                (None, None) => true,
                _ => false,
            }
            && match (&self.endpoint, &other.endpoint) {
                (Some(ours), Some(theirs)) => ours.matches(theirs),
                (None, None) => true,
                _ => false,
            }
    }

    /// Whether this capsule's recipient is exactly that registration.
    fn serves_registration(
        &self,
        witness: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    ) -> bool {
        self.endpoint
            .as_ref()
            .is_some_and(|endpoint| endpoint.is_registration(witness))
    }

    /// Whether the worker that served this home was collected through exactly
    /// that custody's join.
    fn joined_through(&self, witness: &Arc<PrivateJoinEvidence>) -> bool {
        self.worker_join
            .as_ref()
            .is_some_and(|joined| Arc::ptr_eq(joined, witness))
    }
}

/// Read this connection's home. `None` when it holds no serving owner at all.
fn held_capsule(
    home: &Arc<PrivateOrderedHome>,
    registry: &XServerFrontendRouteRegistry,
) -> Option<HeldCapsule> {
    let held = home
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let standing = held.standing;
    let PrivateOrderedContinuation::Serving { owner, evidence } = held.payload.as_ref()? else {
        return Some(HeldCapsule {
            standing,
            serving: false,
            delivery: None,
            frames_owed: 0,
            frame_index: 0,
            frame_bytes: None,
            sent: None,
            blocked_micros: 0,
            finalizer: None,
            finalizer_completion: None,
            finalizer_answer: None,
            finalizer_delivery: None,
            finalizer_client: None,
            endpoint: None,
            answers_for_this_origin: false,
            in_flight: false,
            fence: None,
            worker_join: None,
            worker_never_started: false,
            source_poisoned: false,
        });
    };
    let (capsule, in_flight) = match owner.in_flight() {
        Some(capsule) => (capsule, true),
        None => (owner.retained_unanswered().first()?, false),
    };
    let frame = capsule.send.frame.as_ref();
    Some(HeldCapsule {
        standing,
        serving: true,
        delivery: capsule.delivery().emission().delivery(),
        frames_owed: capsule.delivery().emission().frame_count(),
        frame_index: capsule.frame_index(),
        frame_bytes: frame.map(|frame| frame.bytes.as_ref().to_vec()),
        sent: frame.and_then(|frame| match frame.progress {
            X11OrderedSendProgress::Sent(offset) => Some(offset),
            X11OrderedSendProgress::Unknown { .. } => None,
        }),
        blocked_micros: capsule.send.blocked.as_micros(),
        finalizer: capsule.delivery().finalizer().cloned(),
        finalizer_completion: capsule
            .delivery()
            .finalizer()
            .map(|finalizer| Arc::clone(&finalizer.completion)),
        finalizer_answer: capsule
            .delivery()
            .finalizer()
            .and_then(|finalizer| finalizer.completion.answer()),
        finalizer_delivery: capsule.delivery().finalizer().and_then(|held| held.delivery),
        finalizer_client: capsule.delivery().finalizer().map(|held| held.client),
        endpoint: Some(capsule.delivery().endpoint().clone()),
        answers_for_this_origin: capsule.delivery().emission().answers_for(registry),
        in_flight,
        fence: evidence.fence,
        worker_join: match &evidence.worker {
            PrivateOrderedWorkerExit::Joined(joined) => joined.upgrade(),
            PrivateOrderedWorkerExit::NeverStarted => None,
        },
        worker_never_started: evidence.worker == PrivateOrderedWorkerExit::NeverStarted,
        source_poisoned: evidence.source_poisoned,
    })
}

/// One retained release, named by what identifies it rather than summarised.
///
/// EQUAL SUMMARIES ARE NOT THE SAME RECORD. A phase, a pending flag and an
/// answered flag compare equal across a replacement, and across an attempted
/// resend that was afterwards put back. The delivery, the incarnation, the
/// attempt token, the recipient it reached and the address of the completion
/// it was admitted with do not.
#[derive(Clone)]
struct RetainedRelease {
    dispatch: PrivateDispatchPhase,
    delivery: Option<XAuthorityInputDeliveryId>,
    incarnation: sophia_input_authority::HoldIncarnation,
    attempt: Option<sophia_input_authority::AttemptToken>,
    /// The completion this release has carried since its debt was recorded,
    /// kept as the original handle. Holding it is holding that exact
    /// admission's completion; an address could be reused by anything.
    completion: Option<Arc<PrivateDeliveryCompletion>>,
    /// The recipient the source obligation this release still answers for
    /// names, kept whole. Compared against the endpoint the handover itself
    /// used, which is the capsule's own and not this one.
    endpoint: Option<PrivateEndpointIdentity>,
    pending_capsule: bool,
    answered: bool,
    reached_client: XServerFrontendClientId,
    reached_window: XResourceId,
}

impl std::fmt::Debug for RetainedRelease {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("RetainedRelease")
            .field("dispatch", &self.dispatch)
            .field("delivery", &self.delivery)
            .field("incarnation", &self.incarnation)
            .field("attempt", &self.attempt)
            .field("has_completion", &self.completion.is_some())
            .field("has_endpoint", &self.endpoint.is_some())
            .field("pending_capsule", &self.pending_capsule)
            .field("answered", &self.answered)
            .field("reached_client", &self.reached_client)
            .field("reached_window", &self.reached_window)
            .finish()
    }
}

impl RetainedRelease {
    /// Whether the recipient this release's source obligation names is exactly
    /// that one.
    fn names_endpoint(&self, other: &PrivateEndpointIdentity) -> bool {
        self.endpoint
            .as_ref()
            .is_some_and(|ours| ours.matches(other))
    }

    /// Whether that recipient is exactly this registration.
    fn serves_registration(
        &self,
        witness: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    ) -> bool {
        self.endpoint
            .as_ref()
            .is_some_and(|endpoint| endpoint.is_registration(witness))
    }

    /// Whether two readings are of the same release, compared by what
    /// identifies it: its completion by handle, everything else by value.
    fn same_as(&self, other: &Self) -> bool {
        self.dispatch == other.dispatch
            && self.delivery == other.delivery
            && self.incarnation == other.incarnation
            && self.attempt == other.attempt
            && self.pending_capsule == other.pending_capsule
            && self.answered == other.answered
            && self.reached_client == other.reached_client
            && self.reached_window == other.reached_window
            && match (&self.endpoint, &other.endpoint) {
                (Some(ours), Some(theirs)) => ours.matches(theirs),
                (None, None) => true,
                _ => false,
            }
            && match (&self.completion, &other.completion) {
                (Some(ours), Some(theirs)) => Arc::ptr_eq(ours, theirs),
                (None, None) => true,
                _ => false,
            }
    }

    /// Whether this release carries that exact completion.
    fn carries(&self, cell: &Arc<PrivateDeliveryCompletion>) -> bool {
        self.completion
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, cell))
    }
}

/// Every release this invocation's own origin still holds in the store.
fn retained_dispatch(service: &LifecycleService) -> Vec<RetainedRelease> {
    let held = service.owner.store.inner.lock().expect("a readable store");
    let seen = held
        .terminal
        .iter()
        .filter(|inventory| Arc::ptr_eq(&inventory.origin.clients, &service.registry.clients))
        .flat_map(|inventory| {
            inventory.settling.iter().map(|release| RetainedRelease {
                dispatch: release.custody.dispatch,
                delivery: release.delivery(),
                incarnation: release.incarnation(),
                attempt: release.attempt(),
                completion: release.completion().cloned(),
                endpoint: release.native().map(PrivateNativeHold::endpoint).cloned(),
                pending_capsule: release.custody.pending.is_some(),
                answered: release
                    .completion()
                    .is_some_and(|cell| cell.answer().is_some()),
                reached_client: release.reached().client(),
                reached_window: release.reached().window(),
            })
        })
        .collect();
    drop(held);
    seen
}

/// One step of the actual writer, as it happened, for the delivery it was
/// serving.
///
/// WHOLE FRAMES, NOT BYTES. `advanced` names a frame this delivery owed that
/// went out entire. The writer's own byte offset within a frame is not
/// reported here, so nothing built from these may claim a partial-byte
/// prefix; what they establish is how much of one capsule was committed.
#[derive(Clone, Debug)]
struct ObservedFrame {
    delivery: Option<XAuthorityInputDeliveryId>,
    frames: usize,
    /// The frame this visit tried, read before it tried it.
    attempted: Option<usize>,
    index: usize,
    advanced: Option<usize>,
    failure: Option<String>,
}

/// One attempt to put a frame on the wire, recorded before the syscall.
///
/// THE ATTEMPT, NOT ITS RESULT. A socket that refuses everything answers a
/// replay of a committed frame exactly as it answers never having tried, and
/// a replay that afterwards restored every summary field would leave the
/// returns identical. This is the only record that separates them.
#[derive(Clone, Debug)]
struct SendEntry {
    delivery: Option<XAuthorityInputDeliveryId>,
    frames: usize,
    frame_index: usize,
    sent_before: Option<usize>,
    frame_len: usize,
}

/// One handover of a capsule into its recipient's queue, and what the queue
/// answered. Counted apart from frame send attempts: they are different facts.
#[derive(Clone, Debug)]
struct QueueHandover {
    delivery: XAuthorityInputDeliveryId,
    accepted: bool,
}

/// One actual visit to the watched pending transient, recorded after its own
/// cell was read.
///
/// ONE CELL, EXPLICITLY ARMED. Recording every cell whenever any origin is
/// watched would let unrelated traffic in a parallel suite fill the bound and
/// crowd out the very visits a case is counting.
#[derive(Clone, Copy, Debug)]
struct TransientVisit {
    dispatch: PrivateDispatchPhase,
    answer_seen: bool,
}

static TRANSIENT_VISITS: Mutex<Vec<TransientVisit>> = Mutex::new(Vec::new());
/// The one completion whose visits are recorded, held as the original Arc.
static WATCHED_COMPLETION: Mutex<Option<Arc<PrivateDeliveryCompletion>>> = Mutex::new(None);
static QUEUE_HANDOVERS: Mutex<Vec<QueueHandover>> = Mutex::new(Vec::new());
static SEND_ENTRIES: Mutex<Vec<SendEntry>> = Mutex::new(Vec::new());

/// Set when any recorder had to drop a record for want of room.
///
/// A RECORDER THAT SILENTLY STOPS IS WORSE THAN NO RECORDER. A missing tail
/// reads exactly like a retry that never happened, so overflow is a fact the
/// control has to fail on rather than a limit it can ignore.
static OBSERVER_OVERFLOWED: AtomicBool = AtomicBool::new(false);

/// Serialises the controls that own the process-wide observers.
///
/// The observer names one origin at a time, which is right for a case and
/// wrong for two at once. The M3 runner takes acceptance cases one at a time,
/// but an ordinary full-suite run does not, so the controls that arm these
/// take this first. Nothing else in the suite waits on it, and no production
/// lock is involved.
static OBSERVER_OWNER: Mutex<()> = Mutex::new(());

fn own_the_observer() -> std::sync::MutexGuard<'static, ()> {
    OBSERVER_OWNER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether this emission belongs to the watched origin, answered before its
/// capsule is moved into the queue.
pub(crate) fn queue_handover_subject(
    emission: &PrivateOrderedEmission,
) -> Option<XAuthorityInputDeliveryId> {
    let watched = WATCHED_ORIGIN.lock().unwrap().clone();
    let watched = watched?;
    if !emission.answers_for(&watched) {
        return None;
    }
    emission.delivery()
}

/// One labelled acceptance fault at the moment a held offer is adjudicated.
///
/// ARMED AGAINST ONE EXACT COMPLETION AND ONE OFFERED OUTCOME, once. It exists
/// because a decided request whose admission is gone by the time its refusal is
/// offered has no other deterministic arrangement: the execution path claims
/// that same admission before it runs, so removing it any earlier stops the
/// source outcome from ever being produced and leaves nothing to refuse the
/// publication of.
type AdjudicationFault = Box<dyn FnOnce() + Send>;

struct ArmedAdjudicationFault {
    completion: Arc<PrivateDeliveryCompletion>,
    delivery: XAuthorityInputDeliveryId,
    outcome: XAuthorityInputDeliveryOutcome,
    fault: AdjudicationFault,
}

static ADJUDICATION_FAULT: Mutex<Option<ArmedAdjudicationFault>> = Mutex::new(None);

fn arm_adjudication_fault(
    completion: &Arc<PrivateDeliveryCompletion>,
    delivery: XAuthorityInputDeliveryId,
    outcome: XAuthorityInputDeliveryOutcome,
    fault: AdjudicationFault,
) {
    *ADJUDICATION_FAULT.lock().unwrap() = Some(ArmedAdjudicationFault {
        completion: Arc::clone(completion),
        delivery,
        outcome,
        fault,
    });
}

/// Production's entry into that fault, before it takes any lock of its own.
/// Empty for every adjudication but the one a case armed.
pub(crate) fn before_adjudication(
    completion: &Arc<PrivateDeliveryCompletion>,
    delivery: XAuthorityInputDeliveryId,
    outcome: XAuthorityInputDeliveryOutcome,
) {
    // THE FAULT IS TAKEN OUT OF ITS OWN LOCK AND THAT LOCK IS RELEASED BEFORE
    // IT RUNS. What it does takes the recovery ledger, and holding an
    // unrelated mutex across that would order two locks nothing else orders.
    let armed = {
        let mut held = ADJUDICATION_FAULT.lock().unwrap();
        let matched = held.as_ref().is_some_and(|armed| {
            Arc::ptr_eq(&armed.completion, completion)
                && armed.delivery == delivery
                && armed.outcome == outcome
        });
        matched.then(|| held.take().expect("just matched").fault)
    };
    if let Some(fault) = armed {
        fault();
    }
}

/// One adjudication of an offer against its own admission, recorded with the
/// answer the deciding branch gave it.
#[derive(Clone, Copy, Debug)]
struct AdjudicationSeen {
    delivery: XAuthorityInputDeliveryId,
    answer: PrivateAdjudication,
}

static ADJUDICATIONS: Mutex<Vec<AdjudicationSeen>> = Mutex::new(Vec::new());

/// Production's entry into the adjudication recording, filtered to the one
/// completion a case has armed.
pub(crate) fn observed_adjudication(
    completion: &Arc<PrivateDeliveryCompletion>,
    delivery: XAuthorityInputDeliveryId,
    answer: PrivateAdjudication,
) {
    let watched = WATCHED_COMPLETION.lock().unwrap().clone();
    let Some(watched) = watched else {
        return;
    };
    if !Arc::ptr_eq(&watched, completion) {
        return;
    }
    let mut seen = ADJUDICATIONS.lock().unwrap();
    if seen.len() >= 4096 {
        OBSERVER_OVERFLOWED.store(true, Ordering::Release);
        return;
    }
    seen.push(AdjudicationSeen { delivery, answer });
}

/// The adjudications recorded so far, read without disarming.
fn adjudications_snapshot() -> Vec<AdjudicationSeen> {
    ADJUDICATIONS.lock().unwrap().clone()
}

/// Production's entry into the transient-visit recording.
pub(crate) fn observed_transient_visit(
    completion: Option<&Arc<PrivateDeliveryCompletion>>,
    dispatch: PrivateDispatchPhase,
    answer_seen: bool,
) {
    let watched = WATCHED_COMPLETION.lock().unwrap().clone();
    let (Some(watched), Some(completion)) = (watched, completion) else {
        return;
    };
    if !Arc::ptr_eq(&watched, completion) {
        return;
    }
    let mut seen = TRANSIENT_VISITS.lock().unwrap();
    if seen.len() >= 4096 {
        OBSERVER_OVERFLOWED.store(true, Ordering::Release);
        return;
    }
    seen.push(TransientVisit {
        dispatch,
        answer_seen,
    });
}

/// Record visits to this exact completion and no other.
fn watch_completion(cell: &Arc<PrivateDeliveryCompletion>) {
    TRANSIENT_VISITS.lock().unwrap().clear();
    ADJUDICATIONS.lock().unwrap().clear();
    *WATCHED_COMPLETION.lock().unwrap() = Some(Arc::clone(cell));
}

fn stop_watching_completion() {
    *WATCHED_COMPLETION.lock().unwrap() = None;
}

/// Production's entry into the queue-handover recording.
pub(crate) fn observed_queue_handover(subject: Option<XAuthorityInputDeliveryId>, accepted: bool) {
    let Some(delivery) = subject else {
        return;
    };
    let mut seen = QUEUE_HANDOVERS.lock().unwrap();
    if seen.len() >= 4096 {
        OBSERVER_OVERFLOWED.store(true, Ordering::Release);
        return;
    }
    seen.push(QueueHandover { delivery, accepted });
}
static OBSERVED_FRAMES: Mutex<Vec<ObservedFrame>> = Mutex::new(Vec::new());
/// The one invocation whose writer is being watched.
///
/// EXACT ORIGIN IDENTITY, not a socket and not a client number. A descriptor
/// is reused as connections come and go, and the acceptance binary runs cases
/// beside each other; recording every service while armed would let one
/// case's steps be read as another's.
static WATCHED_ORIGIN: Mutex<Option<XServerFrontendRouteRegistry>> = Mutex::new(None);

/// Start recording one invocation's writer progress. Bounded, and cleared
/// here so a case never reads another case's steps.
fn observe_frames(registry: &XServerFrontendRouteRegistry) {
    OBSERVED_FRAMES.lock().unwrap().clear();
    SEND_ENTRIES.lock().unwrap().clear();
    QUEUE_HANDOVERS.lock().unwrap().clear();
    TRANSIENT_VISITS.lock().unwrap().clear();
    *WATCHED_COMPLETION.lock().unwrap() = None;
    OBSERVER_OVERFLOWED.store(false, Ordering::Release);
    *WATCHED_ORIGIN.lock().unwrap() = Some(registry.clone());
}

/// The transient visits recorded so far, read without disarming.
fn transient_visits_snapshot() -> Vec<TransientVisit> {
    TRANSIENT_VISITS.lock().unwrap().clone()
}

/// The queue handovers recorded so far, read without disarming.
fn queue_handovers_snapshot() -> Vec<QueueHandover> {
    QUEUE_HANDOVERS.lock().unwrap().clone()
}

/// Whether any recorder ran out of room. A control must fail on this.
fn observation_overflowed() -> bool {
    OBSERVER_OVERFLOWED.load(Ordering::Acquire)
}

/// How many send attempts have been recorded so far.
///
/// A case marks this at each boundary rather than disarming, because the
/// observer has to stay armed across the invocation's close and its
/// maintenance visit: those are exactly the intervals in which an unwanted
/// resend would happen.
fn send_entries_so_far() -> usize {
    SEND_ENTRIES.lock().unwrap().len()
}

/// The send attempts recorded so far, read without disarming.
fn send_entries_snapshot() -> Vec<SendEntry> {
    SEND_ENTRIES.lock().unwrap().clone()
}

/// The writer returns recorded so far, read without disarming.
fn frames_so_far() -> Vec<ObservedFrame> {
    OBSERVED_FRAMES.lock().unwrap().clone()
}

fn take_observations() -> (Vec<ObservedFrame>, Vec<SendEntry>, Vec<QueueHandover>) {
    *WATCHED_ORIGIN.lock().unwrap() = None;
    let frames = std::mem::take(&mut *OBSERVED_FRAMES.lock().unwrap());
    let entries = std::mem::take(&mut *SEND_ENTRIES.lock().unwrap());
    let handovers = std::mem::take(&mut *QUEUE_HANDOVERS.lock().unwrap());
    (frames, entries, handovers)
}

/// Production's entry into the send-attempt recording.
pub(crate) fn observed_send_entry(
    emission: &PrivateOrderedEmission,
    frame_index: usize,
    progress: Option<(Option<usize>, usize)>,
) {
    let watched = WATCHED_ORIGIN.lock().unwrap().clone();
    let Some(watched) = watched else {
        return;
    };
    if !emission.answers_for(&watched) {
        return;
    }
    let (sent_before, frame_len) = progress.unwrap_or((None, 0));
    let mut seen = SEND_ENTRIES.lock().unwrap();
    if seen.len() >= 8192 {
        OBSERVER_OVERFLOWED.store(true, Ordering::Release);
    } else {
        seen.push(SendEntry {
            delivery: emission.delivery(),
            frames: emission.frame_count(),
            frame_index,
            sent_before,
            frame_len,
        });
    }
}

/// Production's entry into that recording. Reads nothing back and changes
/// nothing; with no origin watched, or with a step belonging to another
/// origin, it returns immediately.
pub(crate) fn observed_ordered_frame(
    emission: Option<&PrivateOrderedEmission>,
    attempted: Option<usize>,
    index: usize,
    advanced: Option<usize>,
    failure: Option<String>,
) {
    let watched = WATCHED_ORIGIN.lock().unwrap().clone();
    let (Some(watched), Some(emission)) = (watched, emission) else {
        return;
    };
    // The emission's own answer about whose registry it belongs to, compared
    // by Arc identity. Two live invocations can number a client alike.
    if !emission.answers_for(&watched) {
        return;
    }
    let mut seen = OBSERVED_FRAMES.lock().unwrap();
    if seen.len() >= 4096 {
        OBSERVER_OVERFLOWED.store(true, Ordering::Release);
    } else {
        seen.push(ObservedFrame {
            delivery: emission.delivery(),
            frames: emission.frame_count(),
            attempted,
            index,
            advanced,
            failure,
        });
    }
}

/// What the writer's own steps say about one delivery of the watched
/// invocation: how many of its frames went out whole, how many it owed, and
/// the failure that ended it if any. Whole frames only; no byte offset.
fn frames_of(
    observed: &[ObservedFrame],
    delivery: XAuthorityInputDeliveryId,
) -> (usize, usize, Option<String>, Option<usize>) {
    let mine: Vec<_> = observed
        .iter()
        .filter(|step| step.delivery == Some(delivery))
        .collect();
    let advanced = mine.iter().filter(|step| step.advanced.is_some()).count();
    let owed = mine.iter().map(|step| step.frames).max().unwrap_or(0);
    let failed = mine.iter().find(|step| step.failure.is_some());
    let failure = failed.and_then(|step| step.failure.clone());
    // WHICH FRAME IT FAILED ON, not merely that it failed. A stall on the
    // first frame of a capsule is a blocked-before-any-byte send; a stall on
    // the second is the prefix this row is about, and they are told apart
    // here rather than by counting whole frames alone.
    let failed_at = failed.map(|step| step.index);
    (advanced, owed, failure, failed_at)
}

/// Focus this connection's window and leave the notification it produced
/// sitting unread in the recipient's buffer.
///
/// THE ACKNOWLEDGEMENT IS THE PROOF THE WRITE HAPPENED, so nothing is assumed
/// by not reading it. What the unread notification buys is an odd number of
/// whole frame writes already outstanding when a stream of two-frame capsules
/// starts, which is the difference between stalling before a capsule and
/// stalling inside one. The shared helper drains it, which is right for every
/// case that wants to compare bytes and wrong for this one.
fn focus_leaving_notification_unread(
    service: &LifecycleService,
    peer: &mut UnixStream,
    window: u32,
    transaction: u64,
) -> (SurfaceId, u16, PrivateIngress) {
    let (surface, sequence) = selecting_window(
        peer,
        &service.transactions,
        window,
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6) | (1 << 21),
    );
    let custody = wait_attached(&service.registry);
    let client = custody.cleanup_record().client;
    let lease = service.owner.lease();
    let control = service
        .access
        .control_producer(&lease)
        .expect("the service's own control producer");
    control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(transaction),
                    surface,
                },
            },
        )
        .expect("the order accepts the focus control");
    assert_eq!(
        ack_for(&service.acks, transaction)
            .expect("the writer published its own outcome")
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered,
        "the focus write actually happened, which is what the unread notification is"
    );
    let ingress = service
        .access
        .ingress_for(&lease, client, DeviceId::from_raw(1))
        .expect("an actual leased producer");
    (surface, sequence, ingress)
}

/// One bounded attempt at stopping a real writer between the two frames of
/// one capsule.
///
/// Everything here is the production path: the producer, the order, the
/// runner, the writer and the receipts. What the attempt arranges is its own
/// end of the connection -- how much the recipient may hold, and whether it
/// has already left a whole frame unread -- and then it stops reading.
/// Returns what was observed, whether a same-capsule whole-frame prefix was
/// established, and the actors it collected.
fn blocked_recipient_attempt(
    label: &'static str,
    leave_notification_unread: bool,
    requested_buffer: u32,
    namespace: u64,
    window: u32,
) -> (Value, Vec<String>) {
    let _observer = own_the_observer();
    let store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let mut blocked =
        LifecycleService::launch_over_store(label, namespace, None, false, 1, store.clone());
    blocked.start();
    let (mut peer, custody) = blocked.connect();
    let (surface, _sequence, ingress) = if leave_notification_unread {
        focus_leaving_notification_unread(&blocked, &mut peer, window, namespace)
    } else {
        focus_window(&blocked, &mut peer, window, namespace)
    };
    let bounded =
        rustix::net::sockopt::set_socket_recv_buffer_size(&peer, requested_buffer as usize).is_ok();
    let effective = rustix::net::sockopt::socket_recv_buffer_size(&peer).ok();
    assert!(
        bounded,
        "{label}: the attempt could bound its own end of the real connection"
    );
    // From here this recipient never reads again.
    observe_frames(&blocked.registry);
    let mut delivered = 0u64;
    let mut outcomes: Vec<String> = Vec::new();
    let mut stalled: Option<(&'static str, u64)> = None;
    // The completion the stalling delivery's own admission minted, kept as the
    // original handle so its receipt can be compared against the cell rather
    // than only against a channel message.
    let mut current_cell: Option<Arc<PrivateDeliveryCompletion>> = None;
    let mut stalling_receipt: Option<XAuthorityClientInputDelivery> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    'blocking: for round in 0..4_000u64 {
        if std::time::Instant::now() >= deadline {
            stalled = Some((
                "the writer was never stopped within this attempt's bound",
                0,
            ));
            break 'blocking;
        }
        // ONE MULTI-FRAME CAPSULE AT A TIME, its receipt awaited before the
        // next, so a stall belongs to a delivery this attempt can name.
        let id = 12600 + namespace * 1_000 + round;
        if ingress
            .submit(
                &blocked.owner.lease(),
                axis_to(surface, XAuthorityInputDeliveryId::from_raw(id), 30 + round),
            )
            .is_err()
        {
            stalled = Some(("the source would take no further work", id));
            break 'blocking;
        }
        current_cell = waited_for_value(|| delivery_cell(&blocked.registry, id));
        match blocked.deliveries.recv_timeout(Duration::from_secs(9)) {
            Ok(receipt) => {
                // THE RECEIPT IS THIS DELIVERY'S. One capsule is outstanding at
                // a time precisely so an answer belongs to a delivery this
                // attempt can name; taking whatever arrived would let another
                // delivery's outcome stand in for it.
                assert_eq!(
                    receipt.delivery.raw(),
                    id,
                    "{label}: the receipt answers the delivery that was submitted, not another"
                );
                let name = format!("{receipt:?}", receipt = receipt.outcome);
                if receipt.outcome != XAuthorityInputDeliveryOutcome::Flushed {
                    outcomes.push(name);
                    stalled = Some((
                        "an outcome that establishes nothing",
                        receipt.delivery.raw(),
                    ));
                    stalling_receipt = Some(receipt);
                    break 'blocking;
                }
                outcomes.push(name);
                delivered += 1;
            }
            Err(_) => {
                stalled = Some(("no receipt within the bound", id));
                break 'blocking;
            }
        }
    }
    // THE OBSERVER STAYS ARMED. Disarming here would blind the case to exactly
    // the intervals a resend would use: the invocation's own close, and the
    // maintenance visit after it. Boundaries are marked instead.
    let attempts_after_traffic = send_entries_so_far();
    // What the writer committed of the stalling capsule, before anything else
    // touches it.
    let stalled_delivery = stalled.map(|(_, id)| id).filter(|id| *id != 0);
    let (advanced, owed, failure, failed_at) = stalled_delivery
        .map(|id| frames_of(&frames_so_far(), XAuthorityInputDeliveryId::from_raw(id)))
        .unwrap_or((0, 0, None, None));
    // TYPED, AND THE CELL'S OWN. The stall is required to be the declared
    // blocked limit expiring, published to the delivery that stalled, and the
    // same value the completion its admission minted is holding. A receipt off
    // the channel alone would not say the cell was answered.
    let stalling_receipt = stalling_receipt
        .unwrap_or_else(|| panic!("{label}: the recipient stopped the writer and was answered: stalled={stalled:?} outcomes={outcomes:?}"));
    assert_eq!(
        stalling_receipt.outcome,
        XAuthorityInputDeliveryOutcome::TimedOut,
        "{label}: the stall is the declared blocked limit expiring: {stalling_receipt:?}"
    );
    assert_eq!(
        Some(stalling_receipt.delivery.raw()),
        stalled_delivery,
        "{label}: published to the delivery that stalled: {stalling_receipt:?}"
    );
    let stalling_cell = current_cell
        .clone()
        .unwrap_or_else(|| panic!("{label}: the stalling delivery's own completion was taken"));
    assert_eq!(
        stalling_cell.answer(),
        Some(stalling_receipt),
        "{label}: and it is the answer written into the completion that delivery's own admission minted"
    );

    blocked.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = blocked.closed();
    let attempts_after_close = send_entries_so_far();
    let before = retained_dispatch(&blocked);
    // THE HOME AND THE CAPSULE IT STILL HOLDS, read from the home itself.
    // The retained-release list is empty for an axis transient, so comparing
    // it either side of the visit compares nothing; this is the custody the
    // row is actually about.
    let home = Arc::clone(&custody.cleanup_record().ordered_home);
    let capsule_before = held_capsule(&home, &blocked.registry)
        .unwrap_or_else(|| panic!("{label}: this connection's home still holds its serving owner"));
    let credit_before_visit = store.reserved();

    // ARMED AGAIN, AROUND THE VISIT ALONE. Whether the store looks the same
    // afterwards settles nothing for this capsule: an axis capsule is not a
    // held release in terminal dispatch, so that comparison is empty either
    // way. What has to be established is whether the visit tried to put any
    // of this capsule on the wire again, and because the attempt is recorded
    // before the write, a retry of a frame already committed is visible
    // whether or not a closed socket would have taken it.
    // A CHARGED VISIT IS WAITED FOR, NOT ASSUMED, and not manufactured. The
    // row has to establish what a charged retained visit does with a frame it
    // still holds, so a charged visit has to happen; the traffic above spent
    // this budget's starts, so the first one after it can legitimately yield
    // with a retry window. Waiting that window out is reading the budget's own
    // answer. Nothing here is done to produce send observations: the visit
    // making no attempt at all is the expected result, and is asserted as one.
    let mut visit = blocked.step();
    let mut visits = BoundedTrace::default();
    visits.push(|| format!("{visit:?}"));
    let mut terminal_visits: Vec<String> = Vec::new();
    let visit_deadline = std::time::Instant::now() + Duration::from_secs(10);
    // THE OUTPUT VISIT IS THE ONE THIS ROW IS ABOUT. The scheduler takes its
    // phases in turn, so a charged Terminal visit is a legitimate answer and
    // not this one; it is recorded and stepped past. Only an allowance that
    // says it is retryable is waited out, for the delay it reports; any other
    // refusal ends the loop and is asserted on rather than slept through.
    while !(visit.phase == PrivateMaintenancePhase::Output && visit.charged)
        && std::time::Instant::now() < visit_deadline
    {
        match visit.allowance_refusal {
            None => {}
            // Every refusal that names a delay is waited out for it; the two
            // that name none are what waiting cannot fix.
            Some(
                sophia_input_authority::ServiceStartRefusal::StartsExhausted { retry_after }
                | sophia_input_authority::ServiceStartRefusal::TimeExhausted { retry_after }
                | sophia_input_authority::ServiceStartRefusal::CleanupStartsReserved { retry_after }
                | sophia_input_authority::ServiceStartRefusal::CleanupTimeReserved { retry_after },
            ) => {
                std::thread::sleep(retry_after.min(Duration::from_millis(50)));
            }
            Some(
                other @ (sophia_input_authority::ServiceStartRefusal::ClockRegressed
                | sophia_input_authority::ServiceStartRefusal::Interrupted),
            ) => panic!(
                "{label}: the retained visit refused for something waiting cannot fix: {other:?} in {visits:?}"
            ),
        }
        visit = blocked.step();
        // A charged terminal visit is a real answer, recorded as what it was
        // rather than skipped silently, and required to have kept its
        // supervisor before this case steps past it.
        if visit.phase == PrivateMaintenancePhase::Terminal && visit.charged {
            assert!(
                visit.supervision_ok,
                "{label}: a charged terminal visit kept its supervisor: {visit:?}"
            );
            terminal_visits.push(format!(
                "visit={:?} refusal={:?}",
                visit.terminal_visit, visit.terminal_refusal
            ));
        }
        visits.push(|| format!("{visit:?}"));
    }
    assert_eq!(
        visit.phase,
        PrivateMaintenancePhase::Output,
        "{label}: a charged retained output visit was reached: {visits:?}"
    );
    let drive = store.drive();
    let after = retained_dispatch(&blocked);
    let capsule_after = held_capsule(&home, &blocked.registry)
        .unwrap_or_else(|| panic!("{label}: the home still holds it after the visit"));
    let credit_after_visit = store.reserved();
    let (observed_frames, send_entries, queue_handovers) = take_observations();

    let wanted = stalled_delivery.map(XAuthorityInputDeliveryId::from_raw);
    // Every send attempt made after this invocation's traffic stopped: its
    // close, and its maintenance visit. For the capsule that stalled, none of
    // them may target a frame it had already committed.
    let attempts_after_the_stall: Vec<_> = send_entries
        .iter()
        .skip(attempts_after_traffic)
        .filter(|entry| entry.delivery == wanted)
        .collect();
    // The exact frame sequence this capsule's own send attempts walked, so a
    // prefix is read from the order of attempts and not only from counting
    // how many reports came back advanced.
    let attempted_frame_sequence: Vec<usize> = send_entries
        .iter()
        .filter(|entry| entry.delivery == wanted)
        .map(|entry| entry.frame_index)
        .collect();
    let resent_committed = attempts_after_the_stall
        .iter()
        .filter(|entry| entry.frame_index < advanced)
        .count();
    // THE RECORDER DEMONSTRABLY SAW THIS CAPSULE. A count of zero attempts in
    // the maintenance interval means nothing unless the same armed recorder
    // is shown to have caught this exact capsule's own frames going out; it
    // did, in the order the capsule owed them.
    let first_attempt = attempted_frame_sequence.first().copied();
    let reached_second_frame = attempted_frame_sequence.contains(&1);
    let returned_to_committed = attempted_frame_sequence
        .iter()
        .skip_while(|frame| **frame == 0)
        .any(|frame| *frame == 0);
    let touched_this_capsule: Vec<_> = observed_frames
        .iter()
        .filter(|step| step.delivery == wanted && step.attempted.is_some())
        .collect();
    assert!(
        visit.charged,
        "{label}: a charged visit was reached within the budget's own retry window: {visits:?}"
    );
    // TYPED, AND EXACT. What the charged visit did with the frame it still
    // held is the whole of the no-replay claim on this side, so it is compared
    // as the value it is: the frame preserved, incomplete, with none of its
    // bytes sent and its full length still owed.
    assert_eq!(
        visit.output_refusal,
        Some(PrivateRetainedDriveRefusal::FramePreserved(
            PrivateRetainedFrame::Incomplete { sent: 0, len: 32 }
        )),
        "{label}: the charged visit preserved the frame it still held rather than rebuilding it: {visit:?}"
    );
    assert!(
        visit.supervision_ok,
        "{label}: under successful supervision, so the refusal is the visit's decision and not a lost guard: {visit:?}"
    );
    assert!(
        !observation_overflowed(),
        "{label}: no recorder dropped a record; a missing tail would read like a retry that never happened"
    );
    let queue_handovers_for_it = queue_handovers
        .iter()
        .filter(|handover| Some(handover.delivery) == wanted && handover.accepted)
        .count();
    assert_eq!(
        queue_handovers_for_it, 1,
        "{label}: the stalling capsule reached its recipient's queue exactly once: {queue_handovers:?}"
    );
    assert_eq!(
        first_attempt,
        Some(0),
        "{label}: the armed recorder caught this capsule's own first frame going out: {attempted_frame_sequence:?}"
    );
    assert!(
        reached_second_frame,
        "{label}: and its second frame being tried, which is the one that stalled: {attempted_frame_sequence:?}"
    );
    assert!(
        !returned_to_committed,
        "{label}: the writer never went back to a frame this capsule had already committed: {attempted_frame_sequence:?}"
    );
    // THE PREFIX ITSELF, AS EXACT VALUES. Two frames owed, one of them gone
    // whole, and the writer stopped on the second. Nothing here falls back to
    // a weaker reading when these do not hold.
    assert_eq!(
        owed, 2,
        "{label}: the stalling capsule owed two frames: {attempted_frame_sequence:?}"
    );
    assert_eq!(
        advanced, 1,
        "{label}: exactly one of them went out whole: {attempted_frame_sequence:?}"
    );
    assert_eq!(
        failed_at,
        Some(1),
        "{label}: and the writer failed on the second: failure={failure:?}"
    );
    // THE ORDER THE WRITER WALKED, exactly. Consecutive repeats are the writer
    // waiting on the same unsent frame and are collapsed; a return to a frame
    // already committed would appear as a third step and does not.
    let mut frames_walked = attempted_frame_sequence.clone();
    frames_walked.dedup();
    assert_eq!(
        frames_walked,
        vec![0, 1],
        "{label}: frame zero, then frame one, and nothing else: {attempted_frame_sequence:?}"
    );
    assert_eq!(
        attempted_frame_sequence
            .iter()
            .filter(|frame| **frame == 0)
            .count(),
        1,
        "{label}: with the committed frame attempted exactly once: {attempted_frame_sequence:?}"
    );
    assert_eq!(
        resent_committed, 0,
        "{label}: and nothing after the stall tried one either: {attempts_after_the_stall:?}"
    );
    assert!(
        after.len() == before.len()
            && after
                .iter()
                .zip(before.iter())
                .all(|(after, before)| after.same_as(before)),
        "{label}: the retained reading is unchanged across the visit"
    );
    // THE EXACT CAPSULE, and exactly what it still owes. One of its two
    // frames went; the second is the one in hand, none of its bytes sent, its
    // full length still owed, carrying the finalizer of the debt that owns it
    // and answering for this invocation's own origin.
    assert_eq!(
        capsule_before.delivery,
        stalled_delivery.map(XAuthorityInputDeliveryId::from_raw),
        "{label}: the capsule the home holds is the one that stalled: {capsule_before:?}"
    );
    assert!(
        capsule_before.serving && capsule_before.answers_for_this_origin,
        "{label}: held by this connection's own serving owner: {capsule_before:?}"
    );
    assert_eq!(
        (capsule_before.frames_owed, capsule_before.frame_index),
        (2, 1),
        "{label}: it owed two frames and is holding the second: {capsule_before:?}"
    );
    assert_eq!(
        capsule_before.sent,
        Some(0),
        "{label}: with none of that frame's bytes sent: {capsule_before:?}"
    );
    assert_eq!(
        capsule_before.frame_bytes.as_ref().map(Vec::len),
        Some(32),
        "{label}: and its full length still in hand: {capsule_before:?}"
    );
    assert!(
        capsule_before.finalizer.is_some(),
        "{label}: carrying the finalizer of the debt that owns it: {capsule_before:?}"
    );
    assert!(
        capsule_before.blocked_micros > 0,
        "{label}: having actually waited on its recipient: {capsule_before:?}"
    );
    assert!(
        capsule_after.same_as(&capsule_before),
        "{label}: and the charged visit left every one of those unchanged, the same finalizer and the same recipient: before {capsule_before:?} after {capsule_after:?}"
    );
    // RETAINED, AND THIS CONNECTION'S OWN RECIPIENT. The home's standing is
    // required as the value it is rather than inferred from the capsule
    // answering for the origin, which is a weaker thing to know.
    assert_eq!(
        capsule_before.standing,
        PrivateHomeStanding::Retained,
        "{label}: the home is retained after its connection ended: {capsule_before:?}"
    );
    // THIS REGISTRATION, BY THE CELL THE REGISTRY MADE FOR IT. A client number
    // and a window are not an identity; two live origins can agree on both.
    assert!(
        capsule_before.serves_registration(&custody.cleanup_record().connection_state),
        "{label}: the capsule names this connection's own registration: {capsule_before:?}"
    );
    // AND THE SERVING EVIDENCE IS STILL THERE. A serving owner answers for what
    // it holds and says nothing about the wire or the worker, so what teardown
    // established is required as the values it wrote: this connection's wire
    // closed, and its worker collected through this custody's own join.
    assert!(
        capsule_before.fence.is_some(),
        "{label}: the retained home kept what closing this endpoint established: {capsule_before:?}"
    );
    assert!(
        capsule_before.joined_through(custody.join()),
        "{label}: and the worker that served it was collected through this custody's join: {capsule_before:?}"
    );
    assert!(
        !capsule_before.worker_never_started,
        "{label}: a worker did serve this home: {capsule_before:?}"
    );
    assert!(
        !capsule_before.source_poisoned,
        "{label}: and its home was readable when the connection ended: {capsule_before:?}"
    );
    // THE FINALIZER'S OWN CELL, UNANSWERED. The finalizer handle compares equal
    // to itself across a publication, so what the row rests on is the cell it
    // guards and the answer in it.
    assert!(
        capsule_before.finalizer_completion.is_some(),
        "{label}: the capsule carries the completion its finalizer answers through: {capsule_before:?}"
    );
    assert_eq!(
        capsule_before.finalizer_delivery,
        stalled_delivery.map(XAuthorityInputDeliveryId::from_raw),
        "{label}: minted for the delivery that stalled: {capsule_before:?}"
    );
    assert_eq!(
        capsule_before.finalizer_answer,
        Some(stalling_receipt),
        "{label}: holding the one answer that delivery was given: {capsule_before:?}"
    );
    assert_eq!(
        credit_before_visit, credit_after_visit,
        "{label}: and the visit released no credit"
    );
    // ANSWERED ONCE, AND NOTHING ELSE PUBLISHED. A resend that did reach the
    // recipient would publish a second receipt for the same delivery. Any
    // other receipt is equally unaccounted for here -- every delivery before
    // the stall was awaited and answered -- so nothing is filtered away.
    let trailing = blocked
        .deliveries
        .recv_timeout(Duration::from_millis(300))
        .ok();
    assert!(
        trailing.is_none(),
        "{label}: nothing further was published after the stalled delivery was answered: {trailing:?}"
    );
    let seen = json!({
        "attempt": label,
        "focus_notification_left_unread": leave_notification_unread,
        "requested_recipient_buffer": requested_buffer,
        "effective_recipient_buffer": effective,
        "capsules_flushed_before_the_stall": delivered,
        "stalled": stalled.map(|(why, id)| json!({"why": why, "delivery": id})),
        "stalling_receipt": format!("{stalling_receipt:?}"),
        "stalling_delivery_completion": Arc::as_ptr(&stalling_cell) as usize,
        "stalling_capsule": stalled_delivery.map(|id| json!({
            "delivery": id,
            "frames_this_delivery_owed": owed,
            "whole_frames_of_it_that_went_out": advanced,
            "writer_failure_on_it": failure,
            "writer_failed_on_frame": failed_at,
        })),
        "writer_steps_recorded": observed_frames.len(),
        "multi_frame_deliveries_seen": observed_frames
            .iter()
            .filter(|step| step.frames > 1)
            .map(|step| json!({
                "delivery": step.delivery.map(|id| id.raw()),
                "frames_owed": step.frames,
                "frame_index": step.index,
                "whole_frame_that_went": step.advanced,
            }))
            .take(8)
            .collect::<Vec<_>>(),
        "writer_failures": observed_frames
            .iter()
            .filter(|step| step.failure.is_some())
            .map(|step| json!({
                "delivery": step.delivery.map(|id| id.raw()),
                "frames_owed": step.frames,
                "frame_index": step.index,
                "failure": step.failure.clone(),
            }))
            .take(4)
            .collect::<Vec<_>>(),
        "retained_after_exit": format!("{before:?}"),
        "held_capsule_before_visit": format!("{capsule_before:?}"),
        "held_capsule_after_visit": format!("{capsule_after:?}"),
        "credit_before_visit": credit_before_visit,
        "credit_after_visit": credit_after_visit,
        "maintenance_visit": format!("{visit:?}"),
        "what_the_visit_reported": visit.detail.clone(),
        "typed_visit_refusal": format!("{:?}", visit.output_refusal),
        "visit_supervision_ok": visit.supervision_ok,
        "visits_until_charged": visits.evidence(),
        "charged_terminal_visits_stepped_past": terminal_visits,
        "send_attempts_for_this_capsule_after_the_stall": attempts_after_the_stall
            .iter()
            .map(|entry| json!({
                "frame_index": entry.frame_index,
                "frames_owed": entry.frames,
                "bytes_of_it_already_sent": entry.sent_before,
                "frame_length": entry.frame_len,
            }))
            .collect::<Vec<_>>(),
        "attempted_frame_sequence_for_this_capsule": attempted_frame_sequence,
        "recorder_saw_this_capsules_own_frames": json!({
            "first_attempt": first_attempt,
            "reached_the_second_frame": reached_second_frame,
            "ever_returned_to_a_committed_frame": returned_to_committed,
            "note": "a maintenance interval with no attempt at all is the expected result; it means something only because the same armed recorder caught these.",
        }),
        "send_attempts_before_close": attempts_after_traffic,
        "send_attempts_by_close": attempts_after_close,
        "send_attempts_total": send_entries.len(),
        "queue_handovers_for_this_capsule": queue_handovers_for_it,
        "writer_returns_for_this_capsule": touched_this_capsule
            .iter()
            .map(|step| json!({
                "attempted_frame": step.attempted,
                "frames_owed": step.frames,
                "whole_frame_that_went": step.advanced,
                "failure": step.failure.clone(),
            }))
            .collect::<Vec<_>>(),
        "committed_frames_retried": resent_committed,
        "frames_walked_in_order": frames_walked,
        "receipts_after_the_stall": Option::<String>::None,
        "serves_this_registration": true,
        "fence_kept_with_the_serving_owner": format!("{:?}", capsule_before.fence),
        "worker_joined_through_this_custody": capsule_before.joined_through(custody.join()),
        "no_replay_rests_on": "the attempt this invocation's own charged visit made, recorded before the write so a resend cannot hide behind a closed socket; the typed result that visit reported for the frame it still held; the capsule the home itself still holds, compared either side of the visit by delivery, frames owed, frame index, exact frame bytes, send progress, accumulated wait and carried finalizer; and the delivery being answered once. The retained-release list is empty for an axis transient and establishes nothing here, which is why it is not what this rests on.",
        "durable_drive": format!("{drive:?}"),
        "closed_error": closed.error.clone(),
        "what_a_prefix_means_here": "one whole frame of a capsule that owed more than one, with the rest stopped. The seam reports whole frames of the exact watched invocation and no byte offset, so nothing below claims a split inside a frame.",
    });
    let collected = finish_labelled(label, blocked, &[custody]);
    (seen, collected)
}
