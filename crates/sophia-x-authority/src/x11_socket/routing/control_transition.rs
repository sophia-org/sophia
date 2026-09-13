// The control-transition surface a broker under a coordinator exposes.
//
// Split from the broker so no file has to grow past what the layout gate
// allows; the split is by subject, so what a control transition produces and
// how it is applied and reported stay together. Producer-side admission is
// next door, in private_admission.rs.

/// How much of a private host's ready capacity is kept for cleanup.
#[cfg(unix)]
const PRIVATE_CLEANUP_RESERVE: usize = 4;

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

/// What a private frontend is built from.
///
/// A bundle rather than loose arguments, so a refusal can hand them back
/// intact. A constructor that consumed them on the way to declining would
/// leave a caller unable to try again with what it already had.
#[cfg(unix)]
pub struct PrivateFrontendParts {
    pub input_capacity: NonZeroUsize,
    pub control_acknowledgements: SyncSender<XAuthorityClientControlAck>,
    pub input_deliveries: std::sync::mpsc::Sender<XAuthorityClientInputDelivery>,
    /// The authority this frontend executes against, owned rather than named.
    ///
    /// Taken rather than a gate, because a gate is derived from an authority
    /// and pairing one with a different authority makes the two disagree about
    /// which identity is being driven. The gate this frontend installs is
    /// built here from this instance, so the identity it serves and the
    /// identity that executes are the same by construction rather than by
    /// a check somebody has to remember to write.
    pub authority: sophia_input_authority::AuthorityInstance,
    /// Issuer rights over that authority: transitions, cleanup and the
    /// issuer-owned state paths. Distinct from submission.
    pub issuer: sophia_input_authority::IssuerHandle,
    /// Submission rights: reserving a request before it is enqueued. A
    /// reservation is not an issuer act, and cleanup is not a submission, so
    /// the two are carried separately rather than inferred from each other.
    pub submit: sophia_input_authority::SubmitHandle,
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
    /// Where obligations go if a settlement handle is abandoned.
    durable: PrivateSettlementOwner,
    /// Per-operation completion records for control accepted here.
    completion: ControlCompletionRegistry,
    /// Whether this instance still holds its failure slot.
    ///
    /// Released when the instance closes without failing, or handed over with
    /// the queue when it does. Holding it past either would leak a slot that
    /// another instance could have used.
    failure_slot_held: bool,
    /// Work that has been routed but not yet terminally answered.
    ///
    /// A credit belongs to its work until the work reaches a real terminal
    /// outcome. Releasing when the consumer takes an operation frees it while
    /// a client writer still holds the command, so capacity would be handed to
    /// new work on the strength of something that has not happened.
    outstanding: Vec<PrivateIdentity>,
    /// The authority this frontend executes against.
    ///
    /// Owned, not borrowed from a caller. Execution needs `&mut` to it and the
    /// gate was derived from it, so the instance that stamps and the instance
    /// that applies cannot drift apart.
    ///
    /// Held before the execution path reads it. Construction is what has to
    /// own this -- a frontend that acquired an authority later could have
    /// stamped work under a coordinator describing something else first --
    /// so it is kept from the moment the instance exists rather than from the
    /// moment it is first used.
    ///
    /// Behind the controller rather than in hand, because reserving happens at
    /// a detached producer and needs the same instance this executes against.
    /// One authoritative instance, several roles over it.
    controller: PrivateAuthorityController,
    /// Where admission and revocation cross into this boundary.
    ///
    /// Built with the instance, before any ingress exists, so there is no
    /// interval in which work could be accepted for a client this boundary
    /// never admitted.
    participant: PrivateAdmissionParticipant,
    /// Submission rights: reserving a request before it is enqueued.
    ///
    /// Kept here so the instance can hand a producer a reservation role bound
    /// to this authority. Held rather than used directly: the frontend is the
    /// executor, and executing is not submitting.
    submit: sophia_input_authority::SubmitHandle,
    /// Where each hold this executor began was delivered.
    ///
    /// Recorded when a press begins a hold and read when one ends. A release
    /// answers to what the press reached, and that is a fact from the moment
    /// of the press: resolving it again would describe wherever the route
    /// points now, which is a different client the moment a grab or a surface
    /// has moved.
    /// Storage is reserved before any work can be accepted, so publishing a
    /// hold's plan cannot fail after the ledger has already moved.
    holds: Vec<(u64, PrivateReachedResources)>,
    /// Holds whose release has been decided and not yet handed on.
    ///
    /// An event having been built is not an event having been delivered, so
    /// the plan moves here rather than being dropped: this is the only record
    /// of who is owed one, and the terminal handoff is what clears it.
    settling: Vec<(u64, PrivateReachedResources)>,
    /// Whether this instance has already handed out its keyboard state.
    ///
    /// One history per instance, so the answer is asked and answered once.
    keyboards_issued: std::sync::atomic::AtomicBool,
    /// Whether this instance's queue was unreadable when it closed.
    ///
    /// Remembered rather than recomputed. Settlement runs once, so asking a
    /// second time answers about a closed queue rather than about what
    /// happened, and Drop would conclude the instance never failed and release
    /// a slot that is still in use.
    failed: bool,
    /// Whether settlement has already run.
    ///
    /// shutdown consumes the frontend, so Drop still follows it. Without this
    /// the instance settles twice, which double-counts an unreadable queue and
    /// would double-answer anything a second pass could reach.
    settled: bool,
}

