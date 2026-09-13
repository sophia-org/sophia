/// The one place a private instance's runnable work is accepted.
///
/// Producers admit here directly rather than into their own channels for a
/// consumer to collect later. Position is assigned and the entry published
/// inside one hold on this lock, so two producers cannot interleave between
/// the two, and a send that has returned cannot be overtaken by one that
/// started afterwards.
#[cfg(unix)]
pub struct SharedAdmission {
    ready: Mutex<SharedQueue>,
    /// Set once the stream can no longer name an entry.
    ///
    /// Terminal, unlike a full queue. Retrying cannot produce an identity that
    /// does not exist, so further acceptance stops rather than looping. What
    /// was already accepted keeps its completion and its debt; this refuses
    /// new work instead of pretending the instance is healthy.
    exhausted: AtomicBool,
}

#[cfg(unix)]
impl SharedAdmission {
    fn new(ready: crate::ReadyStream<PrivateOperation>) -> Self {
        Self {
            ready: Mutex::new(SharedQueue {
                ready,
                closed: false,
            }),
            exhausted: AtomicBool::new(false),
        }
    }

    /// Stop accepting, and take what was accepted and never run.
    ///
    /// Closing happens under the same lock acceptance takes, so a producer is
    /// either accepted before the close or refused after it, never accepted
    /// into a queue nobody will drain. What was already accepted comes back
    /// here: those entries were promised a consumer and are owed an outcome,
    /// so they are handed to whoever closes rather than dropped with the
    /// queue.
    fn close(&self) -> Result<Vec<PrivateOperation>, ()> {
        // A queue that cannot be opened cannot be closed or drained either.
        // Returning an empty list here would say there was nothing owed.
        let mut queue = self.ready.lock().map_err(|_| ())?;
        queue.closed = true;
        let mut stranded = Vec::new();
        while let Some((_, _, operation)) = queue.ready.take_next() {
            stranded.push(operation);
        }
        Ok(stranded)
    }

    /// Accept runnable work, assigning its position as it is published.
    ///
    /// A refusal returns the operation itself rather than some part of it.
    /// Control and cleanup are not routes, so a refusal that handed back only
    /// a route would destroy exactly the work that has no other owner.
    fn accept(
        &self,
        class: crate::ReadyClass,
        operation: PrivateOperation,
    ) -> Result<crate::ReadySequence, (AdmissionRefusal, PrivateOperation)> {
        // Checked before acceptance, so an exhausted stream never takes work
        // it cannot name.
        if self.exhausted.load(Ordering::Acquire) {
            return Err((AdmissionRefusal::Exhausted, operation));
        }
        let Ok(mut queue) = self.ready.lock() else {
            return Err((AdmissionRefusal::Unavailable, operation));
        };
        // Checked inside the same hold as admission, so a close cannot land
        // between deciding this is acceptable and accepting it.
        if queue.closed {
            return Err((AdmissionRefusal::ConsumerGone, operation));
        }
        match queue.ready.admit(class, operation) {
            Ok(sequence) => Ok(sequence),
            Err(refused) => match refused.refusal {
                crate::ReadyRefusal::AtCapacity => {
                    Err((AdmissionRefusal::Saturated, refused.payload))
                }
                crate::ReadyRefusal::SequencesExhausted => {
                    // Latched here rather than rediscovered on every later
                    // send, and never reset: reusing a position would answer
                    // one request with another's identity.
                    self.exhausted.store(true, Ordering::Release);
                    Err((AdmissionRefusal::Exhausted, refused.payload))
                }
            },
        }
    }

    /// Take the next entry, distinguishing an empty queue from an unusable one.
    ///
    /// Mapping a poisoned lock to `None` made an unreachable queue look drained
    /// and a run of it look like progress, while accepted work sat in it
    /// unanswered.
    fn take_next(
        &self,
    ) -> Result<Option<(crate::ReadySequence, crate::ReadyClass, PrivateOperation)>, ()> {
        let mut queue = self.ready.lock().map_err(|_| ())?;
        Ok(queue.ready.take_next())
    }
}

