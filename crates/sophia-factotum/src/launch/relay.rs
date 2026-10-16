//! Conversation-only 9P export for Session's end of a provisioned launch pair.
//! No listener, credential lookup, or caller-supplied registration is involved.

use super::{
    Nonce,
    queue::{Error, Ingress, Ticket},
    wire::{Command, Response},
};
use sophia_9p::{
    ConnectionId, Errno, OpenFlags,
    export::{
        Access, AttachContext, Attachment, Entry, Epoch, Export, NodeKind, ReadOutcome, WalkName,
    },
};
use std::{
    io,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

pub const ATTACH: &[u8] = b"sophia-launch-relay-v1";
pub const RECORD_BYTES: usize = 40;
pub const STATUS_BYTES: usize = 8;

/// Version and kind are independent of every public role's record vocabulary.
pub fn encode_challenge(nonce: Nonce) -> [u8; RECORD_BYTES] {
    let mut bytes = [0; RECORD_BYTES];
    bytes[..4].copy_from_slice(&1_u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
    bytes[8..].copy_from_slice(&nonce.0);
    bytes
}

pub fn decode_challenge(bytes: &[u8]) -> Result<Nonce, Errno> {
    if bytes.len() != RECORD_BYTES || bytes[..8] != [1, 0, 0, 0, 1, 0, 0, 0] {
        return Err(Errno::EINVAL);
    }
    Ok(Nonce(bytes[8..].try_into().map_err(|_| Errno::EINVAL)?))
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Node {
    Root,
    Rpc,
}
pub struct Handle {
    rpc: bool,
    pending: Option<Pending>,
}
struct Pending {
    nonce: Nonce,
    deadline: Instant,
    ticket: Option<Ticket>,
}

pub struct Relay {
    connection: Option<ConnectionId>,
    launch: u64,
    ingress: Ingress,
    active: bool,
    revoked: bool,
    opened: bool,
    wake: Option<sophia_9p::unix::Wake>,
    deadline: Option<Instant>,
}

/// Embeds the shared core, with a deadline wake for a Reserved conversation.
/// Session multiplexes up to 64 such owners; there is no relay thread.
pub struct RelayServer(sophia_9p::unix::Server<Relay>);
impl RelayServer {
    pub fn export_mut(&mut self) -> &mut Relay {
        self.0.export_mut()
    }
    pub fn subscription(&self) -> Option<sophia_9p::unix::Subscription> {
        self.0.subscription()
    }
    pub fn visit_poll_fds<'a>(&'a self, visit: impl FnMut(rustix::event::PollFd<'a>)) {
        self.0.visit_poll_fds(visit);
    }
    pub fn observation_is_current(&self, observation: &sophia_9p::unix::Observation) -> bool {
        self.0.observation_is_current(observation)
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.0.export().deadline
    }
    /// The owner has sampled this relay beside its other descriptors. An idle
    /// observation avoids transport work, but never delays expiry or revocation.
    pub fn turn_observed(
        &mut self,
        observation: sophia_9p::unix::Observation,
        now: Instant,
    ) -> io::Result<bool> {
        if self.0.export().revoked {
            return Ok(false);
        }
        if self.deadline().is_some_and(|end| now >= end) {
            if !self.0.flush()? {
                return Ok(false);
            }
            self.0.export_mut().deadline = None;
        }
        if self.observation_is_current(&observation) && !observation.is_ready() {
            return Ok(true);
        }
        Ok(self.0.turn_observed(observation)? && !self.0.export().revoked)
    }
    pub fn turn(&mut self, maximum: Option<Duration>) -> io::Result<bool> {
        // Retry local expiry even if the child and Factotum are both silent.
        if !self.0.flush()? {
            return Ok(false);
        }
        // With no outstanding read, expiry is reported on the next read. Do
        // not keep a past deadline armed and spin an otherwise idle owner.
        if self
            .0
            .export()
            .deadline
            .is_some_and(|end| Instant::now() >= end)
        {
            self.0.export_mut().deadline = None;
        }
        let timeout = self
            .0
            .export()
            .deadline
            .map(|end| end.saturating_duration_since(Instant::now()));
        let timeout = match (maximum, timeout) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if !self.0.turn(timeout)? {
            return Ok(false);
        }
        Ok(self.0.flush()? && !self.0.export().revoked)
    }
}

impl Relay {
    /// Bind before handing the child its endpoint. The connection is the one
    /// adopted from this socketpair, not a connection named by a wire request.
    pub fn over(stream: UnixStream, launch: u64, ingress: Ingress) -> io::Result<RelayServer> {
        if launch == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "zero launch generation",
            ));
        }
        let relay = Self {
            connection: None,
            launch,
            ingress,
            active: false,
            revoked: false,
            opened: false,
            wake: None,
            deadline: None,
        };
        let limits =
            sophia_9p::Limits::new(65536, 512, 4, 4, 131072, 1).expect("fixed relay limits");
        let mut server = sophia_9p::unix::Server::new(relay, limits)?;
        let connection = server
            .adopt(stream)
            .map_err(|_| io::Error::other("relay adoption failed"))?;
        server.export_mut().connection = Some(connection);
        let wake = server.wake();
        server.export_mut().ingress = server.export().ingress.clone().with_wake({
            let wake = wake.clone();
            move || wake.wake()
        });
        server.export_mut().wake = Some(wake);
        Ok(RelayServer(server))
    }

    /// Caller must first confirm this launch's protected spawn and Factotum
    /// activation. Activation of a different or retired reservation refuses.
    pub fn activate(&mut self, launch: u64) -> Result<(), Errno> {
        if launch != self.launch || self.revoked || self.active {
            return Err(Errno::ESTALE);
        }
        self.active = true;
        if let Some(wake) = &self.wake {
            wake.wake();
        }
        Ok(())
    }
    pub fn revoke(&mut self) {
        self.revoked = true;
        if let Some(wake) = &self.wake {
            wake.wake();
        }
    }
}

