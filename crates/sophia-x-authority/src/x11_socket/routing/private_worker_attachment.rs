// Attaching a registered ordered-output worker to a connection the private
// routed service admitted, and collecting it when the service ends.
//
// Split by subject from the service: the service frame is where this happens
// -- it is the one place that holds the checked owner lease and the private
// frontend together -- and this file is what it does there. Nothing here is
// reached by the connection thread, which only PUBLISHES what a worker will
// need, once, on its own record, and never starts one.
//
// THE THREADING MODEL IS NOT CHANGED TO MAKE THIS FIT. Connection threads are
// `'static` and cannot carry the borrowed lease; a weak upgrade is not that
// lease and is not used in its place. The worker is started from the service
// frame, and the closure it runs owns exactly the body's ingredients -- the
// home, the notice, the stop, the exit sink and the dispatch's sequence, all
// `Arc`s the connection already made -- and no registration, custody or keeper.
//
// COLLECTION OWNS THE INTERRUPT. A worker blocked inside a wire write answers
// to nothing but its socket being shut down, and its own shutdown handle lives
// inside the home it is blocked in. So the connection takes a second
// independent handle when it binds, keeps it on its record outside every
// mutex, and the service uses that one -- after the stop, before any wait.
//
// A JOIN IS NOT A SETTLEMENT. What collection establishes is that the thread
// ended and how; the registration's own destruction decided what it left, and
// a collected worker still leaves its deferred duty and its number with the
// custody's keeper, untouched by anything here.

/// What a connection publishes, once, for the worker that may serve it.
///
/// PUBLISHED ON THE RECORD THE CONNECTION ALREADY HAS, after its setup is
/// complete, and never reconstructed by client number. Its absence means the
/// connection is not ready to be served, and nothing starts for it.
#[cfg(unix)]
struct PrivateWorkerReadiness {
    /// The byte order the dispatch negotiated for this connection.
    byte_order: XByteOrder,
    /// The event sequence the dispatch advances. THE ONE IT ALREADY MAKES,
    /// shared, not a mirror.
    sequence: Arc<AtomicU16>,
    /// The authoritative stop this connection's transport was bound with.
    /// The worker's control context resolves the same `Arc` from the home;
    /// a start whose stop is a different one is refused.
    stop: Arc<AtomicBool>,
    /// An independent handle on the client socket, taken at binding.
    ///
    /// THE INTERRUPT. Shutting it down unblocks a write in progress on the
    /// shared output without taking the mutex that write holds. Owned here,
    /// outside the home, so the service can reach it while the worker cannot
    /// be reached any other way.
    interrupt: UnixStream,
}

#[cfg(unix)]
impl PrivateWorkerReadiness {
    /// Shut the connection's socket down through the owned handle.
    ///
    /// NO LOCK IS TAKEN. A socket that is already gone is not a failure to
    /// interrupt: there is nothing left blocked on it.
    fn interrupt(&self) -> std::io::Result<()> {
        match self.interrupt.shutdown(std::net::Shutdown::Both) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotConnected => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Why the service did not start a worker for a ready connection.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateAttachmentRefusal {
    /// The bound home could not be promoted to serving.
    NotPromoted(PrivateOrderedPromotion),
    /// The serving home yielded no control association.
    NoControl(PrivateControlRefusal),
    /// The home's stop is not the one the connection published.
    ForeignStop,
    /// The registered startup transaction did not start a worker.
    Startup(PrivateStartupOutcome),
}

/// What one visit established for a connection, recorded once.
///
/// ONE ATTEMPT PER SOURCE. A later visit finds this and does not respawn,
/// re-promote, replace or retry; a refusal is the answer that stands.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateAttachment {
    /// A worker was started through this custody's registered startup.
    Started,
    /// The visit did not start a worker, and why.
    ///
    /// NOT A CLAIM THAT NO THREAD EXISTS. A startup refused its permit after
    /// its spawn leaves that thread's handle in the custody's slot; the
    /// collection selects by the slot, not by this record, and reaches it.
    Refused(PrivateAttachmentRefusal),
}

/// How a collected worker's thread ended.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateJoinKind {
    Returned,
    Panicked,
}