/// What a shutdown could not settle, handed to whoever shut it down.
///
/// Counting an unsettled obligation and logging it is a diagnostic, not a
/// transfer: the work still leaves scope unanswered. This carries the
/// obligations themselves, so an owner that is still alive can do something
/// about them.
#[cfg(unix)]
#[must_use = "unsettled work is owed an answer by whoever shut this down"]
pub struct PrivateShutdown {
    /// Accepted work that could not be answered here.
    ///
    /// Held rather than published: the envelope it carries is module-private,
    /// and exporting the stamped wire shape so an out-of-crate owner could
    /// read a report is a wider decision than this. What is public is that
    /// obligations remain and how many, which is what a caller must not be
    /// able to ignore.
    unsettled: Vec<PrivateOperation>,
    /// The queue could not be read, so nothing in it could even be recovered.
    pub queue_unreadable: bool,
}

#[cfg(unix)]
impl PrivateShutdown {
    pub fn is_settled(&self) -> bool {
        self.unsettled.is_empty() && !self.queue_unreadable
    }

    /// How many obligations this shutdown could not discharge.
    pub fn owed(&self) -> usize {
        self.unsettled.len()
    }
}

/// One operation the consumer took, and where it sat.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateRun {
    pub sequence: crate::ReadySequence,
    pub class: crate::ReadyClass,
    /// Which operation this was, not merely what kind.
    ///
    /// A class alone says a record was classified, not that it accompanied the
    /// work a producer actually submitted.
    pub identity: PrivateIdentity,
}

/// Which submitted operation a run corresponds to.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateIdentity {
    /// The delivery a routed input carried, when it carried one.
    Delivery(Option<XAuthorityInputDeliveryId>),
    /// The transaction a control named. Every control names one.
    Transaction(TransactionId),
    /// The lease being retired.
    Lease(sophia_protocol::ApplicationRouteLeaseIdentity),
}

#[cfg(unix)]
impl PrivateIdentity {
    fn of(operation: &PrivateOperation) -> Self {
        match operation {
            PrivateOperation::RoutedInput(envelope) => Self::Delivery(envelope.route.delivery),
            // Every control command carries a transaction, so singling one
            // variant out and calling the rest untracked lost the identity of
            // everything except focus.
            PrivateOperation::Control(control) => Self::Transaction(control.command.transaction()),
            PrivateOperation::LeaseRelease(release) => Self::Lease(release.identity),
        }
    }
}

/// The shared queue and whether it is still being drained.
#[cfg(unix)]
struct SharedQueue {
    ready: crate::ReadyStream<PrivateOperation>,
    closed: bool,
}

/// Why the shared admission would not accept work.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionRefusal {
    /// No room now. Retrying later is sensible.
    Saturated,
    /// Positions are exhausted. Terminal: retrying cannot create an identity
    /// that does not exist.
    Exhausted,
    /// The shared queue cannot be reached.
    Unavailable,
    /// The consumer is gone. Nothing accepted now could ever run.
    ConsumerGone,
}

/// Why a private producer's work was not accepted.
///
/// Denial and saturation are different answers and a caller acts on them
/// differently: saturation says try again, denial says this will not be
/// accepted until something changes. The ordinary backend reports a stamp
/// refusal as `TrySendError::Full`, which tells a caller to retry work that is
/// being refused on policy. A private producer is told which it is.
#[cfg(unix)]
#[derive(Debug)]
pub enum PrivateSendError {
    /// No stamp: a transition is in flight, or routing is otherwise closed.
    /// The work is handed back, unaccepted.
    Denied(XAuthorityRoutedInput),
    /// The ingress is full. The work is handed back, and retrying is sensible.
    Saturated(XAuthorityRoutedInput),
    /// The consumer is gone.
    Disconnected(XAuthorityRoutedInput),
    /// This delivery id is already live. Retrying cannot help, and cancelling
    /// the live one would answer a different request.
    DeliveryAlreadyTracked(XAuthorityRoutedInput),
    /// The recovery ledger or the shared queue cannot be reached.
    Unavailable(XAuthorityRoutedInput),
    /// Positions are exhausted. Terminal for this instance: what was already
    /// accepted keeps its completion, and nothing further is taken.
    Exhausted(XAuthorityRoutedInput),
}

