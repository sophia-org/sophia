// Who can still act for a client, and what that licenses.
//
// Split from the completion records by subject: the records are what is owed
// for each operation, and this is what is still able to establish it. Keeping
// them apart is also why abandoning is not a guess -- the answer is read from
// here, under the lock that abandons.

/// What can still act for one client.
#[cfg(unix)]
#[derive(Default)]
struct ControlExecutors {
    /// A writer exists or is about to: the client is registered and its writer
    /// has not stopped. Registration comes before the spawn, and work accepted
    /// in that window is not work with nowhere to go.
    writer: bool,
    /// Routing calls in flight. Routing is where the first authoritative
    /// effect happens, so one in flight can still establish what happened.
    routing: usize,
}

#[cfg(unix)]
impl ControlExecutors {
    fn any(&self) -> bool {
        self.writer || self.routing > 0
    }
}

/// One routing call, held for as long as it could still produce an effect.
///
/// While one is held, that client is executing whatever its writer is doing,
/// because routing itself produces authoritative effects before any writer
/// runs. Its operations are not abandoned while anything can still establish
/// what happened to them.
#[cfg(unix)]
#[must_use = "an executor that is not held is one nothing is waiting for"]
pub enum ControlExecutorLease {
    /// No registry governs this client's control.
    Ungoverned,
    Held {
        registry: ControlCompletionRegistry,
        client: XServerFrontendClientId,
    },
}

#[cfg(unix)]
impl Drop for ControlExecutorLease {
    fn drop(&mut self) {
        if let Self::Held { registry, client } = self {
            registry.leave_routing(*client);
        }
    }
}

#[cfg(unix)]
impl ControlCompletionRegistry {
    /// Record that a client has a writer, or is registered and about to.
    ///
    /// Registration precedes the spawn, and control accepted in that window is
    /// not control with nowhere to go, so the registration is what sets this
    /// and the writer stopping is what clears it.
    pub fn expect_writer(&self, client: XServerFrontendClientId) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.executors.entry(client).or_default().writer = true;
    }

    /// Record that a client's writer has stopped.
    ///
    /// Routing calls in flight are unaffected: each holds its own lease, and
    /// while any does, this client is still executing.
    pub fn writer_stopped(&self, client: XServerFrontendClientId) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(executors) = inner.executors.get_mut(&client) else {
            return;
        };
        executors.writer = false;
        if !executors.any() {
            inner.executors.remove(&client);
        }
    }

    /// Enter a routing call as an executor for one client, if anything is
    /// executing for it.
    ///
    /// `None` means nothing is. Taking the lease is the check, so there is no
    /// gap between deciding a client has an executor and being one.
    pub fn enter_routing(&self, client: XServerFrontendClientId) -> Option<ControlExecutorLease> {
        let mut inner = self.inner.lock().ok()?;
        let executors = inner.executors.get_mut(&client)?;
        if !executors.any() {
            return None;
        }
        executors.routing = executors.routing.saturating_add(1);
        Some(ControlExecutorLease::Held {
            registry: self.clone(),
            client,
        })
    }

    fn leave_routing(&self, client: XServerFrontendClientId) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(executors) = inner.executors.get_mut(&client) else {
            return;
        };
        executors.routing = executors.routing.saturating_sub(1);
        if !executors.any() {
            inner.executors.remove(&client);
        }
    }

    fn executing(inner: &ControlCompletions, client: XServerFrontendClientId) -> bool {
        inner
            .executors
            .get(&client)
            .is_some_and(ControlExecutors::any)
    }
}
