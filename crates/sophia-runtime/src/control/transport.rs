use super::admission::{HostDomain, Peer};
use super::*;
use sophia_protocol::{
    CONTROL_REVISION, ControlCatalog, ControlMessage, ControlOwner, ControlWelcome,
    control_session_operations, decode_control_frame, decode_control_header, encode_control_frame,
};
use std::io::Read;
use std::os::fd::AsFd;
use std::time::Duration;

struct Connection {
    stream: UnixStream,
    peer: Peer,
    id: u64,
    negotiated: bool,
    /// The selected revision; zero before negotiation.
    revision: u16,
    last_request: u64,
    discovered: u64,
    rx: Vec<u8>,
    tx: Vec<u8>,
    written: usize,
    frame_started: Option<Instant>,
    last_activity: Instant,
    reply_started: Instant,
    close_after_reply: bool,
    pending: Option<ControlTicket>,
}

impl Connection {
    fn reply(&mut self, id: u64, message: ControlMessage) -> io::Result<()> {
        self.tx = encode_control_frame(id, &message)
            .map_err(|_| io::Error::other("invalid control reply"))?;
        self.written = 0;
        self.reply_started = Instant::now();
        Ok(())
    }
    fn error(&mut self, id: u64, code: u16) -> io::Result<()> {
        self.close_after_reply = true;
        self.reply(id, ControlMessage::ProtocolError { code })
    }
    fn outcome(&mut self, id: u64, generation: u64, outcome: ControlOutcome) -> io::Result<()> {
        self.reply(
            id,
            ControlMessage::Outcome {
                generation,
                outcome,
                detail: String::new(),
            },
        )
    }
    fn no_extra_input(&self) -> io::Result<()> {
        match sophia_linux_peer::recv_plain(self.stream.as_fd(), &mut [0], true) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(e),
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "control disconnected",
            )),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control pipelining",
            )),
        }
    }
    fn service(&mut self, context: &mut Context<'_>) -> io::Result<bool> {
        let now = Instant::now();
        if !self.negotiated && now.duration_since(self.last_activity) >= Duration::from_secs(2) {
            return Ok(false);
        }
        if let Some(ticket) = self.pending.as_ref() {
            self.no_extra_input()?;
            if ticket.state.phase.load(Ordering::Acquire) == 3 {
                let excluded = context
                    .view
                    .lock()
                    .map_err(|_| io::Error::other("control view"))?
                    .excluded
                    .clone();
                if context.domain.check(&self.peer, &excluded).is_err() {
                    if ticket.cancel_queued() {
                        ticket.finish(ControlOutcome::Denied);
                    }
                } else {
                    let _ = ticket.state.phase.compare_exchange(
                        3,
                        4,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
            }
            if now.duration_since(ticket.received) >= Duration::from_secs(10) {
                ticket.expire();
            }
            let outcome = ticket.state.outcome.load(Ordering::Acquire);
            if outcome != 0 {
                let ticket = self.pending.take().unwrap();
                let outcome =
                    ControlOutcome::from_wire(outcome).map_err(|_| io::Error::other("outcome"))?;
                self.outcome(ticket.request, ticket.generation, outcome)?;
            }
        }
        if !self.tx.is_empty() {
            if now.duration_since(self.reply_started) >= Duration::from_secs(2) {
                return Ok(false);
            }
            self.no_extra_input()?;
            let end = (self.written + 16384).min(self.tx.len());
            match self.stream.write(&self.tx[self.written..end]) {
                Ok(0) => return Ok(false),
                Ok(n) => self.written += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(true),
                Err(e) => return Err(e),
            }
            if self.written == self.tx.len() {
                self.tx.clear();
                self.written = 0;
                self.last_activity = now;
                if self.close_after_reply {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        if self.pending.is_some() {
            return Ok(true);
        }
        if self
            .frame_started
            .is_some_and(|t| now.duration_since(t) >= Duration::from_secs(2))
            || now.duration_since(self.last_activity) >= Duration::from_secs(60)
        {
            return Ok(false);
        }
        let mut budget = 16384;
        while budget > 0 {
            let wanted = if self.rx.len() < 24 {
                24
            } else {
                match decode_control_header(&self.rx[..24]) {
                    Ok((_, _, len)) => 24 + len,
                    Err(_) => {
                        self.error(0, 1)?;
                        return Ok(true);
                    }
                }
            };
            if self.rx.len() == wanted && wanted >= 24 {
                // Header-only Commands is a complete frame.
                if self.rx.len() == 24
                    && decode_control_header(&self.rx).is_ok_and(|(_, _, len)| len != 0)
                {
                    continue;
                }
                self.no_extra_input()?;
                let error_id = decode_control_header(&self.rx[..24]).map_or(0, |(_, id, _)| id);
                let decoded = decode_control_frame(&self.rx);
                self.rx.clear();
                self.frame_started = None;
                self.last_activity = now;
                match decoded {
                    Ok((id, message)) => self.dispatch(id, message, context)?,
                    Err(_) => self.error(error_id, 1)?,
                }
                return Ok(true);
            }
            let mut bytes = [0_u8; 16384];
            let capacity = (wanted - self.rx.len()).min(budget);
            match sophia_linux_peer::recv_plain(self.stream.as_fd(), &mut bytes[..capacity], false)
            {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.frame_started.get_or_insert(now);
                    self.rx.extend_from_slice(&bytes[..n]);
                    budget -= n;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(true)
    }
    fn dispatch(
        &mut self,
        id: u64,
        message: ControlMessage,
        context: &mut Context<'_>,
    ) -> io::Result<()> {
        let (catalog, excluded) = {
            let view = context
                .view
                .lock()
                .map_err(|_| io::Error::other("control view poisoned"))?;
            (view.catalog.clone(), view.excluded.clone())
        };
        context.domain.check(&self.peer, &excluded)?;
        if !self.negotiated {
            let ControlMessage::Hello {
                minimum_revision,
                maximum_revision,
                required_features,
            } = message
            else {
                return self.error(id, 2);
            };
            if minimum_revision > CONTROL_REVISION || maximum_revision < 1 {
                return self.error(0, 3);
            }
            if required_features != 0 {
                return self.error(0, 4);
            }
            self.negotiated = true;
            self.revision = maximum_revision.min(CONTROL_REVISION);
            return self.reply(
                0,
                ControlMessage::Welcome(ControlWelcome {
                    revision: self.revision,
                    session_id: context.session_id,
                    connection_id: self.id,
                    command_timeout_ms: 10000,
                    frame_timeout_ms: 2000,
                    idle_timeout_ms: 60000,
                }),
            );
        }
        if id <= self.last_request {
            return self.error(id, 2);
        }
        self.last_request = id;
        // A connection sees and may invoke only its revision's session
        // operations; the published catalog is the highest revision's.
        let view = revision_view(&catalog, self.revision);
        match message {
            ControlMessage::Commands => {
                self.discovered = catalog.generation;
                self.reply(id, ControlMessage::Catalog(view))
            }
            ControlMessage::Invoke {
                generation,
                command,
            } => {
                if generation != catalog.generation || generation != self.discovered {
                    return self.outcome(id, generation, ControlOutcome::Stale);
                }
                if !view.commands.contains(&command) {
                    return self.outcome(id, generation, ControlOutcome::Rejected);
                }
                if context.active.len() >= CONTROL_MAX_PENDING {
                    return self.outcome(id, generation, ControlOutcome::Overloaded);
                }
                let ticket = ControlTicket {
                    connection: self.id,
                    request: id,
                    generation,
                    command,
                    received: Instant::now(),
                    state: Arc::new(TicketState {
                        phase: AtomicU8::new(0),
                        outcome: AtomicU16::new(0),
                        settled: AtomicBool::new(false),
                        wake: context.wake.clone(),
                    }),
                };
                if context.requests.try_send(ticket.clone()).is_err() {
                    return self.outcome(id, generation, ControlOutcome::Overloaded);
                }
                context.active.push(ticket.clone());
                self.pending = Some(ticket);
                Ok(())
            }
            _ => self.error(id, 2),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(ticket) = &self.pending {
            ticket.disconnect();
        }
    }
}

struct Context<'a> {
    domain: &'a HostDomain,
    session_id: [u64; 2],
    view: &'a Mutex<View>,
    requests: &'a SyncSender<ControlTicket>,
    active: &'a mut Vec<ControlTicket>,
    wake: &'a Arc<UnixStream>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    listener: UnixListener,
    mut wake_rx: UnixStream,
    wake_tx: Arc<UnixStream>,
    domain: HostDomain,
    session_id: [u64; 2],
    view: Arc<Mutex<View>>,
    stop: Arc<AtomicBool>,
    requests: SyncSender<ControlTicket>,
) {
    let mut peers = Vec::<Connection>::new();
    let mut active = Vec::<ControlTicket>::new();
    let mut next_id = 1_u64;
    while !stop.load(Ordering::Acquire) {
        let mut fds = vec![
            rustix::event::PollFd::new(&listener, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&wake_rx, rustix::event::PollFlags::IN),
        ];
        for peer in &peers {
            fds.push(rustix::event::PollFd::new(
                &peer.stream,
                rustix::event::PollFlags::IN
                    | if peer.tx.is_empty() {
                        rustix::event::PollFlags::empty()
                    } else {
                        rustix::event::PollFlags::OUT
                    },
            ));
        }
        match rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 25_000_000,
            }),
        ) {
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => break,
            Ok(_) => {}
        }
        drop(fds);
        let mut scratch = [0; 128];
        while wake_rx.read(&mut scratch).is_ok_and(|n| n != 0) {}
        active.retain(|t| !t.state.settled.load(Ordering::Acquire));
        let excluded = match view.lock() {
            Ok(view) => view.excluded.clone(),
            Err(_) => break,
        };
        for _ in 0..4 {
            let Ok((stream, _)) = listener.accept() else {
                break;
            };
            if peers.len() >= 32 || next_id == u64::MAX {
                continue;
            }
            let now = Instant::now();
            let Ok(peer) = domain.admit(&stream, &excluded) else {
                continue;
            };
            if stream.set_nonblocking(true).is_err() {
                continue;
            }
            if now.elapsed() >= Duration::from_secs(2) {
                continue;
            }
            peers.push(Connection {
                stream,
                peer,
                id: next_id,
                negotiated: false,
                revision: 0,
                last_request: 0,
                discovered: 0,
                rx: Vec::new(),
                tx: Vec::new(),
                written: 0,
                frame_started: None,
                last_activity: now,
                reply_started: now,
                close_after_reply: false,
                pending: None,
            });
            next_id += 1;
        }
        let mut context = Context {
            domain: &domain,
            session_id,
            view: &view,
            requests: &requests,
            active: &mut active,
            wake: &wake_tx,
        };
        peers.retain_mut(|peer| peer.service(&mut context).unwrap_or(false));
        if !peers.is_empty() {
            peers.rotate_left(1);
        }
    }
    for ticket in active {
        ticket.expire();
    }
}

/// The catalog a connection of `revision` may see: every policy command and
/// the session operations that revision dispatches. Revision 1 dispatches only
/// `restart-wm` (it reserves `reload-profile`), so it never sees revision 2's.
fn revision_view(catalog: &ControlCatalog, revision: u16) -> ControlCatalog {
    let dispatched: &[&str] = if revision == 1 {
        &["restart-wm"]
    } else {
        control_session_operations(revision)
    };
    ControlCatalog {
        generation: catalog.generation,
        commands: catalog
            .commands
            .iter()
            .filter(|c| c.owner == ControlOwner::Policy || dispatched.contains(&c.name.as_str()))
            .cloned()
            .collect(),
    }
}