/// A producer's handle to a private frontend.
///
/// Offers one way in, and answers with a typed refusal. The ordinary sender is
/// deliberately not reachable through this: its `try_send` calls a policy
/// denial `Full`, which tells a caller to retry something that is being
/// refused.
#[cfg(unix)]
pub struct PrivateIngress {
    sender: XAuthorityRoutedInputSender,
    admission: Arc<SharedAdmission>,
}

/// A producer of control work, bound to one instance's shared admission.
#[cfg(unix)]
pub struct PrivateControlProducer {
    admission: Arc<SharedAdmission>,
}

#[cfg(unix)]
impl PrivateControlProducer {
    /// Accept control into the shared order.
    pub fn submit(
        &self,
        control: XAuthorityClientControlCommand,
    ) -> Result<crate::ReadySequence, (AdmissionRefusal, XAuthorityClientControlCommand)> {
        self.admission
            .accept(crate::ReadyClass::Control, PrivateOperation::Control(control))
            .map_err(|(refusal, returned)| match returned {
                PrivateOperation::Control(control) => (refusal, control),
                _ => unreachable!("control is returned as control"),
            })
    }
}

#[cfg(unix)]
impl PrivateIngress {
    /// Accept work into the shared order, stamping it first.
    ///
    /// Position is assigned as the entry is published, inside the shared
    /// admission's own hold, so a send that has returned cannot be overtaken
    /// by one that started afterwards.
    pub fn submit(&self, route: XAuthorityRoutedInput) -> Result<crate::ReadySequence, PrivateSendError> {
        let envelope = self.sender.stamp_and_reserve(route)?;
        self.admission
            .accept(
                crate::ReadyClass::RoutedInput,
                PrivateOperation::RoutedInput(envelope),
            )
            .map_err(|(refusal, returned)| {
                let route = match returned {
                    PrivateOperation::RoutedInput(envelope) => envelope.route,
                    _ => unreachable!("routed input is returned as routed input"),
                };
                // The reservation this send made is rolled back, and only
                // this one: another request's live delivery is untouched.
                self.sender.abort_reservation(route.delivery);
                match refusal {
                    AdmissionRefusal::Saturated => PrivateSendError::Saturated(route),
                    AdmissionRefusal::Exhausted => PrivateSendError::Exhausted(route),
                    AdmissionRefusal::Unavailable => PrivateSendError::Unavailable(route),
                    AdmissionRefusal::ConsumerGone => PrivateSendError::Disconnected(route),
                }
            })
    }
}

/// How much of a private host's ready capacity is kept for cleanup.
#[cfg(unix)]
const PRIVATE_CLEANUP_RESERVE: usize = 4;

// The control-transition surface a broker under a coordinator exposes.
//
// Split from the broker so neither file has to grow past what the layout gate
// allows; the split is by subject, so what a control transition produces and
// how it is applied and reported stay together.

/// Why a broker refused to come under a coordinator.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationRefused {
    /// A different coordinator is already installed.
    ///
    /// Reported rather than ignored: the cell accepts one value and discards
    /// later ones silently, which would tell a caller its coordinator was in
    /// charge while another one was.
    DifferentGateInstalled,
    /// A raw ingress handle was handed out before this coordinator arrived.
    ///
    /// That handle cannot be recalled, and work already sent through it cannot
    /// be answered, so the instance stays ordinary rather than becoming a
    /// private one with an unstamped way in.
    RawIngressAlreadyExposed,
    /// Raw ingress was asked for while a coordinator is installed.
    RawIngressRefusedUnderGate,
}

/// What a control transition left for its caller to deliver.
///
/// Carried rather than sent, because delivery must happen with the guards
/// released, and owed regardless of whether the transition applied: work
/// already taken off the frozen queue has no other way to be answered.
#[cfg(unix)]
#[must_use = "revoked input owes its clients a receipt"]
pub struct ControlTransitionOutcome {
    /// Which broker revoked this work.
    ///
    /// Receipts name clients, and client identifiers are only unique within
    /// one frontend. Handing this batch to another broker would deliver one
    /// frontend's revocations to whichever of its clients happened to share
    /// those numbers.
    ///
    /// An allocation rather than a number: identity by address cannot be
    /// exhausted, and holding it keeps it distinct for as long as any batch
    /// still refers to it, which a counter could not promise.
    registry: Arc<()>,
    receipts: VecDeque<XDeferredRoutedInput>,
    applied: bool,
}

