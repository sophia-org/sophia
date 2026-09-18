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
    /// No worker was started, and why.
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
    /// Whether this collection joined the thread. `join` is `Some` exactly
    /// when it did.
    pub joined: bool,
    pub slot_poisoned: bool,
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
    /// worker in this custody. Every one of those is collected; an
    /// unreadable slot is treated as one that may hold a worker.
    fn ever_started(&self) -> bool {
        match self.worker_slot().lock() {
            Ok(slot) => slot.life != PrivateWorkerLife::NeverStarted,
            Err(poisoned) => poisoned.into_inner().life != PrivateWorkerLife::NeverStarted,
        }
    }

    /// Record the one attachment attempt.
    fn record_attachment(&self, attachment: PrivateAttachment) -> bool {
        self.source.attachment.set(attachment).is_ok()
    }
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Prepare this instance's applied registry for the service's namespace,
    /// so a promoted home can establish the endpoint identity it serves.
    ///
    /// THE ONE PREPARATION PROMOTION NEEDS, AND NO MORE. Promotion asks the
    /// applied registry for the connection's endpoint; an applied owner that
    /// was never installed answers "preparation incomplete" and nothing is
    /// promoted. The prepared runner installs the same owner as part of
    /// preparing the private input pipeline; this installs only the owner,
    /// once, for the service's own namespace, and attaches no producer,
    /// runner or executor. Connections that arrive afterwards bind their
    /// selections to it as they attach their state.
    /// Where this instance's service collection records the places it could
    /// not join, for disposal to consult.
    fn uncollected_mark(&self) -> Arc<Mutex<Vec<usize>>> {
        Arc::clone(&self.uncollected)
    }

    pub(crate) fn prepare_applied_for_service(
        &self,
        namespace: NamespaceId,
    ) -> Result<(), PrivateAppliedRegistryRefusal> {
        self.broker
            .registry
            .install_private_applied(&self.participant, namespace)
            .map(|_publication| ())
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
        let outcome = context.start(move || {
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
        });
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
        if reaping.reaped != PrivateReaped::Joined {
            uncollected.push(place);
        }
        collected.push(PrivateWorkerCollection {
            place,
            joined: reaping.reaped == PrivateReaped::Joined,
            slot_poisoned: reaping.slot_poisoned,
            join,
            reaped: reaping.reaped,
            exit: reaping.exit,
        });
    }
    (collected, uncollected)
}