/// What service exit established about one attached worker.
///
/// THE REAPING'S OWN ANSWER, KEPT. `reaped` says whether this collection
/// joined the thread; `join` is what the join found, published by the custody
/// when it did; `exit` is the diagnostic the body left. Only `Joined` with a
/// `join` is evidence of collection.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateWorkerCollection {
    /// The place in the owner's inventory the custody occupies.
    pub place: usize,
    /// Whether THIS collection joined the thread.
    pub joined: bool,
    pub slot_poisoned: bool,
    /// What the custody's join evidence holds, whoever published it.
    ///
    /// DISTINCT FROM `joined`. A reaping this collection was refused (the
    /// handle handed elsewhere, the publication right spent) can still read
    /// a result an earlier join published; `Some` here with `joined == false`
    /// is exactly that, and is not this collection's evidence.
    pub join: Option<PrivateJoinKind>,
    /// The reaping's own answer, whole.
    reaped: PrivateReaped,
    /// The diagnostic the body left, whole.
    exit: Option<PrivateExitReading>,
}

#[cfg(unix)]
impl PrivateCleanupRecord {
    /// Publish what a worker for this connection will need, once.
    ///
    /// AFTER SETUP, NOT BEFORE. The connection publishes this only when
    /// everything a serving worker relies on is in place; a connection that
    /// failed earlier publishes nothing and is never served. `false` says a
    /// readiness already stands and this one was not taken.
    fn publish_worker_readiness(&self, readiness: PrivateWorkerReadiness) -> bool {
        self.worker_readiness.set(readiness).is_ok()
    }

    /// What this connection published for its worker, if it is ready.
    fn worker_readiness(&self) -> Option<&PrivateWorkerReadiness> {
        self.worker_readiness.get()
    }

    /// Whether this record was prepared by exactly this registry.
    ///
    /// BY IDENTITY OF THE TABLE, not by store, keeper or number. The record
    /// holds the client table it must stop naming its connection in; a
    /// registry that owns that same table is the one that published the row.
    fn published_by(&self, registry: &XServerFrontendRouteRegistry) -> bool {
        Arc::ptr_eq(&self.clients, &registry.clients)
    }
}

#[cfg(unix)]
impl PrivateEvidenceCustody {
    /// What the service established for this connection's worker, if it
    /// visited.
    fn attachment(&self) -> Option<PrivateAttachment> {
        self.source.attachment.get().copied()
    }

    /// Whether a thread was ever spawned into this custody's slot.
    ///
    /// THE SLOT'S OWN WORD, NOT THE ATTACHMENT'S. A start refused its permit
    /// after the spawn still left a handle here, and a worker started through
    /// this custody by anything other than the service visit is still a
    /// worker in this custody. Every one of those is collected.
    ///
    /// AN UNREADABLE SLOT IS SELECTED, WHATEVER ITS RECOVERED FIELDS SAY. A
    /// life read through a poisoned guard is not an established absence: the
    /// holder that unwound may have been the start itself. Selecting it puts
    /// it in front of the reaping, which reports what it could establish
    /// (`slot_poisoned`, and a reaping that did not join) instead of this
    /// selector deciding silently that there was nothing to collect.
    fn ever_started(&self) -> bool {
        match self.worker_slot().lock() {
            Ok(slot) => slot.life != PrivateWorkerLife::NeverStarted,
            Err(_) => true,
        }
    }

    /// Record the one attachment attempt.
    fn record_attachment(&self, attachment: PrivateAttachment) -> bool {
        self.source.attachment.set(attachment).is_ok()
    }
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Where this instance's service collection records the places it could
    /// not join, for disposal to consult.
    fn uncollected_mark(&self) -> Arc<Mutex<Vec<usize>>> {
        Arc::clone(&self.uncollected)
    }

}

