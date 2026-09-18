// The producer handoff: how a caller outside the private service's frame
// obtains the existing control producer and an ingress for one actually
// admitted connection, from the runner the service alone owns.
//
// THE RUNNER NEVER LEAVES ITS THREAD. What crosses is a request naming the
// caller's owner and a reply carrying the producer the runner issued under
// the service's own lease. The PORT stays with the service; the ACCESS goes
// to the caller. Readiness is published once preparation and binding have
// succeeded and nothing before; the port is closed -- standing Ended, the
// request channel gone -- as the first act of the service's exit, before any
// worker is stopped or waited for, so a producer cannot be issued into a
// collection. A request the loop never answered is refused as Unanswered,
// not silently dropped.

/// Where the service stands, as a caller of the access sees it.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivatePortStanding {
    /// Preparation has not succeeded (or the service never began).
    NotReady,
    /// The runner is prepared and the listener bound: producers may be asked
    /// for.
    Ready,
    /// The service has begun its exit: nothing more is issued.
    Ended,
}

/// Why the access could not hand a producer back.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateProducerRefusal {
    /// Asked before readiness was published; nothing was queued.
    NotReady,
    /// Asked after the service closed its port (or the service never
    /// reached its loop: every pre-loop exit and the port's own drop close
    /// it).
    Ended,
    /// The bounded request backlog is full; nothing was queued.
    Backlogged,
    /// The request's lease is not on the owner the service is leased from.
    ForeignServiceOwner,
    /// The runner refused, with its own reason.
    Runner(PrivateServiceRefusal),
    /// The service's loop ended while the request was in flight.
    Unanswered,
    /// The bounded wait for readiness ran out.
    ReadinessTimedOut,
}

/// What a request asks for.
#[cfg(unix)]
enum PrivateProducerAsk {
    Control,
    Ingress {
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
        /// The admission the asker meant, checked where the grant is issued.
        expected: Option<sophia_protocol::ClientAdmissionId>,
    },
}

/// What a request is answered with.
#[cfg(unix)]
enum PrivateProducerIssued {
    Control(Box<PrivateControlProducer>),
    Ingress(Box<PrivateIngress>),
}

/// One request, carrying the keeper identity of the lease it was made under
/// and the channel its answer goes back on.
#[cfg(unix)]
struct PrivateProducerRequest {
    keeper: PrivateCustodyKeeper,
    ask: PrivateProducerAsk,
    reply: SyncSender<Result<PrivateProducerIssued, PrivateProducerRefusal>>,
}

#[cfg(unix)]
struct PrivatePortState {
    standing: Mutex<PrivatePortStanding>,
    changed: Condvar,
}

