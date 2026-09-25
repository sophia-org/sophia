/// The same per-connection protocol queue carries both ordinary events and
/// original control dependencies. No secondary queue can reorder them.
#[cfg(unix)]
struct X11ProtocolEvent {
    event: XClientEvent,
    control: Option<PrivateControlProtocolOutput>,
}

#[cfg(unix)]
impl X11ProtocolEvent {
    fn untracked(event: XClientEvent) -> Self {
        Self {
            event,
            control: None,
        }
    }
}

#[cfg(unix)]
struct PrivateControlProtocolOutput {
    receipt: Arc<PrivateControlProtocolReceipt>,
    _dependent: ControlDependent,
}

#[cfg(unix)]
struct PrivateControlProtocolReceipt {
    recipient: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    endpoint: Option<PrivateEndpointIdentity>,
    event: XClientEvent,
    flushed: AtomicBool,
    terminated: AtomicBool,
    wire_record: Mutex<Option<Vec<u8>>>,
}

#[cfg(unix)]
impl PrivateControlProtocolReceipt {
    fn settled(&self) -> bool {
        self.flushed.load(Ordering::Acquire) || self.terminated.load(Ordering::Acquire)
    }
}

#[cfg(unix)]
#[derive(Clone)]
struct X11ProtocolSender {
    sender: SyncSender<X11ProtocolEvent>,
    /// Raised as an event is queued; the connection's reply ordering reads
    /// it.
    watermark: Arc<X11ProtocolWatermark>,
}

#[cfg(unix)]
impl X11ProtocolSender {
    fn try_send(&self, event: XClientEvent) -> Result<(), TrySendError<XClientEvent>> {
        self.sender
            .try_send(X11ProtocolEvent::untracked(event))
            .map_err(|error| match error {
                TrySendError::Full(event) => TrySendError::Full(event.event),
                TrySendError::Disconnected(event) => TrySendError::Disconnected(event.event),
            })?;
        self.watermark.queued();
        Ok(())
    }
}

#[cfg(unix)]
trait X11RouteSender<T> {
    fn try_route_send(self, value: T) -> Result<(), TrySendError<T>>;
}

#[cfg(unix)]
impl<T> X11RouteSender<T> for SyncSender<T> {
    fn try_route_send(self, value: T) -> Result<(), TrySendError<T>> {
        self.try_send(value)
    }
}

#[cfg(unix)]
impl X11RouteSender<XClientEvent> for X11ProtocolSender {
    fn try_route_send(self, value: XClientEvent) -> Result<(), TrySendError<XClientEvent>> {
        self.try_send(value)
    }
}

#[cfg(unix)]
enum X11ProtocolReceiver {
    Tracked {
        receiver: Receiver<X11ProtocolEvent>,
        registration: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
        watermark: Arc<X11ProtocolWatermark>,
    },
    #[cfg(all(test, unix))]
    Ordinary(Receiver<XClientEvent>),
}

#[cfg(all(test, unix))]
impl From<Receiver<XClientEvent>> for X11ProtocolReceiver {
    fn from(receiver: Receiver<XClientEvent>) -> Self {
        Self::Ordinary(receiver)
    }
}

#[cfg(unix)]
impl X11ProtocolReceiver {
    /// The writer has finished with one event, whatever became of it.
    fn drained(&self) {
        match self {
            Self::Tracked { watermark, .. } => watermark.drained(),
            #[cfg(all(test, unix))]
            Self::Ordinary(_) => {}
        }
    }

    fn receive(&self, timeout: Duration) -> Result<X11ProtocolEvent, RecvTimeoutError> {
        match self {
            Self::Tracked { receiver, .. } => receiver.recv_timeout(timeout),
            #[cfg(all(test, unix))]
            Self::Ordinary(receiver) => receiver
                .recv_timeout(timeout)
                .map(X11ProtocolEvent::untracked),
        }
    }

    fn validate(&self, event: &X11ProtocolEvent) -> Result<(), X11SetupSocketError> {
        if let Some(control) = &event.control {
            match self {
                Self::Tracked { registration, .. }
                    if Arc::ptr_eq(registration, &control.receipt.recipient)
                        && event.event == control.receipt.event => {}
                _ => {
                    return Err(X11SetupSocketError::new(
                        "protocol control names another original receiver",
                    ));
                }
            }
        }
        Ok(())
    }