#[cfg(unix)]
impl<'o> PrivateServiceLease<'o> {
    /// The custodies this registry published, pinned.
    ///
    /// BOUNDED BY THE INVENTORY, AND THE LOCK IS RELEASED BEFORE ANY OF THEM
    /// IS ACTED ON. What comes back is what this registry put in the owner's
    /// places: another live instance over the same store, or an entry a
    /// retired one left, is not this registry's and is not returned.
    fn custodies_of(&self, registry: &XServerFrontendRouteRegistry) -> Vec<PrivateCustodyPin<'o>> {
        let kept = match self.owner.inventory.kept.lock() {
            Ok(kept) => kept,
            Err(poisoned) => poisoned.into_inner(),
        };
        let found: Vec<Arc<PrivateEvidenceCustody>> = kept
            .places
            .iter()
            .flatten()
            .filter(|custody| custody.cleanup_record().published_by(registry))
            .map(Arc::clone)
            .collect();
        drop(kept);
        found
            .into_iter()
            .map(|custody| PrivateCustodyPin {
                custody,
                owner: std::marker::PhantomData,
            })
            .collect()
    }
}

/// Visit this registry's ready connections and start a worker for each one
/// that has none, from the service frame.
///
/// THE ORDER PER CONNECTION: readiness must be published; the bound home is
/// promoted (an already serving home is fine); the control association is
/// resolved from that home and its stop must be the published one; then the
/// registered startup transaction spawns the body. Whatever happens is
/// recorded once on the custody, and a connection already visited is skipped.
///
/// THE BODY CLOSURE OWNS ONLY WHAT THE BODY BORROWS. It is `'static` because
/// the thread is; it carries `Arc`s to the home, the notice, the stop, the
/// exit sink and the sequence, and builds the borrowing body over them. It
/// owns no registration, custody or keeper.
#[cfg(unix)]
fn attach_ready_workers(
    frontend: &PrivateXServerFrontend,
    service: &PrivateServiceLease<'_>,
) -> usize {
    let mut started = 0;
    for pin in service.custodies_of(&frontend.broker.registry) {
        if pin.attachment().is_some() {
            continue;
        }
        let record = pin.cleanup_record();
        let Some(readiness) = record.worker_readiness() else {
            continue;
        };
        let promotion = record.promote_ordered_serving(frontend);
        if !matches!(
            promotion,
            PrivateOrderedPromotion::Ready | PrivateOrderedPromotion::AlreadyServing
        ) {
            let _ = pin.record_attachment(PrivateAttachment::Refused(
                PrivateAttachmentRefusal::NotPromoted(promotion),
            ));
            continue;
        }
        let context = match pin.prepare_control() {
            Ok(context) => context,
            Err(refusal) => {
                let _ = pin.record_attachment(PrivateAttachment::Refused(
                    PrivateAttachmentRefusal::NoControl(refusal),
                ));
                continue;
            }
        };
        if !Arc::ptr_eq(context.stop(), &readiness.stop) {
            let _ = pin.record_attachment(PrivateAttachment::Refused(
                PrivateAttachmentRefusal::ForeignStop,
            ));
            continue;
        }
        let home = Arc::clone(&record.ordered_home);
        let wake = Arc::clone(context.notice());
        let stop = Arc::clone(context.stop());
        let exit = Arc::clone(pin.exit_sink());
        let sequence = Arc::clone(&readiness.sequence);
        let byte_order = readiness.byte_order;
        let name = format!("x11-ordered-output-{}", record.client.raw());
        let spawn = move || {
            std::thread::Builder::new().name(name).spawn(move || {
                let _ = PrivateWorkerBody {
                    home: &home,
                    wake: &wake,
                    stop: &stop,
                    byte_order,
                    sequence: &sequence,
                    exit: &exit,
                    steps: usize::MAX,
                }
                .run();
            })
        };
        #[cfg(not(test))]
        let outcome = context.start(spawn);
        #[cfg(all(test, unix))]
        let outcome = routing_tests::m3_acceptance::acceptance_start(&context, &pin, spawn);
        let attachment = match outcome {
            PrivateStartupOutcome::Started => {
                started += 1;
                PrivateAttachment::Started
            }
            refused => PrivateAttachment::Refused(PrivateAttachmentRefusal::Startup(refused)),
        };
        let _ = pin.record_attachment(attachment);
    }
    started
}

