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
    ready: crate::ReadyStream<PrivateOperation>,
}

/// One thing the private host has to run, in the order it was admitted.
#[cfg(unix)]
enum PrivateOperation {
    /// Input admitted through the stamped envelope.
    RoutedInput(XAuthorityEpochRoutedInput),
    /// A route lease being retired. Privileged cleanup.
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
        let capacity = input_capacity
            .get()
            .saturating_mul(2)
            .saturating_add(PRIVATE_CLEANUP_RESERVE);
        let ready = crate::ReadyStream::new(
            NonZeroUsize::new(capacity).expect("a doubled non-zero capacity is non-zero"),
            PRIVATE_CLEANUP_RESERVE,
        )
        .expect("a reserve smaller than the capacity it was added to");
        Self { broker, ready }
    }

    /// The stamped ingress. There is no unstamped one.
    pub fn routed_input_sender(&self) -> XAuthorityRoutedInputSender {
        self.broker.routed_input_sender()
    }

    /// Admit everything runnable into one order, then run it in that order.
    ///
    /// Two passes rather than five loops. The first takes what is available
    /// from each source and gives each item its position as it is admitted, so
    /// the interleaving is decided once. The second runs them in that order,
    /// and nothing jumps.
    ///
    /// Raw ingress is not among the sources. It carries no stamp, and a
    /// private instance refuses to expose a handle to it in the first place.
    pub fn route_pending(&mut self) -> Result<usize, XServerFrontendRouteError> {
        self.admit_runnable();
        self.run_admitted()
    }

    /// Take what each source has ready, in one pass.
    ///
    /// A source that cannot be admitted keeps its item rather than losing it:
    /// the stream hands a refused payload back, and it is offered again on the
    /// next pass instead of being dropped here.
    fn admit_runnable(&mut self) {
        while let Ok(release) = self.broker.route_lease_release_receiver.try_recv() {
            if self
                .ready
                .admit(crate::ReadyClass::Cleanup, PrivateOperation::LeaseRelease(release))
                .is_err()
            {
                break;
            }
        }
        while let Ok(route) = self.broker.routed_input_receiver.try_recv() {
            if self
                .ready
                .admit(
                    crate::ReadyClass::RoutedInput,
                    PrivateOperation::RoutedInput(route),
                )
                .is_err()
            {
                break;
            }
        }
        while let Ok(control) = self.broker.control_receiver.try_recv() {
            if self
                .ready
                .admit(crate::ReadyClass::Control, PrivateOperation::Control(control))
                .is_err()
            {
                break;
            }
        }
    }

    /// Run what was admitted, in the order it was admitted.
    fn run_admitted(&mut self) -> Result<usize, XServerFrontendRouteError> {
        let mut ran = 0usize;
        while let Some((_, _, operation)) = self.ready.take_next() {
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
            ran = ran.saturating_add(1);
        }
        Ok(ran)
    }
}