    fn record_flushed(&self, event: &X11ProtocolEvent) -> Result<(), X11SetupSocketError> {
        self.validate(event)?;
        if let Some(control) = &event.control {
            control.receipt.flushed.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn retain_wire_record(
        &self,
        event: &X11ProtocolEvent,
        record: &[u8],
    ) -> Result<(), X11SetupSocketError> {
        self.validate(event)?;
        if let Some(control) = &event.control {
            let mut retained = control
                .receipt
                .wire_record
                .lock()
                .map_err(|_| X11SetupSocketError::new("protocol wire custody unavailable"))?;
            if retained.is_some() {
                return Err(X11SetupSocketError::new(
                    "protocol control was already attempted",
                ));
            }
            *retained = Some(record.to_vec());
        }
        Ok(())
    }
}

// Ordinary component callers can observe the public event. Consuming it here
// intentionally supplies no private writer receipt.
#[cfg(all(test, unix))]
impl X11ProtocolReceiver {
    fn recv(&self) -> Result<XClientEvent, std::sync::mpsc::RecvError> {
        match self {
            Self::Tracked { receiver, .. } => receiver.recv().map(|event| event.event),
            Self::Ordinary(receiver) => receiver.recv(),
        }
    }
    fn recv_timeout(&self, timeout: Duration) -> Result<XClientEvent, RecvTimeoutError> {
        self.receive(timeout).map(|event| event.event)
    }
    fn try_recv(&self) -> Result<XClientEvent, TryRecvError> {
        match self {
            Self::Tracked { receiver, .. } => receiver.try_recv().map(|event| event.event),
            Self::Ordinary(receiver) => receiver.try_recv(),
        }
    }
    fn try_iter(&self) -> impl Iterator<Item = XClientEvent> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }
}

#[cfg(unix)]
fn control_generation_pending(
    execution: Option<&Arc<Mutex<PrivateControlExecution>>>,
    pending: bool,
) -> Result<(), X11SetupSocketError> {
    if let Some(execution) = execution {
        execution
            .lock()
            .map_err(|_| X11SetupSocketError::new("control output generation unavailable"))?
            .peer_generation_begun = pending;
    }
    Ok(())
}

#[cfg(unix)]
impl XServerFrontendRouteRegistry {
    fn route_control_protocol(
        &self,
        client: XServerFrontendClientId,
        event: XClientEvent,
        execution: Option<&Arc<Mutex<PrivateControlExecution>>>,
    ) -> Result<(), XServerFrontendRouteError> {
        let Some(execution) = execution else {
            return self.route_protocol(client, event);
        };
        let (token, source) = {
            let held = execution
                .lock()
                .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
            (held.token, held.source.clone())
        };
        let completion = self
            .control_completion()
            .ok_or(XServerFrontendRouteError::RegistryPoisoned)?;
        if token.origin != completion.origin
            || !std::sync::Weak::ptr_eq(&source.completion, &Arc::downgrade(&completion.inner))
        {
            return Err(XServerFrontendRouteError::RegistryPoisoned);
        }
        let senders = match self.client_senders(client) {
            Ok(senders) => senders,
            Err(error) => {
                execution
                    .lock()
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .peer_generation_begun = true;
                return Err(error);
            }
        };
        let recipient = senders.connection_state.clone();
        let endpoint = recipient
            .get()
            .and_then(|state| state.control_source.get())
            .and_then(std::sync::Weak::upgrade)
            .map(|source| source.endpoint.clone());
        let receipt = Arc::new(PrivateControlProtocolReceipt {
            recipient: recipient.clone(),
            endpoint,
            event,
            flushed: AtomicBool::new(false),
            terminated: AtomicBool::new(false),
            wire_record: Mutex::new(None),
        });
        execution
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .protocol_receipts
            .push(receipt.clone());
        let dependent = completion.track_dependent(token).map_err(|refusal| {
            XServerFrontendRouteError::DependentNotTracked { client, refusal }
        })?;
        let X11ProtocolSender { sender, watermark } = senders.protocol;
        let result = self.route_to_client(
            client,
            &recipient,
            sender,
            X11ProtocolEvent {
                event,
                control: Some(PrivateControlProtocolOutput {
                    receipt,
                    _dependent: dependent,
                }),
            },
        );
        if result.is_ok() {
            watermark.queued();
        }
        result
    }
}

#[cfg(unix)]
fn visit_control_protocol_receipt(
    execution: &Arc<Mutex<PrivateControlExecution>>,
    origin: &XServerFrontendRouteRegistry,
    service: &PrivateServiceLease<'_>,
    collected: Option<&PrivateConnectionsCollected>,
    place: usize,
    cycle_ended: bool,
) -> Result<bool, PrivateTerminalDriveRefusal> {
    let receipt = {
        let mut operation = execution
            .lock()
            .map_err(|_| PrivateControlCleanupRefusal::Unavailable)?;
        let count = operation.protocol_receipts.len();
        if count == 0 {
            return Ok(false);
        }
        let index = operation.protocol_cursor % count;
        if cycle_ended {
            operation.protocol_cursor = (index + 1) % count;
        }
        operation.protocol_receipts[index].clone()
    };
    if receipt.settled() {
        return Ok(false);
    }
    let endpoint = receipt
        .endpoint
        .as_ref()
        .ok_or(PrivateControlCleanupRefusal::MissingSource)?;
    if !Arc::ptr_eq(&endpoint.registration, &receipt.recipient) {
        return Err(PrivateControlCleanupRefusal::ForeignSource.into());
    }
    let mut examined = place;
    let _terminated = PrivateRecipientTermination::from_place(
        service,
        origin,
        collected,
        &mut examined,
        endpoint,
    )?;
    receipt.terminated.store(true, Ordering::Release);
    Ok(true)
}