/// Tell every worker in this registry's custodies to stop, and interrupt its
/// wire.
///
/// EVERY SLOT THAT EVER HELD A THREAD, not only the visits that reported
/// `Started`: a permit refused after its spawn, and a worker started into the
/// custody by a caller other than the visit, are workers here too.
///
/// BEFORE ANY WAIT. The stop goes through the connection's bound
/// association, the interrupt through the handle the connection published;
/// neither touches the home or the output. A missing association or handle
/// is reported, not silently skipped.
#[cfg(unix)]
fn stop_attached_workers(
    service: &PrivateServiceLease<'_>,
    registry: &XServerFrontendRouteRegistry,
) -> Vec<String> {
    let mut failures = Vec::new();
    for pin in service.custodies_of(registry) {
        if !pin.ever_started() {
            continue;
        }
        let place = pin.identity().index;
        match pin.bound_pair() {
            Some((stop, notice)) => cancel_connection_worker(&stop, &notice),
            None => failures.push(format!("worker at place {place} has no bound stop")),
        }
        match pin.cleanup_record().worker_readiness() {
            Some(readiness) => {
                if let Err(error) = readiness.interrupt() {
                    failures.push(format!("worker at place {place} interrupt failed: {error}"));
                }
            }
            None => failures.push(format!("worker at place {place} has no interrupt handle")),
        }
    }
    failures
}

/// Give back what departed connections left, in a window where no connection
/// frame is active.
///
/// WHY THIS EXISTS. A connection that started an ordered worker departs
/// through the deferred branch, which by design retains nothing and returns no
/// place. What would retain it is the deferred cleanup, and that needs two
/// things nothing produced during the run: the custody's published join, and
/// the collection's word that this registry's frames are all collected. Both
/// were made only by the invocation's own collection -- so a private instance
/// gave every place back at once, at shutdown, and an instance that had seen
/// `max_concurrent_clients` departures could admit nobody.
///
/// WHY THE WINDOW IS HONEST, AND WHY THE COUNT IS A PARAMETER. The token this
/// mints says no connection frame is active. That is the same fact the
/// collection's mint establishes, and here it is established the same way: the
/// service frame is the only thread that spawns a client worker, it holds the
/// frontend exclusively while it does, and it has just been told how many are
/// active. Taking the count as an argument is what keeps the reading and the
/// mint in one place; reading it here, from a frontend this function does not
/// hold, would be asking a question whose answer could already have changed.
///
/// NOTHING IS WIDENED. The token is the ordinary one and the discharge is the
/// ordinary one. Every other path that takes this token is untouched, and none
/// of them is reached from here.
///
/// AND NOTHING WAITS. The reap below is the non-blocking one: a worker still
/// running is left exactly where it is, for a later turn, because this runs
/// between accepting a connection and serving the order.
#[cfg(unix)]
fn reclaim_idle_departures(
    frontend: &PrivateXServerFrontend,
    service: &PrivateServiceLease<'_>,
    frames_active: usize,
) -> usize {
    if frames_active != 0 {
        return 0;
    }
    let registry = &frontend.broker.registry;
    let mut progressed = 0usize;
    // THE JOIN FIRST, because the discharge refuses without it -- and ONLY
    // FOR A DEPARTURE THAT HAS DECIDED. The reap is offered to a custody
    // whose destruction has been decided as a deferral, which is exactly the
    // arm the discharge will require of it next; nothing earlier, and
    // nothing else.
    //
    // NOT EVERY FINISHED THREAD, which is what this first did. A worker whose
    // permit was refused finishes at once, and its connection may still be
    // deciding its departure -- destruction Requested, not yet Decided. The
    // decision reads the slot's life to say what it found, so a reap that
    // took the handle in that gap made the decision record WorkerHandedOn
    // where the truth was WorkerRunning: the reclaim was changing what the
    // departure said about itself, not merely racing a reader of the slot.
    // It reached the M3 start-failures control one run in ten.
    //
    // A live connection's dead worker is left for the collection too, as it
    // always was. Reaping it would publish a join nobody asked for and make
    // that connection NoLongerStartable, and the reclaim has no business with
    // a connection that has not left.
    for pin in service.custodies_of(registry) {
        if !pin.ever_started() {
            continue;
        }
        if !matches!(
            pin.cleanup_record().destruction_standing(),
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(
                PrivateDestructionDeferral::WorkerRunning
                    | PrivateDestructionDeferral::WorkerHandedOn,
            ))
        ) {
            continue;
        }
        if PrivateReapingRecord::bound_to(&pin).reap_finished().reaped
            == PrivateReaped::Joined
        {
            progressed += 1;
        }
    }
    // THEN THE DISCHARGE, on the same selection and under the token this
    // window earned. A custody whose worker is still running refuses
    // `JoinUnpublished` and is visited again on a later turn.
    let collected = PrivateConnectionsCollected {
        registry: Arc::clone(&registry.clients),
    };
    for outcome in run_deferred_cleanups(service, registry, Some(&collected)) {
        if outcome.result.is_ok() {
            progressed += 1;
        }
    }
    // THEN THE PLACE, which is what the discharge hands to the continuation
    // store, and only a settled continuation gives it back.
    progressed += registry.drive_departed_continuations();
    // AND LAST THE CUSTODY, because it is the last thing a departed connection
    // holds and the only thing the admission after it runs out of once the
    // place has come back. Each step above leaves what the next one needs --
    // the join for the discharge, the discharge's commitment and the returned
    // place for the evidence -- and one turn may not finish all four for one
    // connection. The next idle turn continues; nothing here loops to force
    // it.
    //
    // WITHOUT THE INSTANCE-WIDE GATES, AND WITH THE ONE THING THEY PROTECTED.
    // The invocation-end visit retires a custody only once the invocation has
    // completed and no control record is outstanding anywhere. Neither is a
    // fact about this custody. What the control gate was protecting is real:
    // control cleanup pairs each record with its connection's custody slot,
    // so a custody must outlive its OWN client's unanswered records. So that is
    // what is asked, per client, and nothing about anybody else's.
    let controls = registry.control_completion();
    for pin in service.custodies_of(registry) {
        let client = pin.cleanup_record().client;
        if controls
            .as_ref()
            .and_then(|controls| controls.outstanding_for(client))
            != Some(0)
        {
            continue;
        }
        let custody = Arc::clone(&pin.custody);
        if PrivateRetainedExecutionResources::retire_one_completed_custody(
            &custody,
            Some(&collected),
            service,
        )
        .is_ok()
        {
            progressed += 1;
        }
    }
    progressed
}

