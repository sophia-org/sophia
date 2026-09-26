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

use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::pipe::{PipeFlags, pipe_with};

use crate::connection::{Connection, ConnectionId};
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
        if readiness.wake.contains(PollFlags::IN) {
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
        }
        if readiness.listener.contains(PollFlags::IN) {
            self.accept()?;
        }
        for (slot, flags) in self.slots.iter_mut().zip(readiness.slots) {
            if flags.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR) {
                Self::read(&mut self.export, slot);
            }
        }
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
        Ok(true)
    }

    fn wait(&self, timeout: Option<Duration>) -> io::Result<Readiness> {
        let listening = self.slots.len() < self.limits.max_connections();
        let mut fds = Vec::with_capacity(self.slots.len() + 2);
        fds.push(PollFd::new(&self.reader, PollFlags::IN));
        if let Some(listener) = self.listener.as_ref().filter(|_| listening) {
            fds.push(PollFd::new(listener, PollFlags::IN));
        }
        let offset = fds.len();
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
        let timeout = timeout
            .map(Timespec::try_from)
            .transpose()
            .map_err(|_| io::Error::other("timeout out of range"))?;
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

    fn read(export: &mut E, slot: &mut Slot<E>) {
        let room = slot.connection.input_room();
        if room == 0 {
            return;
        }
        let mut buffer = vec![0; room];
        match slot.stream.read(&mut buffer) {
            Ok(0) => slot.ended = true,
            // The buffer is sized to the room, so every byte read is taken.
            Ok(count) => match slot.connection.receive(export, &buffer[..count]) {
                Ok(taken) if taken == count => {}
                Ok(_) | Err(_) => slot.ended = true,
            },
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => slot.ended = true,
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