/// What settling the abandoned operations found.
#[cfg(unix)]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ControlReconcileReport {
    /// Nothing reachable is left disagreeing, so nothing is owed. Not a
    /// statement about what the operation did.
    pub discharged: usize,
    /// It finished changing the shared runtime and never began projecting it,
    /// so the projection it left behind does not agree.
    pub retained_half_applied: usize,
    /// A step had begun and is not known to have finished. The effect may have
    /// happened, which is not the same as knowing it did not.
    pub retained_in_progress: usize,
    /// Nothing establishes what it left, so it keeps its obligation.
    pub retained_unproved: usize,
    /// Whether the records could be read at all. Finding nothing because
    /// nothing could be looked at is not finding nothing.
    pub readable: bool,
}

/// One thing the private host has to run, in the order it was admitted.
///
/// Carried in a shutdown report so an owner can act on what it is handed, but
/// crate-visible: publishing it would export the stamped envelope shape for
/// the sake of a report.
// Routed input is much the larger variant, and boxing it would put an
// allocation on the admission path. The order reserves its storage before
// anything is accepted precisely so that accepting never allocates, and a
// refusal hands the payload back rather than dropping it -- both of which a
// box would undo at exactly the wrong moment.
#[cfg(unix)]
#[allow(clippy::large_enum_variant)]
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
    /// Control whose application belongs in this order, with the completion
    /// registration made before it was accepted.
    Control(XAuthorityClientControlCommand, Option<ControlCompletionToken>),
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Build a frontend that is private from the moment it exists.
    ///
    /// Fallible: a failure slot is reserved before anything is exposed, so an
    /// instance that could not hand over its queue if it failed is never
    /// built. Activation itself cannot refuse here, because nothing has been
    /// exposed for it to refuse over.
    ///
    /// A refusal returns the inputs it was given. They may be the caller's
    /// only handles, so consuming them would mean a caller could not retry the
    /// same construction -- refusing would then cost more than the instance it
    /// declined to build.
    // The refusal carries the parts back, and the parts now include an
    // authority, so the error is large. Boxing it would mean a caller that was
    // refused has to unwrap an allocation to get its own handles back, and the
    // allocation would happen on the path where something already went wrong.
    // The size is the cost of handing the caller everything it gave us.
    #[allow(clippy::result_large_err)]
    pub fn new(
        parts: PrivateFrontendParts,
        durable: &PrivateSettlementOwner,
    ) -> Result<Self, (AdmissionRefusal, PrivateFrontendParts)> {
        // Before anything is exposed, and before the parts are taken apart.
        if let Err(refusal) = durable.reserve_failure_slot() {
            return Err((refusal, parts));
        }
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
        let capacity = parts
            .input_capacity
            .get()
            .saturating_mul(2)
            .saturating_add(PRIVATE_CLEANUP_RESERVE);
        // Also before the parts are taken apart. A registry that could not
        // take an unused origin would issue identities another live instance
        // already answers to, and refusing after the senders were consumed
        // would cost the caller the handles it would need to retry.
        let Some(completion) = ControlCompletionRegistry::with_capacity(capacity) else {
            durable.release_failure_slot();
            return Err((AdmissionRefusal::Exhausted, parts));
        };
        // Derived from the authority this frontend owns, before the parts are
        // taken apart so a refusal can hand them all back. An authority that
        // cannot report its published revision cannot be driven, and building
        // a coordinator around a guess would leave the gate naming a state the
        // authority never published.
        let coordinator =
            match crate::ControlEpochCoordinator::derive(&parts.authority, &parts.issuer) {
                Ok(coordinator) => coordinator,
                Err(_) => {
                    durable.release_failure_slot();
                    return Err((AdmissionRefusal::AuthorityUnreadable, parts));
                }
            };
        let gate = crate::ControlEpochGate::new(coordinator);
        // Checked here too, and separately: derive proves the authority can
        // report a revision, not that this issuer answers for it. A controller
        // built over a mismatched pair accepts reservations and then cannot
        // dispose them.
        let controller = match PrivateAuthorityController::new(parts.authority, parts.issuer) {
            Ok(controller) => controller,
            Err((_refusal, authority, issuer)) => {
                durable.release_failure_slot();
                return Err((
                    AdmissionRefusal::AuthorityUnreadable,
                    PrivateFrontendParts {
                        authority,
                        issuer,
                        ..parts
                    },
                ));
            }
        };
        let PrivateFrontendParts {
            input_capacity,
            control_acknowledgements,
            input_deliveries,
            submit,
            ..
        } = parts;
        let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
            input_capacity,
            control_acknowledgements,
            input_deliveries,
        );
        broker
            .try_install_control_gate(gate)
            .expect("a broker built here has exposed nothing to refuse over");
        let staged = crate::ReadyStream::new(
            NonZeroUsize::new(capacity).expect("a doubled non-zero capacity is non-zero"),
            PRIVATE_CLEANUP_RESERVE,
        )
        .expect("a reserve smaller than the capacity it was added to");
        // Installed before the instance exists, so no client can register a
        // writer that would report its outcomes nowhere.
        assert!(
            broker.registry.install_control_completion(completion.clone()),
            "a freshly built broker has no completion registry yet"
        );
        Ok(Self {
            broker,
            admission: Arc::new(SharedAdmission::new(staged, durable.clone())),
            completion,
            service_budget: capacity,
            durable: durable.clone(),
            outstanding: Vec::with_capacity(capacity),
            settled: false,
            failed: false,
            failure_slot_held: true,
            participant: PrivateAdmissionParticipant::new(controller.clone()),
            controller,
            submit,
            keyboards_issued: std::sync::atomic::AtomicBool::new(false),
            holds: Vec::with_capacity(capacity),
            settling: Vec::with_capacity(capacity),
        })
    }

    /// The gate this frontend derived from the authority it owns.
    ///
    /// Handed out rather than taken in. A caller that needs to stamp or drive
    /// a transition uses the one this instance is actually running under; a
    /// caller that built its own would be naming a different coordinator.
    pub fn control_gate(&self) -> &crate::ControlEpochGate {
        self.broker
            .control_gate
            .get()
            .expect("a private frontend installs its gate at construction")
    }

    /// The keyboard state for this instance's executing thread.
    ///
    /// Handed out **once**. The state is the seat's history, and a second one
    /// would carry this instance's identity, pass every check that identity
    /// answers, and hold none of the keys the first is holding -- so a key
    /// down in the first would be a key nobody released as far as the second
    /// could tell. Identity equality cannot tell those two apart, which is why
    /// uniqueness is established here rather than checked later.
    ///
    /// Refuses for three different reasons and says which. An authority that
    /// cannot be read is not a keymap that will not compile, and reporting
    /// either as the other would send someone to look in the wrong place.
    pub fn keyboards(&self) -> Result<PrivateKeyboards, PrivateKeyboardsRefusal> {
        let authority = self
            .controller
            .identity()
            .map_err(|_| PrivateKeyboardsRefusal::AuthorityUnreadable)?;
        // Claimed before the state is built, so a build that fails does not
        // leave the instance thinking it has issued one -- and two callers
        // racing here cannot both come away with a history.
        if self.keyboards_issued.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return Err(PrivateKeyboardsRefusal::AlreadyIssued);
        }
        match PrivateKeyboards::for_instance(authority, crate::XkbRmlvoConfig::default()) {
            Ok(keyboards) => Ok(keyboards),
            Err(refusal) => {
                // Nothing was handed out, so the claim is given back. An
                // instance that failed to build its state has not issued one.
                self.keyboards_issued
                    .store(false, std::sync::atomic::Ordering::Release);
                Err(refusal)
            }
        }
    }

    /// Where admission and revocation reach this boundary.
    ///
    /// Handed to whoever performs revocation. Taking it is not a right over
    /// the authority: it admits and revokes, and the rights over the instance
    /// itself stay with the origin.
    pub fn admission_participant(&self) -> &PrivateAdmissionParticipant {
        &self.participant
    }

    /// The one authority this instance executes against.
    ///
    /// Role-limited: what a caller can do with it depends on which method it
    /// reaches for, not on holding the instance.
    pub fn authority(&self) -> &PrivateAuthorityController {
        &self.controller
    }

    /// Execute one reserved request in the order the ranks require.
    ///
    /// Common first, then the boundary's own bindings beneath it, and the
    /// execution happens before either is released. An execution already under
    /// way therefore holds common, which is what makes a revocation wait for
    /// it rather than race it, and a revocation that has returned cannot be
    /// overtaken by a later attempt.
    ///
    /// The evidence is the binding the admission producer maintains here, not
    /// a copy of an admission context taken when the client registered with
    /// this frontend. That copy said what was admitted once, and a client
    /// revoked upstream stayed registered behind it.
    ///
    /// Refuses when nothing is bound, which is the same answer for a client
    /// this boundary never admitted and one whose admission has been revoked:
    /// in both cases nothing here answers for the work.
    #[cfg_attr(not(test), allow(dead_code))]
    fn execute_ordered(
        &self,
        outstanding: &PrivateOutstandingRequest,
        client: XServerFrontendClientId,
        act: impl FnOnce(
            &mut sophia_input_authority::ExecutionPermit<'_>,
            &PrivateAdmissionBindings,
        ) -> Result<(), sophia_input_authority::RegistrationError>,
    ) -> Result<sophia_input_authority::RequestCompletion, PrivateAuthorityRefusal> {
        match self.participant.execute_current(outstanding, client, act) {
            Ok(completion) => completion,
            // Unreadable is not absent. A boundary nobody can read established
            // nothing about who is admitted, and answering that with "not
            // admitted" tells a caller a decision was made when none was.
            Err(PrivateAdmissionRefusal::Unreachable) => Err(PrivateAuthorityRefusal::Unreachable),
            // The authority's own refusal is kept as its own, not relabelled
            // as a question about admission.
            Err(PrivateAdmissionRefusal::Authority(error)) => {
                Err(PrivateAuthorityRefusal::Authority(error))
            }
            Err(PrivateAdmissionRefusal::NotAdmitted)
            | Err(PrivateAdmissionRefusal::DifferentAdmission)
            | Err(PrivateAdmissionRefusal::AlreadyAdmitted)
            | Err(PrivateAdmissionRefusal::GrantRecordsExhausted) => {
                Err(PrivateAuthorityRefusal::NoCurrentAdmission)
            }
        }
    }

    /// A reservation role for one admitted connection, bound to this
    /// instance's authority.
    ///
    /// This is what a detached producer gets: the right to reserve its own
    /// request and to observe its own completion, against a capability issued
    /// here. Not the authority, not the issuer, and not a closure over either
    /// -- a producer holding any of those could revoke and reissue the grant
    /// its own request was validated against.
    ///
    /// Issuing the capability is an origin act, so it happens on this side of
    /// the handover rather than being something the producer asks for.
    pub fn reservation_role(
        &self,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateReservationRole, PrivateAdmissionRefusal> {
        // Issued only against a live binding, and recorded on it. A capability
        // issued for a client this boundary has not admitted would be a grant
        // nothing could later revoke, because revocation retires what an
        // admission authorised rather than sweeping the authority.
        self.participant
            .issue_role(self.submit, client, device)
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
            role: None,
            requests: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// The stamped ingress for one admitted producer, reserving its requests
    /// against this instance's authority before they are published.
    ///
    /// This is the production shape. The capability is issued here, on the
    /// origin's side of the handover, and what the producer receives is the
    /// right to reserve and to observe its own outcomes -- never the authority
    /// and never the issuer.
    pub fn ingress_for(
        &self,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateIngress, PrivateAdmissionRefusal> {
        Ok(PrivateIngress {
            sender: self.broker.routed_input_sender(),
            admission: Arc::clone(&self.admission),
            role: Some(self.reservation_role(client, device)?),
            requests: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        })
    }

    /// A producer handle for control, bound to this instance's admission.
    ///
    /// A second real producer class, so the shared order is something two
    /// producers actually contend for rather than one producer's queue with a
    /// new name.
    pub fn control_producer(&self) -> PrivateControlProducer {
        PrivateControlProducer {
            admission: Arc::clone(&self.admission),
            completion: self.completion.clone(),
            routing: self.broker.registry.clone(),
        }
    }

    /// Settle what each abandoned operation is owed, while this instance is
    /// still live.
    ///
    /// The same reconciliation the settlement and the durable owner apply, so
    /// a proof established here does not stop being applied when this frontend
    /// is consumed.
    pub fn reconcile_abandoned(&mut self) -> ControlReconcileReport {
        self.completion.reconcile_unstarted()
    }

    /// Republish acknowledgements a client writer could not deliver.
    ///
    /// Republishing only: those effects already happened, so nothing here is
    /// re-run. Returns how many reached the receiver. What does not is kept,
    /// because a retry that consumed the outcome it failed to publish would
    /// lose the only record of what happened.
    pub fn republish_owed_acknowledgements(&self) -> usize {
        let sender = &self.broker.registry.acknowledgement_sender;
        self.completion
            .publish_owed_with(|acknowledgement| match sender.try_send(*acknowledgement) {
                Ok(()) => ControlPublication::Delivered,
                Err(TrySendError::Disconnected(_)) => ControlPublication::ReceiverGone,
                Err(TrySendError::Full(_)) => ControlPublication::Retained,
            })
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
    /// Release credits for work that has reached a terminal outcome.
    ///
    /// Returns how many were reclaimed. Input is observable: the recovery
    /// ledger holds a ticket while a delivery is live and drops it when the
    /// delivery ends, however it ended, so a credit is released exactly when
    /// its work is answered rather than when it was handed on.
    ///
    /// Control is observable through its registration. The acknowledgement
    /// goes to a receiver this frontend does not hold, so the send itself
    /// cannot be watched, but the registry it is reported to is this
    /// instance's: a retired record means an outcome was reached, and only
    /// then is the credit released. Accepted, applying and owed all keep it.
    pub fn reclaim_settled(&mut self) -> usize {
        let recovery = &self.broker.registry.input_recovery;
        let before = self.outstanding.len();
        self.outstanding.retain(|identity| match identity {
            PrivateIdentity::Delivery(Some(delivery)) => {
                match recovery.delivery_state(*delivery) {
                    DeliveryState::Ended => false,
                    // Live, and equally: a ledger that cannot be read tells us
                    // nothing, and reading silence as completion would free a
                    // credit for work that is still outstanding.
                    DeliveryState::Live | DeliveryState::Unavailable => true,
                }
            }
            // No public delivery id means nothing to observe here, which is
            // not the same as nothing outstanding: the work can still be
            // writer-pending or frozen. Held until an internal completion
            // record can answer for it.
            PrivateIdentity::Delivery(None) => true,
            PrivateIdentity::Control {
                completion: Some(token),
                ..
            } => match self.completion.state_of(*token) {
                // An outcome was reached and the record retired, so this
                // credit is released here and cannot be released again: the
                // identity leaves `outstanding` with it.
                ControlRecordState::Retired => false,
                // Still owed an outcome, or a registry that cannot answer.
                // Neither is a completion.
                ControlRecordState::Outstanding | ControlRecordState::Unanswerable => true,
            },
            // No registration means nothing to observe, which is not the same
            // as nothing outstanding.
            PrivateIdentity::Control { completion: None, .. } | PrivateIdentity::Lease(_) => true,
        });
        let reclaimed = before.saturating_sub(self.outstanding.len());
        for _ in 0..reclaimed {
            self.durable.release();
        }
        reclaimed
    }

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
            match self.run_one(operation) {
                // Routed or enqueued, which is not the same as answered. The
                // credit stays with the work until its real terminal outcome,
                // because route_control only hands a command to a client
                // writer and routed input can sit writer-pending or frozen.
                Ok(()) => self.outstanding.push(identity),
                Err(error) => {
                    // The operation is consumed by now, so its credit has
                    // nothing left to travel with. Recorded as outstanding
                    // rather than released, so a failure cannot look like a
                    // completion and free capacity for new work.
                    self.outstanding.push(identity);
                    return Err(error);
                }
            }
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
                PrivateOperation::Control(control, completion) => {
                    self.broker
                        .registry
                        .route_control_with_completion(control, completion)?;
                }
            }
        Ok(())
    }
}
