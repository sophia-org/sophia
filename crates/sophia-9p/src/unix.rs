//! A single-threaded driver for connections over Unix stream sockets.
//!
//! It owns no authority. A caller that admits peers itself -- Session, with
//! its protected endpoint -- hands over each admitted socket through
//! [`Server::adopt`]; a test harness may instead give it a listener. Either
//! way the export decides what each attach may reach.
//!
//! An export whose data changes outside a request (an event arriving for a
//! waiting read) calls [`Wake::wake`], and every waiting read is retried.
//! [`Wake::stop`] ends [`Server::run`] and closes every connection.
//!
//! An owner that also waits on other sources takes [`Server::poll_fds`] into
//! its own wait, then calls [`Server::turn`] with a zero timeout.

use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::pipe::{PipeFlags, pipe_with};

use crate::connection::{Connection, ConnectionId, InPlaceTarget};
use crate::export::{Export, PeerCredentials};
use crate::records::Limits;

/// Wakes a running [`Server`] from any thread.
#[derive(Clone)]
pub struct Wake {
    inner: Arc<WakeInner>,
}

struct WakeInner {
    writer: OwnedFd,
    stop: AtomicBool,
}

impl Wake {
    /// Retry every waiting read.
    pub fn wake(&self) {
        // A full pipe already holds a wakeup; nothing is lost by dropping this.
        let _ = rustix::io::write(&self.inner.writer, &[1]);
    }

    /// End [`Server::run`], closing every connection.
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        self.wake();
    }
}

/// A socket the server could not take: it already serves its connection
/// limit, or the socket could not be prepared.
pub struct Refused {
    pub stream: UnixStream,
    pub error: io::Error,
}

struct Slot<E: Export> {
    stream: UnixStream,
    connection: Connection<E>,
    ended: bool,
}

pub struct Server<E: Export> {
    export: E,
    limits: Limits,
    listener: Option<UnixListener>,
    slots: Vec<Slot<E>>,
    reader: OwnedFd,
    wake: Wake,
    next_id: u64,
    /// Shared by sequential connection reads, bounded by their input limits.
    read_buffer: Vec<u8>,
}

