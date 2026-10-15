// Lease effects reserve notification custody before changing input state.
// A full owner channel retains its FIFO here; exhausting this separate bound
// refuses the effect. The caller treats that refusal as authority failure.
#[cfg(unix)]
#[derive(Clone)]
struct RouteLeaseOutbox {
    state: Arc<Mutex<RouteLeaseOutboxState>>,
}

#[cfg(unix)]
struct RouteLeaseOutboxState {
    sender: Option<sophia_wake::SignalSender<XAuthorityRouteLeaseUpdate>>,
    capacity: usize,
    pending: VecDeque<XAuthorityRouteLeaseUpdate>,
    reserved: usize,
    disconnected: bool,
    accepted: u64,
    published: u64,
    abandoned: u64,
}

#[cfg(unix)]
struct RouteLeaseReservation {
    outbox: RouteLeaseOutbox,
    live: bool,
}

#[cfg(unix)]
impl RouteLeaseOutbox {
    fn new(
        sender: Option<sophia_wake::SignalSender<XAuthorityRouteLeaseUpdate>>,
        capacity: usize,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(RouteLeaseOutboxState {
                sender,
                capacity,
                pending: VecDeque::with_capacity(capacity),
                reserved: 0,
                disconnected: false,
                accepted: 0,
                published: 0,
                abandoned: 0,
            })),
        }
    }

    fn reserve(&self) -> Result<Option<RouteLeaseReservation>, XServerFrontendRouteError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        if state.sender.is_none() {
            return Ok(None);
        }
        state.flush()?;
        // Reserve the lifetime accounting increment too: committing an effect
        // must not discover that its notification counter cannot represent it.
        let available_counts = u64::MAX - state.accepted;
        let counts_fit =
            u64::try_from(state.reserved).is_ok_and(|reserved| reserved < available_counts);
        if state.pending.len().saturating_add(state.reserved) >= state.capacity || !counts_fit {
            return Err(XServerFrontendRouteError::RouteLeaseQueueFull);
        }
        state.reserved += 1;
        Ok(Some(RouteLeaseReservation {
            outbox: self.clone(),
            live: true,
        }))
    }

    fn flush(&self) -> Result<usize, XServerFrontendRouteError> {
        self.state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .flush()
    }

}

#[cfg(unix)]
impl RouteLeaseReservation {
    fn commit(
        mut self,
        update: XAuthorityRouteLeaseUpdate,
    ) -> Result<(), XServerFrontendRouteError> {
        let mut state = self
            .outbox
            .state
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        state.reserved -= 1;
        self.live = false;
        // reserve held both queue capacity and this counter's increment.
        state.accepted += 1;
        if state.disconnected {
            state.abandoned += 1;
            return Err(XServerFrontendRouteError::RouteLeaseQueueDisconnected);
        }
        state.pending.push_back(update);
        state.flush().map(|_| ())
    }
}

#[cfg(unix)]
impl Drop for RouteLeaseReservation {
    fn drop(&mut self) {
        if self.live {
            // Even poisoned state still owns this reservation. Returning its
            // credit does not clear the poison or permit further admission.
            let mut state = self
                .outbox
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.reserved -= 1;
        }
    }
}

#[cfg(unix)]
impl RouteLeaseOutboxState {
    fn flush(&mut self) -> Result<usize, XServerFrontendRouteError> {
        if self.disconnected {
            return Err(XServerFrontendRouteError::RouteLeaseQueueDisconnected);
        }
        let Some(sender) = &self.sender else {
            return Ok(0);
        };
        let mut published = 0;
        while let Some(update) = self.pending.pop_front() {
            match sender.try_send(update) {
                Ok(()) => {
                    // Both counters are bounded by accepted, whose credit is
                    // reserved before effects; they therefore cannot wrap.
                    self.published += 1;
                    published += 1;
                }
                Err(std::sync::mpsc::TrySendError::Full(update)) => {
                    self.pending.push_front(update);
                    break;
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.disconnected = true;
                    self.abandoned += 1;
                    while self.pending.pop_front().is_some() {
                        self.abandoned += 1;
                    }
                    return Err(XServerFrontendRouteError::RouteLeaseQueueDisconnected);
                }
            }
        }
        Ok(published)
    }
}

#[cfg(unix)]
impl Drop for RouteLeaseOutboxState {
    fn drop(&mut self) {
        // Teardown abandons delivery, not the effects already performed.
        self.abandoned += self.pending.len() as u64;
        if self.abandoned != 0 {
            tracing::error!(
                accepted = self.accepted, published = self.published,
                abandoned = self.abandoned,
                "sophia_route_lease_notifications status=delivery_abandoned"
            );
        }
    }
}
