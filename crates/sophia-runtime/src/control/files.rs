//! Administrative files use the existing ticket owner; this worker owns only
//! admission, bounded wire state and the result retained for each connection.
mod export;

use super::admission::{HostDomain, Peer};
use super::*;
use export::AdminExport;
use sophia_9p::unix::Server;
use sophia_9p::{Errno, Limits};
use std::cell::RefCell;
use std::io::Read;
use std::rc::Rc;
use std::time::Duration;

const FRAME_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

struct Shared {
    domain: HostDomain,
    session_id: [u64; 2],
    view: Arc<Mutex<View>>,
    requests: sophia_wake::SignalSender<ControlTicket>,
    wake: Arc<UnixStream>,
    owner_wake: sophia_wake::WakeSlot,
    active: Vec<ControlTicket>,
    next_qid: u64,
}

impl Shared {
    fn qids(&mut self, count: u64) -> Result<u64, Errno> {
        let start = self.next_qid;
        self.next_qid = start.checked_add(count).ok_or(Errno::ENOSPC)?;
        Ok(start)
    }

    fn authorized(&self, peer: &Peer) -> bool {
        self.view
            .lock()
            .is_ok_and(|view| self.domain.check(peer, &view.excluded).is_ok())
    }
}

struct Connection {
    server: Server<AdminExport>,
    wire_started: Option<Instant>,
}

impl Connection {
    fn turn(&mut self) -> bool {
        let now = Instant::now();
        if self
            .wire_started
            .is_some_and(|start| now.duration_since(start) >= FRAME_TIMEOUT)
            || !self.server.export_mut().advance(now)
        {
            self.close();
            return false;
        }
        let running = self.server.turn(Some(Duration::ZERO)).unwrap_or(false);
        if self.server.has_pending_wire_io() {
            self.wire_started.get_or_insert(now);
        } else {
            self.wire_started = None;
        }
        running && self.server.connection_count() != 0
    }

    fn close(&mut self) {
        self.server.wake().stop();
        let _ = self.server.turn(Some(Duration::ZERO));
    }
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
    requests: sophia_wake::SignalSender<ControlTicket>,
    owner_wake: sophia_wake::WakeSlot,
) {
    let shared = Rc::new(RefCell::new(Shared {
        domain,
        session_id,
        view,
        requests,
        wake: wake_tx,
        owner_wake,
        active: Vec::with_capacity(CONTROL_MAX_PENDING),
        next_qid: 1,
    }));
    // The minimum accommodates the full 512-byte invocation and 9P framing.
    // Each peer has at most one catalog, sixteen fids and 128 KiB of replies.
    let limits = Limits::new(64 * 1024, 1024, 8, 16, 128 * 1024, 1).unwrap();
    let mut peers = Vec::<Connection>::new();
    let mut next_id = 1_u64;
    while !stop.load(Ordering::Acquire) {
        let mut fds = vec![
            rustix::event::PollFd::new(&listener, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&wake_rx, rustix::event::PollFlags::IN),
        ];
        for peer in &peers {
            fds.extend(peer.server.poll_fds());
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
        shared
            .borrow_mut()
            .active
            .retain(|ticket| !ticket.state.settled.load(Ordering::Acquire));
        for _ in 0..4 {
            let Ok((stream, _)) = listener.accept() else {
                break;
            };
            if peers.len() >= 32 || next_id == u64::MAX {
                continue;
            }
            let started = Instant::now();
            let admitted = {
                let shared = shared.borrow();
                let Ok(view) = shared.view.lock() else {
                    break;
                };
                shared.domain.admit(&stream, &view.excluded)
            };
            let Ok(peer) = admitted else { continue };
            if started.elapsed() >= FRAME_TIMEOUT {
                continue;
            }
            let Ok(export) = AdminExport::new(shared.clone(), peer, next_id) else {
                continue;
            };
            next_id += 1;
            let Ok(mut server) = Server::new(export, limits) else {
                continue;
            };
            if server.adopt(stream).is_ok() {
                peers.push(Connection {
                    server,
                    wire_started: None,
                });
            }
        }
        peers.retain_mut(Connection::turn);
        if !peers.is_empty() {
            peers.rotate_left(1);
        }
    }
    for peer in &mut peers {
        peer.close();
    }
    for ticket in &shared.borrow().active {
        ticket.expire();
    }
}
