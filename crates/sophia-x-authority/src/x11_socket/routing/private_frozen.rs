// Before-effect requests remain in their original accepted order while
// controls and cleanup continue. A row never goes back through admission.

#[cfg(unix)]
struct PrivateFrozenInput {
    sequence: crate::ReadySequence,
    custody: PrivateOutstandingRequest,
    route: XAuthorityRoutedInput,
    source: Option<private_native::Freeze>,
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    fn resume_frozen(
        &mut self,
        keyboards: &mut PrivateKeyboards,
        start: &mut dyn FnMut(crate::ReadySequence, std::time::Instant) -> Result<(), XServerFrontendRouteError>,
        watch: &private_watchdog::PrivateWatchdogOwner,
    ) -> Result<PrivateOrderedStep, XServerFrontendRouteError> {
        let Some(row) = self.terminal.frozen.pop_front() else {
            return Ok(PrivateOrderedStep::Idle);
        };
        let sequence = row.sequence;
        // Only infallible moves between owned, preallocated storage occur
        // before the hook/watch. An unwind retains current plus its witness.
        self.terminal.current = Some(PrivateOrderedItem::Refused {
            sequence, custody: row.custody, route: row.route,
            refusal: PrivateExecutionRefusal::NotAttempted,
        });
        self.terminal.current_freeze = row.source;
        self.terminal.current_is_frozen = true;
        self.terminal.prefer_frozen = false;
        let now = std::time::Instant::now();
        start(sequence, now)?;
        self.execute_current_step(keyboards, watch, sequence, now)
    }

    fn execute_current_step(
        &mut self,
        keyboards: &mut PrivateKeyboards,
        watch: &private_watchdog::PrivateWatchdogOwner,
        sequence: crate::ReadySequence,
        taken_at: std::time::Instant,
    ) -> Result<PrivateOrderedStep, XServerFrontendRouteError> {
        let Ok(mut watched) = watch.begin_dequeued(taken_at) else {
            return Ok(if self.terminal.current_is_frozen {
                PrivateOrderedStep::Resumed { sequence, deferred: true, watched: false }
            } else {
                PrivateOrderedStep::Unwatched(sequence)
            });
        };
        let outcome = self.run_current(keyboards, &mut watched);
        // Check while current still owns the accepted custody. The accepted
        // store credit guarantees this bound even across grant replacement.
        assert!(self.terminal.frozen.len() < self.terminal.item_capacity);
        let Some(PrivateOrderedItem::Refused { sequence, custody, route, .. }) = self.terminal.current.take() else {
            return Err(XServerFrontendRouteError::OrderedItemUnresolved);
        };
        let resumed = self.terminal.current_is_frozen;
        self.terminal.current_is_frozen = false;
        let deferred = matches!(outcome, Ok(PrivateExecutionAttempt::Deferred));
        match outcome {
            Ok(PrivateExecutionAttempt::Deferred) => {
                let row = PrivateFrozenInput {
                    sequence, custody, route, source: self.terminal.current_freeze.take(),
                };
                if resumed { self.terminal.frozen.push_front(row); }
                else { self.terminal.frozen.push_back(row); }
            }
            Ok(PrivateExecutionAttempt::Completed(run)) => {
                self.terminal.current_freeze = None;
                self.terminal.turn.push(PrivateOrderedItem::Ran { sequence, custody, route, run });
            }
            Err(refusal) => {
                self.terminal.current_freeze = None;
                // A refusal the executor makes on the request's own terms is
                // published as the request's completion, so the grant is free
                // for the producer's next request and the delivery is answered
                // as refused on the next retirement. Any other refusal keeps
                // the request as it was, and if common cannot be reached the
                // item is kept as it is: retirement finds it without an
                // outcome, as before.
                if refusal.declines_the_request() {
                    let _published = custody.refuse_unexecuted();
                }
                self.terminal.turn.push(PrivateOrderedItem::Refused { sequence, custody, route, refusal });
            }
        }
        let watched = watched.finish().is_ok();
        if resumed {
            Ok(PrivateOrderedStep::Resumed { sequence, deferred, watched })
        } else if deferred {
            Ok(PrivateOrderedStep::Deferred { sequence, watched })
        } else if watched {
            Ok(PrivateOrderedStep::Decided(sequence))
        } else {
            Ok(PrivateOrderedStep::DecidedUnwatched(sequence))
        }
    }
}