/// Collect every worker in this registry's custodies, through the join
/// custody -- every slot that ever held a thread, as above.
///
/// AFTER THE STOP AND THE INTERRUPT, AND AFTER THE CONNECTION THREADS. Each
/// reaping is the custody's own; what it found is kept whole. A worker whose
/// handle was not here to join -- handed elsewhere, missing, or already
/// asked -- is reported as uncollected by place, and nothing about it is
/// assumed.
#[cfg(unix)]
fn collect_attached_workers(
    service: &PrivateServiceLease<'_>,
    registry: &XServerFrontendRouteRegistry,
) -> (Vec<PrivateWorkerCollection>, Vec<usize>) {
    let mut collected = Vec::new();
    let mut uncollected = Vec::new();
    for pin in service.custodies_of(registry) {
        if !pin.ever_started() {
            continue;
        }
        let place = pin.identity().index;
        let record = PrivateReapingRecord::bound_to(&pin);
        let reaping = record.reap();
        let join = record.result().map(|result| match result {
            PrivateJoinResult::Returned => PrivateJoinKind::Returned,
            PrivateJoinResult::Panicked(_) => PrivateJoinKind::Panicked,
        });
        // A WORKER ALREADY JOINED IS COLLECTED, WHOEVER JOINED IT. This
        // attempt answers `AlreadyAsked` when the live idle-window reclaim
        // reached the same custody first, and that is not a worker nobody
        // collected -- it is one collected earlier. The published result is
        // what says so, and it is the custody's own: no report, mark or
        // socket state stands in for it.
        //
        // Read from the record rather than from this attempt, because this
        // attempt deliberately did not consume anything in that case.
        let joined = reaping.reaped == PrivateReaped::Joined || join.is_some();
        if !joined {
            uncollected.push(place);
        }
        collected.push(PrivateWorkerCollection {
            place,
            joined,
            slot_poisoned: reaping.slot_poisoned,
            join,
            reaped: reaping.reaped,
            exit: reaping.exit,
        });
    }
    (collected, uncollected)
}