impl ControlTransitionOutcome {
    /// Whether the coordinator recorded the transition as applied.
    pub fn applied(&self) -> bool {
        self.applied
    }
}

#[cfg(unix)]
impl XServerFrontendRouteBroker {
    /// Apply a control transition's X-side clearing.
    ///
    /// The privileged control path. The permit is how the signature requires
    /// what the documentation used to only assert: it borrows the common
    /// authority exclusively, so a caller without that access cannot produce
    /// one, and it remains obtainable after grants are revoked and while
    /// publication is unavailable, which is exactly when a transition needs it.
    ///
    /// Its identity is checked against the coordinator's. Revision numbers
    /// alone never bound the two together: two authorities at the same epoch
    /// and publication are indistinguishable by value, so without this a
    /// coordinator could drive a different authority whenever their revisions
    /// coincided.
    ///
    /// Lock order is the one a transition is opened with: coordinator, then
    /// common, then the X guards. The caller already holds the first two, so
    /// this takes only what ranks below them. Reaching for the gate here while
    /// holding common would close a cycle between exactly those two, which is
    /// why the coordinator arrives by reference.
    ///
    /// Everything is validated before anything is destroyed. A stale or
    /// foreign token, or an installation that would not satisfy the kind in
    /// flight, refuses while the state it describes is still intact.
    ///
    /// A publication-only transition clears nothing. Grabs, pointer state and
    /// frozen input belong to an epoch it is not replacing, and destroying
    /// them would make every focus change cost a client its grab.
    ///
    /// The outcome must be delivered by the caller once every guard is
    /// released. It carries its receipts whether or not the transition
    /// applied, because work already taken off the frozen queue is owed a
    /// receipt no matter what happened afterwards.
    pub fn apply_control_transition(
        &self,
        permit: &sophia_input_authority::ControlPermit<'_>,
        coordinator: &mut crate::TransitionAccess<'_>,
        token: crate::TransitionToken,
        snapshot_installed: bool,
    ) -> Result<ControlTransitionOutcome, XServerFrontendRouteError> {
        coordinator
            .check_permit(permit)
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        // The permit and the coordinator agreeing with each other says nothing
        // about this broker. Without this, another authority's entirely valid
        // coordinator, permit and token would clear these X populations, and
        // an ungated broker would clear them for anyone at all.
        let installed = self
            .control_gate
            .get()
            .ok_or(XServerFrontendRouteError::RegistryPoisoned)?;
        if installed.coordinator_incarnation() != coordinator.incarnation() {
            return Err(XServerFrontendRouteError::RegistryPoisoned);
        }
        let kind = coordinator
            .pending_kind_for(token)
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let cleared = match kind {
            crate::TransitionKind::SecurityControl => crate::TransitionInstallation {
                x_grabs_cleared: true,
                pointer_state_cleared: true,
                frozen_input_cleared: true,
                snapshot_installed,
            },
            crate::TransitionKind::Publication => crate::TransitionInstallation {
                snapshot_installed,
                ..crate::TransitionInstallation::default()
            },
        };
        // Asked before the clearing, so an incomplete report refuses without
        // having already destroyed what it was reporting on.
        coordinator
            .would_install(token, cleared)
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;

        let drained = match kind {
            crate::TransitionKind::SecurityControl => {
                self.registry.clear_revoked_x_populations()?
            }
            crate::TransitionKind::Publication => VecDeque::new(),
        };
        let applied = coordinator.apply(token, cleared);
        Ok(ControlTransitionOutcome {
            registry: Arc::clone(&self.registry_identity),
            receipts: drained,
            applied: applied.is_ok(),
        })
    }