#[cfg(unix)]
impl PrivatePortState {
    fn standing(&self) -> PrivatePortStanding {
        match self.standing.lock() {
            Ok(standing) => *standing,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    fn set(&self, standing: PrivatePortStanding) {
        let mut held = match self.standing.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        *held = standing;
        drop(held);
        self.changed.notify_all();
    }
}

/// The service's side: answers requests from its loop, publishes readiness,
/// closes at exit.
#[cfg(unix)]
pub struct PrivateProducerPort {
    requests: Option<Receiver<PrivateProducerRequest>>,
    state: Arc<PrivatePortState>,
}

/// The caller's side. `Send`: the producers it hands back are, and the
/// runner they come from is not what it holds.
#[cfg(unix)]
pub struct PrivateProducerAccess {
    requests: SyncSender<PrivateProducerRequest>,
    state: Arc<PrivatePortState>,
}

/// The request backlog: how many requests may wait at the port, and how many
/// one turn answers at most, so a caller that keeps asking cannot hold the
/// loop's turn open and cannot grow anything.
#[cfg(unix)]
const PRODUCER_REQUEST_BACKLOG: usize = 8;

#[cfg(unix)]
impl PrivateProducerAccess {
    /// A port for one service invocation and the access that asks it.
    pub fn for_service() -> (PrivateProducerPort, PrivateProducerAccess) {
        let (requests, requested) = sync_channel(PRODUCER_REQUEST_BACKLOG);
        let state = Arc::new(PrivatePortState {
            standing: Mutex::new(PrivatePortStanding::NotReady),
            changed: Condvar::new(),
        });
        (
            PrivateProducerPort {
                requests: Some(requested),
                state: Arc::clone(&state),
            },
            PrivateProducerAccess { requests, state },
        )
    }

    pub fn standing(&self) -> PrivatePortStanding {
        self.state.standing()
    }

    /// Wait, bounded, for readiness. Ended before Ready is a refusal, not a
    /// wait.
    pub fn await_ready(&self, bound: Duration) -> Result<(), PrivateProducerRefusal> {
        let deadline = std::time::Instant::now() + bound;
        let mut held = match self.state.standing.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        loop {
            match *held {
                PrivatePortStanding::Ready => return Ok(()),
                PrivatePortStanding::Ended => return Err(PrivateProducerRefusal::Ended),
                PrivatePortStanding::NotReady => {}
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(PrivateProducerRefusal::ReadinessTimedOut);
            }
            held = match self.state.changed.wait_timeout(held, deadline - now) {
                Ok((held, _)) => held,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }

    /// The existing control producer, issued by the runner under the
    /// service's lease, for a caller whose lease is on the same owner.
    pub fn control_producer(
        &self,
        service: &PrivateServiceLease<'_>,
    ) -> Result<PrivateControlProducer, PrivateProducerRefusal> {
        match self.ask(service, PrivateProducerAsk::Control)? {
            PrivateProducerIssued::Control(producer) => Ok(*producer),
            PrivateProducerIssued::Ingress(_) => Err(PrivateProducerRefusal::Unanswered),
        }
    }

    /// An ingress for one admitted connection, issued by the runner under the
    /// service's lease.
    /// An ingress for a client number, against whatever admission is current.
    ///
    /// The older shape, kept for callers that have no admission in hand. A
    /// client number is reused, so between choosing one and being answered a
    /// successor can take it; this issues against the successor. Prefer
    /// `ingress_for_admission` wherever the admission is known.
    pub fn ingress_for(
        &self,
        service: &PrivateServiceLease<'_>,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateIngress, PrivateProducerRefusal> {
        self.issue_ingress(service, client, device, None)
    }

    /// An ingress for one exact admission.
    ///
    /// THE ADMISSION TRAVELS WITH THE ASK. The check happens inside the act
    /// that issues the grant, so a connection that ended after the caller read
    /// the boundary cannot have its number answered on behalf of whoever took
    /// it next. A mismatch is refused as `DifferentAdmission` rather than
    /// served.
    pub fn ingress_for_admission(
        &self,
        service: &PrivateServiceLease<'_>,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
        expected: sophia_protocol::ClientAdmissionId,
    ) -> Result<PrivateIngress, PrivateProducerRefusal> {
        self.issue_ingress(service, client, device, Some(expected))
    }

    fn issue_ingress(
        &self,
        service: &PrivateServiceLease<'_>,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
        expected: Option<sophia_protocol::ClientAdmissionId>,
    ) -> Result<PrivateIngress, PrivateProducerRefusal> {
        match self.ask(
            service,
            PrivateProducerAsk::Ingress {
                client,
                device,
                expected,
            },
        )? {
            PrivateProducerIssued::Ingress(ingress) => Ok(*ingress),
            PrivateProducerIssued::Control(_) => Err(PrivateProducerRefusal::Unanswered),
        }
    }

    fn ask(
        &self,
        service: &PrivateServiceLease<'_>,
        ask: PrivateProducerAsk,
    ) -> Result<PrivateProducerIssued, PrivateProducerRefusal> {
        // NOT QUEUED BEFORE READINESS: a request made too early is refused
        // here, where the caller is, rather than held until a service that
        // may never prepare answers it.
        match self.state.standing() {
            PrivatePortStanding::NotReady => return Err(PrivateProducerRefusal::NotReady),
            PrivatePortStanding::Ended => return Err(PrivateProducerRefusal::Ended),
            PrivatePortStanding::Ready => {}
        }
        let (reply, answered) = sync_channel(1);
        let request = PrivateProducerRequest {
            keeper: service.keeper(),
            ask,
            reply,
        };
        // NEVER BLOCKING, NEVER UNBOUNDED: the backlog is the channel's
        // bound; a full one refuses here with nothing queued.
        match self.requests.try_send(request) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                return Err(PrivateProducerRefusal::Backlogged);
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                return Err(PrivateProducerRefusal::Ended);
            }
        }
        // The loop answers within its turn; a loop that ended drops the
        // reply sender and this returns Unanswered rather than waiting on
        // a service that is gone.
        answered.recv().unwrap_or(Err(PrivateProducerRefusal::Unanswered))
    }
}

/// A PORT THAT GOES AWAY HAS ENDED: a service that unwound or returned
/// before its loop leaves the access seeing Ended, never NotReady with a
/// receiver nobody serves.
#[cfg(unix)]
impl Drop for PrivateProducerPort {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(unix)]
impl PrivateProducerPort {
    /// A port nobody can ask: for a service whose caller wants no producers.
    pub fn unattended() -> Self {
        let (port, access) = PrivateProducerAccess::for_service();
        drop(access);
        port
    }

    /// Publish readiness. Called once, after preparation and binding
    /// succeeded and before the loop's first turn.
    pub(crate) fn publish_ready(&self) {
        self.state.set(PrivatePortStanding::Ready);
    }

    /// Close: nothing more is issued. The request channel goes, so a request
    /// sent from here is refused Ended by its sender, and one already queued
    /// is dropped with its reply sender, which its caller reads as
    /// Unanswered. Idempotent.
    pub(crate) fn close(&mut self) {
        self.state.set(PrivatePortStanding::Ended);
        drop(self.requests.take());
    }

    /// Answer the requests waiting, at most the backlog's worth, each under
    /// the service's own lease and only for a caller whose keeper identity is
    /// this owner's. THE KEEPER IS IDENTITY, NOT A LEASE: the runner issues
    /// under the service's checked borrow, and the producer keeps its own
    /// submission-time check. Replies never block: each reply channel holds
    /// exactly one answer. Returns (issued, refused).
    pub(crate) fn answer(
        &mut self,
        runner: &mut PrivatePreparedRunner,
        service: &PrivateServiceLease<'_>,
    ) -> (usize, usize) {
        let mut issued = 0;
        let mut refused = 0;
        let Some(requests) = self.requests.as_ref() else {
            return (0, 0);
        };
        for _ in 0..PRODUCER_REQUEST_BACKLOG {
            let Ok(request) = requests.try_recv() else {
                break;
            };
            let answer = if !service.keeps_for(&request.keeper) {
                Err(PrivateProducerRefusal::ForeignServiceOwner)
            } else {
                match request.ask {
                    PrivateProducerAsk::Control => runner
                        .control_producer(service)
                        .map(|producer| PrivateProducerIssued::Control(Box::new(producer)))
                        .map_err(PrivateProducerRefusal::Runner),
                    PrivateProducerAsk::Ingress {
                        client,
                        device,
                        expected,
                    } => runner
                        .ingress_for_admission(service, client, device, expected)
                        .map(|ingress| PrivateProducerIssued::Ingress(Box::new(ingress)))
                        .map_err(PrivateProducerRefusal::Runner),
                }
            };
            match answer {
                Ok(_) => issued += 1,
                Err(_) => refused += 1,
            }
            // A caller that stopped waiting is not this loop's problem.
            let _ = request.reply.send(answer);
        }
        (issued, refused)
    }
}
