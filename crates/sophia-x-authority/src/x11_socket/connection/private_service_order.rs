// How the private routed service reaches the order it serves: the adapter
// the shared loop calls, the public broker's implementation of it, the
// leased private implementation over the prepared runner and the producer
// port, and the tally an invocation reports.
//
// Split by subject from `private_service.rs`, which keeps the loop, the
// collection guard and the entry points.

/// How the routed loop reaches its broker.
///
/// The public service owns its broker and reaches it directly. The private
/// service reaches it through the lease, and a reach that the lease does not
/// cover is refused rather than performed.
#[cfg(unix)]
trait RoutedBrokerAccess {
    fn broker(&mut self) -> Result<&XServerFrontendRouteBroker, X11SetupSocketError>;
    /// Serve the accepted order once, bounded: the public broker routes what
    /// is pending; the private service takes one turn on its prepared
    /// runner. Answers how much moved.
    fn serve_order(&mut self) -> Result<usize, X11SetupSocketError>;
    /// Start a registered worker for every ready connection that has none.
    ///
    /// THE PUBLIC PATH HAS NONE: nothing is registered there and nothing is
    /// started. The private path visits from the service frame, which is the
    /// one place holding the checked lease and the frontend together.
    fn attach_ready(&mut self) -> Result<usize, X11SetupSocketError>;
    /// Answer the producer requests waiting at the port, if this service has
    /// one. The public broker has none.
    fn answer_producers(&mut self) -> Result<usize, X11SetupSocketError>;
    /// End private issuance and acceptance at a cancelling stop decision,
    /// before reporting cancellation or stopping connection workers. Accepted
    /// work stays with its existing owners. Safe to repeat; the public broker
    /// has no private producers to close.
    fn close_private_producers(&mut self);
}

#[cfg(unix)]
impl RoutedBrokerAccess for XServerFrontendRouteBroker {
    fn broker(&mut self) -> Result<&XServerFrontendRouteBroker, X11SetupSocketError> {
        Ok(self)
    }
    fn serve_order(&mut self) -> Result<usize, X11SetupSocketError> {
        XServerFrontendRouteBroker::route_pending(self)
            .map_err(|error| X11SetupSocketError::new(error.to_string()))
    }
    fn attach_ready(&mut self) -> Result<usize, X11SetupSocketError> {
        Ok(0)
    }
    fn answer_producers(&mut self) -> Result<usize, X11SetupSocketError> {
        Ok(0)
    }
    fn close_private_producers(&mut self) {}
}

/// What one invocation's order did, over every turn, for the owner to read
/// beside its workers and maintenance: taken, decided, routed, delivered and
/// settled counts, the turns an allowance refused or an entry blocked, the
/// supervisor failures seen, and the producers issued and refused at the
/// port. Counts, not evidence: the exact work is in the store and the homes.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrivateOrderTally {
    pub turns: usize,
    pub taken: usize,
    pub refused: usize,
    pub routed: usize,
    pub reclaimed: usize,
    pub reclaim_observed: usize,
    pub reclaim_refusals: usize,
    pub reclaim_unwatched: usize,
    pub terminal_steps: usize,
    pub dispatched: usize,
    pub recipient_settled: usize,
    pub activation_pairs_observed: usize,
    pub activations_joined: usize,
    pub native_disposal_observed: usize,
    pub native_disposed: usize,
    pub allowance_refusals: usize,
    pub blocked_turns: usize,
    pub unwatched_turns: usize,
    pub watch_failures: usize,
    pub producers_issued: usize,
    pub producers_refused: usize,
    /// Why the last refused execution was refused, if any was.
    pub(crate) last_refusal: Option<PrivateExecutionRefusal>,
}