    /// Deliver what a control transition revoked.
    ///
    /// Separate from the apply so it runs with every guard released.
    ///
    /// A batch offered to the wrong broker is handed back with the refusal
    /// rather than consumed. Dropping it would lose receipts that clients of
    /// the originating broker are owed, turning a caller's mistake into
    /// silently abandoned work; returning it leaves the origin able to deliver
    /// what it revoked.
    ///
    /// Delivery by the right broker is a different matter. It fails only when
    /// the recovery ledger is poisoned, which is a terminal fault of this
    /// instance rather than something a caller can retry, so the batch that
    /// comes back with that error is empty and carries no promise. Do not read
    /// it as work still waiting to be delivered.
    pub fn report_control_transition(
        &self,
        outcome: ControlTransitionOutcome,
    ) -> Result<usize, (XServerFrontendRouteError, ControlTransitionOutcome)> {
        if !Arc::ptr_eq(&outcome.registry, &self.registry_identity) {
            return Err((XServerFrontendRouteError::RegistryPoisoned, outcome));
        }
        self.registry
            .report_revoked_input(outcome.receipts)
            .map_err(|error| {
                (
                    error,
                    ControlTransitionOutcome {
                        registry: Arc::clone(&self.registry_identity),
                        receipts: VecDeque::new(),
                        applied: false,
                    },
                )
            })
    }

}

/// A frontend built private, and the only way to get one.
///
/// Construction order is the safety property. The coordinator exists before
/// the broker does, so there is no interval in which a handle could be taken
/// from an ungated instance, and no raw ingress is offered at all. An
/// ordinary broker can still be asked to become private later, but that path
/// refuses when a handle already escaped, because a handle cannot be recalled
/// and a send that returned success cannot be answered afterwards.
///
/// The broker is not reachable through this. `input_sender` refuses under a
/// coordinator regardless, so this is the second of two answers rather than
/// the only one, but a facade that handed the broker out would make the first
/// answer the only thing standing between a caller and an unstamped way in.
///
/// Raw ingress is not merely undocumented here; it is absent:
///
/// ```compile_fail
/// # use sophia_x_authority::PrivateXServerFrontend;
/// fn take_unstamped_ingress(private: &PrivateXServerFrontend) {
///     let _ = private.input_sender();
/// }
/// ```
#[cfg(unix)]
pub struct PrivateXServerFrontend {
    broker: XServerFrontendRouteBroker,
    /// The one place runnable work is accepted, shared with every producer
    /// handle this frontend hands out.
    admission: Arc<SharedAdmission>,
    /// The most this will run in one turn.
    service_budget: usize,
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Stop accepting, settle what can be settled, and hand back what cannot.
    ///
    /// The explicit path. It runs while the registry and the authority are
    /// still alive, so settlement here can actually reach a client, and it
    /// returns the obligations it could not discharge to a caller that must
    /// deal with them. Drop is the fallback for when nobody called this, and
    /// a fallback cannot be the only thing able to see a failure.
    pub fn shutdown(mut self) -> PrivateShutdown {
        let report = self.settle_accepted();
        // Drop still runs, and will find the queue already closed and empty.
        report
    }