impl Export for Relay {
    type Node = Node;
    type Handle = Handle;
    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Node>, Errno> {
        if self.revoked
            || Some(context.connection) != self.connection
            || context.aname != ATTACH
            || !context.uname.is_empty()
            || context.n_uname != u32::MAX
        {
            return Err(Errno::EACCES);
        }
        Ok(Attachment {
            root: Node::Root,
            epoch: Epoch(self.launch),
        })
    }
    fn check(&mut self, access: &Access<'_, Node>) -> Result<(), Errno> {
        if self.revoked
            || Some(access.connection) != self.connection
            || access.epoch != Epoch(self.launch)
        {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }
    fn lookup(&mut self, node: &Node, name: WalkName<'_>) -> Result<Node, Errno> {
        match (node, name) {
            (Node::Root, WalkName::Child(b"rpc")) => Ok(Node::Rpc),
            (_, WalkName::Parent) => Ok(Node::Root),
            _ => Err(Errno::ENOENT),
        }
    }
    fn describe(&self, node: &Node, _: Option<&Handle>) -> Entry {
        Entry {
            kind: if *node == Node::Root {
                NodeKind::Directory
            } else {
                NodeKind::File
            },
            qid_path: if *node == Node::Root { 1 } else { 2 },
            qid_version: 0,
            permissions: if *node == Node::Root { 0o500 } else { 0o600 },
            size: 0,
        }
    }
    fn open(&mut self, node: &Node, flags: OpenFlags) -> Result<Handle, Errno> {
        if *node != Node::Rpc || flags != OpenFlags(2) {
            return Err(Errno::EACCES);
        }
        if self.opened {
            return Err(Errno(16));
        }
        self.opened = true;
        Ok(Handle {
            rpc: true,
            pending: None,
        })
    }
    fn write(
        &mut self,
        _: &Node,
        handle: &mut Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        if offset != 0 {
            return Err(Errno::EINVAL);
        }
        if handle.pending.is_some() {
            return Err(Errno::EAGAIN);
        }
        let nonce = decode_challenge(data)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        self.deadline = Some(deadline);
        handle.pending = Some(Pending {
            nonce,
            deadline,
            ticket: None,
        });
        Ok(RECORD_BYTES as u32)
    }
    fn read(
        &mut self,
        _: &Node,
        handle: &mut Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        if offset != 0 || count < STATUS_BYTES as u32 {
            return Err(Errno::EINVAL);
        }
        let pending = handle.pending.as_mut().ok_or(Errno::EAGAIN)?;
        let now = Instant::now();
        if now >= pending.deadline {
            handle.pending = None;
            self.deadline = None;
            return Err(Errno::ESTALE);
        }
        if !self.active {
            return Ok(ReadOutcome::Pending);
        }
        if pending.ticket.is_none() {
            match self.ingress.submit(
                Command::Relay {
                    launch: self.launch,
                    nonce: pending.nonce,
                },
                now,
                pending.deadline,
            ) {
                Ok(ticket) => pending.ticket = Some(ticket),
                Err(Error::Capacity) => {
                    handle.pending = None;
                    self.deadline = None;
                    return Err(Errno::EAGAIN);
                }
                Err(_) => {
                    handle.pending = None;
                    self.deadline = None;
                    return Err(Errno::EIO);
                }
            }
        }
        let result = pending.ticket.as_mut().expect("submitted relay").take(now);
        let errno = match result {
            Ok(None) => return Ok(ReadOutcome::Pending),
            Ok(Some(Response::Done)) => 0,
            Ok(Some(_)) => Errno::EIO.0,
            Err(Error::Remote(errno)) => errno.0,
            Err(Error::Deadline) => Errno::ESTALE.0,
            Err(_) => Errno::EIO.0,
        };
        handle.pending = None;
        self.deadline = None;
        let mut status = 1_u32.to_le_bytes().to_vec();
        status.extend(errno.to_le_bytes());
        Ok(ReadOutcome::Ready(status))
    }
    fn release(&mut self, _: Node, handle: Option<Handle>) {
        if handle.is_some_and(|handle| handle.rpc) {
            self.opened = false;
            self.deadline = None;
        }
    }
    fn connection_closed(&mut self, connection: ConnectionId) {
        if Some(connection) == self.connection {
            self.revoke();
        }
    }
}