#[cfg(unix)]
impl PrivateOrderTally {
    fn record(&mut self, progress: &PrivateRunnerProgress) {
        self.turns += 1;
        self.taken += progress.taken;
        self.refused += progress.refused;
        self.routed += progress.routed;
        self.reclaimed += progress.reclaimed;
        self.reclaim_observed += progress.reclaim_observed;
        self.reclaim_refusals += usize::from(progress.reclaim_refusal.is_some());
        self.reclaim_unwatched += usize::from(progress.reclaim_unwatched);
        self.terminal_steps += progress.terminal_steps;
        self.dispatched += progress.dispatched;
        self.recipient_settled += progress.recipient_settled;
        self.activation_pairs_observed += progress.activation_pairs_observed;
        self.activations_joined += progress.activations_joined;
        self.native_disposal_observed += progress.native_disposal_observed;
        self.native_disposed += progress.native_disposed;
        self.allowance_refusals += usize::from(progress.allowance.is_some());
        self.blocked_turns += usize::from(progress.blocked.is_some());
        self.unwatched_turns += usize::from(progress.unwatched.is_some());
        self.watch_failures += usize::from(progress.watch_failed);
        if progress.last_refusal.is_some() {
            self.last_refusal = progress.last_refusal;
        }
    }
}

/// The loop's reach into the prepared runner, checked against the lease.
///
/// THE RUNNER IS BORROWED FROM THE COLLECTION GUARD that owns it for the
/// invocation, so nothing here can move it off the service thread or
/// finalise it. A cancelling loop exit closes its producers through this
/// adapter; the guard repeats that closure before collection on every exit.
#[cfg(unix)]
struct LeasedPrivateBroker<'a, 'o> {
    runner: &'a mut PrivatePreparedRunner,
    port: &'a mut PrivateProducerPort,
    order: &'a mut PrivateOrderTally,
    service: &'a PrivateServiceLease<'o>,
}

#[cfg(unix)]
impl LeasedPrivateBroker<'_, '_> {
    fn check(&self) -> Result<(), X11SetupSocketError> {
        if self.runner.frontend().broker.registry.leased_by(self.service) {
            Ok(())
        } else {
            Err(X11SetupSocketError::new(
                "private service lease is not on the owner that keeps this frontend's registry",
            ))
        }
    }
}

#[cfg(unix)]
impl RoutedBrokerAccess for LeasedPrivateBroker<'_, '_> {
    fn broker(&mut self) -> Result<&XServerFrontendRouteBroker, X11SetupSocketError> {
        self.check()?;
        Ok(&self.runner.frontend().broker)
    }
    /// ONE BOUNDED TURN ON THE PREPARED RUNNER, under its own budget and
    /// watchdog: the one consumer of the accepted order. The unprepared
    /// route is not reached on this path (and would refuse, the runner's
    /// producer being exposed). An allowance refusal or a blocked entry is a
    /// turn that moved nothing, not an error; a route error is the loop's
    /// error, as the unprepared route's was.
    fn serve_order(&mut self) -> Result<usize, X11SetupSocketError> {
        #[cfg(all(test, unix))]
        routing_tests::m3_acceptance::before_service_turn(self.runner, self.service);
        let progress = self
            .runner
            .service_turn(self.service)
            .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
        self.order.record(&progress);
        #[cfg(all(test, unix))]
        routing_tests::m3_acceptance::after_service_turn(&self.runner.frontend().broker.registry, &progress);
        Ok(usize::from(PrivatePreparedRunner::advanced(&progress)))
    }
    fn attach_ready(&mut self) -> Result<usize, X11SetupSocketError> {
        self.check()?;
        Ok(attach_ready_workers(self.runner.frontend(), self.service))
    }
    fn answer_producers(&mut self) -> Result<usize, X11SetupSocketError> {
        self.check()?;
        let (issued, refused) = self.port.answer(self.runner, self.service);
        self.order.producers_issued += issued;
        self.order.producers_refused += refused;
        Ok(issued + refused)
    }
    fn close_private_producers(&mut self) {
        self.port.close();
        self.runner.close_admission();
        #[cfg(all(test, unix))]
        routing_tests::stage_after_admission_closed(&self.runner.frontend().broker.registry);
    }
}