    /// Close and answer what was accepted, returning what is still owed.
    fn settle_accepted(&mut self) -> PrivateShutdown {
        let stranded = match self.admission.close() {
            Ok(stranded) => stranded,
            Err(()) => {
                return PrivateShutdown {
                    unsettled: Vec::new(),
                    queue_unreadable: true,
                };
            }
        };
        let mut unsettled = Vec::new();
        for operation in stranded {
            match operation {
                PrivateOperation::RoutedInput(envelope) => {
                    // The consumer stopping is not evidence the target is
                    // gone. This work was accepted and not performed, which is
                    // a rejected route.
                    let client = self
                        .broker
                        .registry
                        .surfaces
                        .lock()
                        .ok()
                        .and_then(|surfaces| {
                            surfaces
                                .get(&envelope.route.request.target_surface)
                                .map(|route| route.client)
                        });
                    let Some(client) = client else {
                        // Ownership is retained rather than resolved by
                        // guessing. A receipt goes to the issuer's channel
                        // rather than to the named client, so the harm is not
                        // that some other client would receive it -- frontend
                        // ids start at one and never wrap. It is that a
                        // receipt attributed to a client nobody resolved
                        // cannot be correlated with anything. Choosing a
                        // recipient here would also choose it at the wrong
                        // moment: final target resolution belongs at
                        // execution.
                        unsettled.push(PrivateOperation::RoutedInput(envelope));
                        continue;
                    };
                    if self
                        .broker
                        .registry
                        .send_input_delivery(
                            client,
                            envelope.route.delivery,
                            XAuthorityInputDeliveryOutcome::RouteRejected,
                        )
                        .is_err()
                    {
                        unsettled.push(PrivateOperation::RoutedInput(envelope));
                    }
                }
                PrivateOperation::Control(control) => {
                    // Control has its own acknowledgement contract. Not
                    // ClientGone: the authority stopped, not the client.
                    let acknowledgement = XAuthorityClientControlAck {
                        client: control.client,
                        acknowledgement: XAuthorityControlAck {
                            kind: control.command.kind(),
                            transaction: control.command.transaction(),
                            surface: control.command.surface(),
                            outcome: XAuthorityControlOutcome::AuthorityRejected,
                        },
                    };
                    if self
                        .broker
                        .registry
                        .acknowledgement_sender
                        .try_send(acknowledgement)
                        .is_err()
                    {
                        // A full acknowledgement channel is reachable by
                        // ordinary use. The obligation is retained, not
                        // counted and dropped.
                        unsettled.push(PrivateOperation::Control(control));
                    }
                }
                PrivateOperation::LeaseRelease(release) => {
                    // A queued release is an obligation to perform settlement,
                    // not proof that settlement happened. Perform it through
                    // privileged cleanup, and retain it if that fails.
                    if self
                        .broker
                        .registry
                        .release_route_lease(release)
                        .is_err()
                    {
                        unsettled.push(PrivateOperation::LeaseRelease(release));
                    }
                }
            }
        }
        PrivateShutdown {
            unsettled,
            queue_unreadable: false,
        }
    }
}

#[cfg(unix)]
impl Drop for PrivateXServerFrontend {
    fn drop(&mut self) {
        // The fallback. An owner that called shutdown has already settled and
        // this finds nothing; an owner that did not gets a best effort and a
        // report, because there is nowhere left to hand an obligation to.
        let report = self.settle_accepted();
        if !report.is_settled() {
            tracing::error!(
                "sophia_private_admission status=unsettled unreadable={} owed={}",
                report.queue_unreadable,
                report.unsettled.len()
            );
        }
    }
}