impl<E: Export> Server<E> {
    pub fn new(export: E, limits: Limits) -> io::Result<Self> {
        let (reader, writer) = pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)?;
        Ok(Self {
            export,
            limits,
            listener: None,
            slots: Vec::new(),
            reader,
            wake: Wake {
                inner: Arc::new(WakeInner {
                    writer,
                    stop: AtomicBool::new(false),
                }),
            },
            next_id: 1,
            read_buffer: Vec::new(),
        })
    }

    pub fn wake(&self) -> Wake {
        self.wake.clone()
    }

    pub fn export(&self) -> &E {
        &self.export
    }

    pub fn export_mut(&mut self) -> &mut E {
        &mut self.export
    }

    pub fn connection_count(&self) -> usize {
        self.slots.len()
    }

    /// Accept connections from a listener, up to the connection limit. For
    /// harnesses; an owner with its own admission uses [`Self::adopt`].
    pub fn listen(&mut self, listener: UnixListener) -> io::Result<()> {
        listener.set_nonblocking(true)?;
        self.listener = Some(listener);
        Ok(())
    }

    /// Serve a socket a caller has already accepted and admitted.
    pub fn adopt(&mut self, stream: UnixStream) -> Result<ConnectionId, Refused> {
        if self.slots.len() >= self.limits.max_connections() {
            return Err(Refused {
                stream,
                error: io::Error::other("connection limit reached"),
            });
        }
        if let Err(error) = stream.set_nonblocking(true) {
            return Err(Refused { stream, error });
        }
        let peer = rustix::net::sockopt::socket_peercred(&stream)
            .ok()
            .map(|credentials| PeerCredentials {
                pid: rustix::process::Pid::as_raw(Some(credentials.pid)),
                uid: credentials.uid.as_raw(),
                gid: credentials.gid.as_raw(),
            });
        let id = ConnectionId(self.next_id);
        self.next_id += 1;
        self.slots.push(Slot {
            stream,
            connection: Connection::new(id, peer, self.limits),
            ended: false,
        });
        Ok(id)
    }

    /// Serve until [`Wake::stop`].
    pub fn run(&mut self) -> io::Result<()> {
        while self.turn(None)? {}
        Ok(())
    }

    /// One wait for readiness and the work it allows. `false` once stopped,
    /// with every connection closed.
    pub fn turn(&mut self, timeout: Option<Duration>) -> io::Result<bool> {
        let readiness = self.wait(timeout)?;
        if readiness.wake.contains(PollFlags::IN) && !self.service_wake()? {
            return Ok(false);
        }
        if readiness.listener.contains(PollFlags::IN) {
            self.accept()?;
        }
        for (slot, flags) in self.slots.iter_mut().zip(readiness.slots) {
            if flags.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR) {
                Self::read(&mut self.export, slot, &mut self.read_buffer);
            }
        }
        self.write_ready();
        Ok(true)
    }

    /// Flush a local export change without accepting or reading new requests.
    /// Readable sockets stay level-ready for the owner's next input pass.
    pub fn flush(&mut self) -> io::Result<bool> {
        if !self.service_wake()? {
            return Ok(false);
        }
        self.write_ready();
        Ok(true)
    }

    fn service_wake(&mut self) -> io::Result<bool> {
        self.drain_wake()?;
        if self.wake.inner.stop.load(Ordering::SeqCst) {
            for slot in &mut self.slots {
                slot.connection.close(&mut self.export);
            }
            self.slots.clear();
            return Ok(false);
        }
        for slot in &mut self.slots {
            if slot.connection.retry_waiting(&mut self.export).is_err() {
                slot.ended = true;
            }
        }
        Ok(true)
    }

    fn write_ready(&mut self) {
        for slot in &mut self.slots {
            Self::write(&mut self.export, slot);
        }
        let export = &mut self.export;
        self.slots.retain_mut(|slot| {
            if slot.ended {
                slot.connection.close(export);
            }
            !slot.ended
        });
    }

    /// The descriptors and interest [`Self::turn`] waits on, for an owner
    /// that waits on this server beside its own sources. This only borrows:
    /// the owner's next turn consumes whatever readiness it reports.
    pub fn poll_fds(&self) -> Vec<PollFd<'_>> {
        self.poll_set()
    }

    /// The same input-room, output and listener-capacity policy as a turn.
    fn poll_set(&self) -> Vec<PollFd<'_>> {
        let listening = self.slots.len() < self.limits.max_connections();
        let mut fds = Vec::with_capacity(self.slots.len() + 2);
        fds.push(PollFd::new(&self.reader, PollFlags::IN));
        if let Some(listener) = self.listener.as_ref().filter(|_| listening) {
            fds.push(PollFd::new(listener, PollFlags::IN));
        }
        for slot in &self.slots {
            let mut flags = PollFlags::empty();
            if slot.connection.input_room() > 0 {
                flags |= PollFlags::IN;
            }
            if !slot.connection.output().is_empty() {
                flags |= PollFlags::OUT;
            }
            fds.push(PollFd::new(&slot.stream, flags));
        }
        fds
    }

    fn wait(&self, timeout: Option<Duration>) -> io::Result<Readiness> {
        let timeout = timeout
            .map(Timespec::try_from)
            .transpose()
            .map_err(|_| io::Error::other("timeout out of range"))?;
        let mut fds = self.poll_set();
        let offset = fds.len() - self.slots.len();
        poll(&mut fds, timeout.as_ref())?;
        Ok(Readiness {
            wake: fds[0].revents(),
            listener: if offset == 2 {
                fds[1].revents()
            } else {
                PollFlags::empty()
            },
            slots: fds[offset..].iter().map(PollFd::revents).collect(),
        })
    }

    fn drain_wake(&self) -> io::Result<()> {
        let mut buffer = [0; 64];
        loop {
            match rustix::io::read(&self.reader, &mut buffer) {
                Ok(0) | Err(rustix::io::Errno::AGAIN) => return Ok(()),
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn accept(&mut self) -> io::Result<()> {
        while self.slots.len() < self.limits.max_connections() {
            let Some(listener) = &self.listener else {
                return Ok(());
            };
            match listener.accept() {
                // A socket that cannot be prepared is dropped, closing it.
                Ok((stream, _)) => drop(self.adopt(stream)),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Reads up to one message's worth for the connection. A read that
    /// fills what it asked for is followed by another, so the header and the
    /// data of a write received in place, read apart, arrive in one turn.
    fn read(export: &mut E, slot: &mut Slot<E>, buffer: &mut Vec<u8>) {
        let budget = slot.connection.read_budget();
        let mut taken = 0;
        while !slot.ended && taken < budget {
            match Self::read_once(export, slot, buffer, budget - taken) {
                Some((count, asked)) => {
                    taken += count;
                    if count < asked {
                        break;
                    }
                }
                None => break,
            }
        }
    }

    /// One read of at most `limit` bytes: how many it took and how many it
    /// asked for, or `None` when it took none.
    fn read_once(
        export: &mut E,
        slot: &mut Slot<E>,
        buffer: &mut Vec<u8>,
        limit: usize,
    ) -> Option<(usize, usize)> {
        // A write received in place reads straight into its destination.
        if let Some(target) = slot.connection.in_place_target(export) {
            let (result, asked) = match target {
                InPlaceTarget::Destination(destination) => {
                    let asked = destination.len().min(limit);
                    (slot.stream.read(&mut destination[..asked]), asked)
                }
                InPlaceTarget::Discard(rest) => {
                    let asked = rest.min(limit);
                    if buffer.len() < asked {
                        buffer.resize(asked, 0);
                    }
                    (slot.stream.read(&mut buffer[..asked]), asked)
                }
            };
            return match result {
                Ok(0) => {
                    slot.ended = true;
                    None
                }
                Ok(count) => {
                    if slot.connection.received_in_place(export, count).is_err() {
                        slot.ended = true;
                        return None;
                    }
                    Some((count, asked))
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    None
                }
                Err(_) => {
                    slot.ended = true;
                    None
                }
            };
        }
        let room = slot.connection.input_room().min(limit);
        if room == 0 {
            return None;
        }
        if buffer.len() < room {
            buffer.resize(room, 0);
        }
        match slot.stream.read(&mut buffer[..room]) {
            Ok(0) => {
                slot.ended = true;
                None
            }
            // The buffer is sized to the room, so every byte read is taken.
            Ok(count) => match slot.connection.receive(export, &buffer[..count]) {
                Ok(taken) if taken == count => Some((count, room)),
                Ok(_) | Err(_) => {
                    slot.ended = true;
                    None
                }
            },
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                None
            }
            Err(_) => {
                slot.ended = true;
                None
            }
        }
    }

    /// Writes what the socket takes, then lets held requests and waiting
    /// reads use the room that made.
    fn write(export: &mut E, slot: &mut Slot<E>) {
        while !slot.ended && !slot.connection.output().is_empty() {
            match slot.stream.write(slot.connection.output()) {
                Ok(0) => slot.ended = true,
                Ok(count) => {
                    slot.connection.sent(count);
                    let resumed = slot
                        .connection
                        .resume(export)
                        .and_then(|()| slot.connection.retry_waiting(export));
                    if resumed.is_err() {
                        slot.ended = true;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(_) => slot.ended = true,
            }
        }
    }
}

struct Readiness {
    wake: PollFlags,
    listener: PollFlags,
    slots: Vec<PollFlags>,
}
