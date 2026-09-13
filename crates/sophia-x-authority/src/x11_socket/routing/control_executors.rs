// Who can still act for a client, and what that licenses.
//
// Split from the completion records by subject: the records are what is owed
// for each operation, and this is what is still able to establish it. Keeping
// them apart is also why abandoning is not a guess -- the answer is read from
// here, under the lock that abandons.

/// What can still act for one client.
///
/// Two different questions, and they do not have the same answer. Whether
/// anything could still establish what happened to an operation already in
/// flight is not whether anything could start a new one.
#[cfg(unix)]
#[derive(Default)]
struct ControlExecutors {
    /// The client is registered and no writer has started yet.
    ///
    /// Registration comes before the spawn, and control accepted in that
    /// window is not control with nowhere to go. Cleared when a writer starts,
    /// and cleared when the registration goes without one ever having started.
    expected: bool,
    /// A writer has started and has not stopped.
    running: bool,
    /// Routing calls in flight. Routing is where the first authoritative
    /// effect happens, so one in flight can still establish what happened.
    routing: usize,
}

#[cfg(unix)]
impl ControlExecutors {
    /// Whether anything could still establish what happened to work already
    /// in flight.
    fn any(&self) -> bool {
        self.expected || self.running || self.routing > 0
    }

    /// Whether anything could execute work that has not started.
    ///
    /// A routing call in flight is not this. It is one operation's existence,
    /// and borrowing it to start another would let work begin for a client
    /// whose writer has gone, on the authority of an operation that has
    /// nothing to do with it.
    fn admits(&self) -> bool {
        self.expected || self.running
    }
}

/// One routing call, held for as long as it could still produce an effect.
///
/// While one is held, that client is executing, because routing itself
/// produces authoritative effects before any writer runs. Its operations are
/// not abandoned while anything can still establish what happened to them.
///
/// Counted leases cannot be built. Only `enter_routing` makes one, and only by
/// taking the count it stands for, so holding one is evidence rather than a
/// claim -- a value a caller could construct and drop would decrement a real
/// holder's count and end an operation's protection on nothing at all.
///
/// The field is private, so the value cannot be built outside this module.
/// Naming nothing else, so that this fails for that reason and not an
/// unrelated one:
///
/// ```compile_fail
/// # use sophia_x_authority::ControlExecutorLease;
/// let _ = ControlExecutorLease { held: None };
/// ```
///
/// while the public constructor is reachable:
///
/// ```
/// # use sophia_x_authority::ControlExecutorLease;
/// let _ = ControlExecutorLease::ungoverned();
/// ```
#[cfg(unix)]
#[must_use = "an executor that is not held is one nothing is waiting for"]
pub struct ControlExecutorLease {
    held: Option<ControlExecutorHold>,
}

#[cfg(unix)]
struct ControlExecutorHold {
    registry: ControlCompletionRegistry,
    client: XServerFrontendClientId,
}

#[cfg(unix)]
impl ControlExecutorLease {
    /// A lease over nothing, for a control no registry governs.
    pub fn ungoverned() -> Self {
        Self { held: None }
    }
}

#[cfg(unix)]
impl Drop for ControlExecutorLease {
    fn drop(&mut self) {
        if let Some(held) = self.held.take() {
            held.registry.leave_routing(held.client);
        }
    }
}

#[cfg(unix)]
impl ControlCompletionRegistry {
    /// Record that a client is registered and a writer is about to start.
    pub fn expect_writer(&self, client: XServerFrontendClientId) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.executors.entry(client).or_default().expected = true;
    }

    /// Record that a client's writer has started.
    pub fn writer_started(&self, client: XServerFrontendClientId) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let executors = inner.executors.entry(client).or_default();
        executors.running = true;
        executors.expected = false;
    }

    /// Record that a client's writer has stopped.
    ///
    /// Routing calls in flight are unaffected: while any is held, this client
    /// is still executing and nothing of its is abandoned.
    pub fn writer_stopped(&self, client: XServerFrontendClientId) {
        self.release_executor(client, |executors| executors.running = false);
    }

    /// Record that a client's registration has gone without a writer ever
    /// starting.
    ///
    /// A startup that failed between registration and the spawn leaves an
    /// expectation nothing will meet, and an expectation nobody cancels keeps
    /// that client executing forever -- so its operations would never reach
    /// the edge that owes them a cleanup.
    pub fn cancel_expected_writer(&self, client: XServerFrontendClientId) {
        self.release_executor(client, |executors| executors.expected = false);
    }

    /// Enter a routing call as an executor for one client, if anything could
    /// execute new work for it.
    ///
    /// `None` means nothing could. Taking the lease is the check, so there is
    /// no gap between deciding a client has an executor and being one.
    pub fn enter_routing(&self, client: XServerFrontendClientId) -> Option<ControlExecutorLease> {
        let mut inner = self.inner.lock().ok()?;
        let executors = inner.executors.get_mut(&client)?;
        if !executors.admits() {
            return None;
        }
        executors.routing = executors.routing.saturating_add(1);
        Some(ControlExecutorLease {
            held: Some(ControlExecutorHold {
                registry: self.clone(),
                client,
            }),
        })
    }

    fn leave_routing(&self, client: XServerFrontendClientId) {
        self.release_executor(client, |executors| {
            executors.routing = executors.routing.saturating_sub(1);
        });
    }

    /// Give up one kind of executor, and reconcile if it was the last.
    ///
    /// The reconciliation happens here rather than being left to whoever
    /// happens to ask next. A route returning is the last-executor edge as
    /// much as a writer exiting is, and an edge that only moves an operation
    /// to its cleanup when something else calls a sweep is not an edge.
    fn release_executor(
        &self,
        client: XServerFrontendClientId,
        release: impl FnOnce(&mut ControlExecutors),
    ) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(executors) = inner.executors.get_mut(&client) else {
            return;
        };
        release(executors);
        if executors.any() {
            return;
        }
        inner.executors.remove(&client);
        Self::abandon_unexecutable(&mut inner, client);
    }

    fn executing(inner: &ControlCompletions, client: XServerFrontendClientId) -> bool {
        inner
            .executors
            .get(&client)
            .is_some_and(ControlExecutors::any)
    }

    fn admits(inner: &ControlCompletions, client: XServerFrontendClientId) -> bool {
        inner
            .executors
            .get(&client)
            .is_some_and(ControlExecutors::admits)
    }
}