/// One thing the private host has to run, in the order it was admitted.
///
/// Carried in a shutdown report so an owner can act on what it is handed, but
/// crate-visible: publishing it would export the stamped envelope shape for
/// the sake of a report.
#[cfg(unix)]
enum PrivateOperation {
    /// Input admitted through the stamped envelope.
    RoutedInput(XAuthorityEpochRoutedInput),
    /// A route lease being retired. Privileged cleanup.
    ///
    /// No producer facade admits these yet. The class and its reserve exist
    /// because cleanup must never be the thing that cannot be admitted, and
    /// building that in later would mean reworking capacity policy under a
    /// live order.
    #[allow(dead_code)]
    LeaseRelease(XAuthorityRouteLeaseRelease),
    /// Control whose application belongs in this order.
    Control(XAuthorityClientControlCommand),
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Build a frontend that is private from the moment it exists.
    ///
    /// Infallible with respect to activation, and necessarily so: nothing has
    /// been exposed yet, so there is nothing for activation to refuse.
    pub fn new(
        input_capacity: NonZeroUsize,
        control_acknowledgements: SyncSender<XAuthorityClientControlAck>,
        input_deliveries: std::sync::mpsc::Sender<XAuthorityClientInputDelivery>,
        gate: crate::ControlEpochGate,
    ) -> Self {
        let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
            input_capacity,
            control_acknowledgements,
            input_deliveries,
        );
        broker
            .try_install_control_gate(gate)
            .expect("a broker built here has exposed nothing to refuse over");
        // Room for a full ingress round plus the classes that arrive beside
        // it, with a share kept back so cleanup is never the thing that cannot
        // be admitted.
        //
        // Sized above what the bounded channels can hold at once, which is why
        // the refusal path below is not reachable from a single source today.
        // It is kept correct rather than removed, because producer-side
        // admission will make it reachable: a producer refused at send has to
        // be handed its payload back, and nothing may be taken from a channel
        // that cannot then be placed.
        let capacity = input_capacity
            .get()
            .saturating_mul(2)
            .saturating_add(PRIVATE_CLEANUP_RESERVE);
        let staged = crate::ReadyStream::new(
            NonZeroUsize::new(capacity).expect("a doubled non-zero capacity is non-zero"),
            PRIVATE_CLEANUP_RESERVE,
        )
        .expect("a reserve smaller than the capacity it was added to");
        Self {
            broker,
            admission: Arc::new(SharedAdmission::new(staged)),
            service_budget: capacity,
        }
    }

    /// Submit work, and be told why if it is not accepted.
    ///
    /// Reservation happens before acceptance and is rolled back exactly when
    /// acceptance fails, so a refusal leaves no reservation behind and no
    /// other request's delivery is disturbed: only this envelope's own id is
    /// aborted, and only when this envelope was the one that failed.
    pub fn submit(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<crate::ReadySequence, PrivateSendError> {
        self.ingress().submit(route)
    }

    /// The stamped ingress, as a private handle.
    ///
    /// Not the ordinary sender. Handing that out left `try_send` reachable
    /// from a private frontend, and it reports a policy denial as `Full`, so
    /// claiming every private producer error is typed would have been false
    /// while that escape existed.
    pub fn ingress(&self) -> PrivateIngress {
        PrivateIngress {
            sender: self.broker.routed_input_sender(),
            admission: Arc::clone(&self.admission),
        }
    }

    /// A producer handle for control, bound to this instance's admission.
    ///
    /// A second real producer class, so the shared order is something two
    /// producers actually contend for rather than one producer's queue with a
    /// new name.
    pub fn control_producer(&self) -> PrivateControlProducer {
        PrivateControlProducer {
            admission: Arc::clone(&self.admission),
        }
    }

    /// Run what producers have accepted, in the order they accepted it.
    ///
    /// There is no collection pass. Producers admit into the shared order as
    /// their work becomes runnable, so position is already decided by the time
    /// this runs; a pass that gathered from per-source channels would decide
    /// the interleaving here instead, and would decide it by which source it
    /// visited first.
    ///
    /// Raw ingress is not a source. It carries no stamp, and a private
    /// instance will not hand out a handle to one.
    /// Returns what it ran, in the order it took them.
    ///
    /// The order is a return value rather than a count, because a caller that
    /// can only see how many ran cannot tell an ordered consumer from one that
    /// grouped entries someone else had already numbered.
    pub fn route_pending(&mut self) -> Result<Vec<PrivateRun>, XServerFrontendRouteError> {
        // Bounded by what the queue can hold, not by when producers stop.
        // Draining until empty lets a producer that keeps replenishing hold
        // this turn open and grow the report without limit, which is an
        // unbounded allocation added to production to satisfy a test.
        let budget = self.service_budget;
        let mut ran = Vec::with_capacity(budget);
        while ran.len() < budget {
            let next = self
                .admission
                .take_next()
                .map_err(|()| XServerFrontendRouteError::RegistryPoisoned)?;
            let Some((sequence, class, operation)) = next else {
                break;
            };
            let identity = PrivateIdentity::of(&operation);
            self.run_one(operation)?;
            // Pushed into capacity taken before the effect, so recording never
            // allocates after something has already happened.
            ran.push(PrivateRun {
                sequence,
                class,
                identity,
            });
        }
        Ok(ran)
    }

    /// Run one operation, whichever order it came from.
    fn run_one(&mut self, operation: PrivateOperation) -> Result<(), XServerFrontendRouteError> {
            match operation {
                PrivateOperation::LeaseRelease(release) => {
                    self.broker.registry.release_route_lease(release)?;
                }
                PrivateOperation::RoutedInput(route) => {
                    let admitted = self
                        .broker
                        .control_gate
                        .get()
                        .is_some_and(|gate| {
                            gate.admits(crate::ControlStamp {
                                control_epoch: route.control_epoch,
                                publication: route.publication,
                            })
                            .is_ok()
                        });
                    self.broker.registry.route_engine_input_admitted(
                        route.route,
                        crate::ControlStamp {
                            control_epoch: route.control_epoch,
                            publication: route.publication,
                        },
                        admitted,
                    )?;
                }
                PrivateOperation::Control(control) => {
                    self.broker.registry.route_control(control)?;
                }
            }
        Ok(())
    }
}
