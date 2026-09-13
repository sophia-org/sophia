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

/// Engine-facing ingress and per-client queue registry for a routed X11
/// session.
///
/// Engine code sends client-addressed input through the bounded ingress queues,
/// then its session loop calls [`Self::route_pending`] to move it into the
/// registered worker's private queue. Latency-sensitive control can instead use
/// [`Self::control_router`] to reach the selected client's bounded queue
/// directly. The broker never broadcasts a route. Routes whose client
/// disappeared after Engine selection are retired with a negative
/// acknowledgement. A client that saturates its private input queue is
/// quarantined without terminating the shared frontend; corruption of shared
/// registry state remains service-fatal.

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
