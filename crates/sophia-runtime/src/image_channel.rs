//! Bounded Linux image-channel transport primitives.
//!
//! Receiving bytes conveys no authority: the owner must join Hello to a live
//! authenticated 9P principal before admitting any source. Descriptor ownership
//! transfers into a packet only after the kernel message is known to fit.
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rustix::io::Errno;

pub const IMAGE_PACKET_BYTES: usize = 200;
pub const IMAGE_PACKET_FDS: usize = 4;
pub const IMAGE_ENDPOINT_BASENAME: &str = "image.sock";

/// Listener beneath an existing private role directory. It does not create or
/// authorize a role; accepted sockets still need authenticated Hello admission.
/// Drop this listener before the RoleEndpoint that owns the directory.
pub struct ImageChannelListener {
    socket: OwnedFd,
    path: PathBuf,
}

/// Connector pinned to its process. Identity checks and live afid custody
/// precede construction; each operation must still recheck both lifetimes.
pub struct ImageChannelPeer {
    socket: OwnedFd,
    process: OwnedFd,
    connection: sophia_9p::ConnectionId,
    admission: sophia_protocol::ClientAdmissionContext,
}

impl ImageChannelPeer {
    pub fn socket(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }

    pub fn check<E>(
        &self,
        files: &mut crate::authenticated_files::AuthenticatedFiles<E>,
        now: std::time::Instant,
    ) -> std::io::Result<()> {
        if !process_alive(&self.process)? {
            return Err(std::io::Error::from(Errno::STALE));
        }
        let current = files
            .check_side_channel(self.connection, now)
            .map_err(|_| std::io::Error::from(Errno::ACCESS))?;
        if current != self.admission {
            return Err(std::io::Error::from(Errno::STALE));
        }
        Ok(())
    }
}

fn process_alive(process: &OwnedFd) -> Result<bool, Errno> {
    crate::shell_work_cost::count(crate::shell_work_cost::Call::ImageAlive);
    crate::shell_work_cost::result(crate::shell_work_cost::Stage::ImageAlive, 0, || {
        let mut ready = [rustix::event::PollFd::new(
            process,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(
            &mut ready,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )?;
        Ok(ready[0].revents().is_empty())
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageChannelRole {
    Shell,
    Lock,
}

/// A fixed Hello deadline and expected identity for a single accepted socket.
/// Passing this barrier does not publish ChannelReady: the owner must first
/// reserve its terminal journal slot and successfully publish that event.
pub struct ImageHelloPending {
    peer: ImageChannelPeer,
    role: ImageChannelRole,
    epoch: u64,
    generation: u64,
    deadline: std::time::Instant,
    checked: bool,
    failed: bool,
}

impl ImageHelloPending {
    pub fn new(
        peer: ImageChannelPeer,
        role: ImageChannelRole,
        epoch: u64,
        generation: u64,
        deadline: std::time::Instant,
    ) -> std::io::Result<Self> {
        if epoch == 0 || generation == 0 {
            return Err(std::io::Error::from(Errno::INVAL));
        }
        Ok(Self {
            peer,
            role,
            epoch,
            generation,
            deadline,
            checked: false,
            failed: false,
        })
    }

    pub fn socket(&self) -> BorrowedFd<'_> {
        self.peer.socket()
    }
    pub fn deadline(&self) -> std::time::Instant {
        self.deadline
    }

    /// One receive at most. A checked Hello is retained while the owner waits
    /// for journal capacity, but the original deadline still bounds readiness.
    pub fn poll<E>(
        &mut self,
        files: &mut crate::authenticated_files::AuthenticatedFiles<E>,
        now: std::time::Instant,
    ) -> std::io::Result<bool> {
        if self.failed {
            return Err(std::io::Error::from(Errno::PIPE));
        }
        let result = self.poll_inner(files, now);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn poll_inner<E>(
        &mut self,
        files: &mut crate::authenticated_files::AuthenticatedFiles<E>,
        now: std::time::Instant,
    ) -> std::io::Result<bool> {
        self.peer.check(files, now)?;
        if now >= self.deadline {
            return Err(std::io::Error::from(Errno::TIMEDOUT));
        }
        if self.checked {
            return Ok(true);
        }
        let Some(packet) = receive_image_datagram(self.peer.socket())? else {
            return Ok(false);
        };
        if packet.descriptor_count() != 0 {
            return Err(std::io::Error::from(Errno::PROTO));
        }
        let matches = match self.role {
            ImageChannelRole::Shell => matches!(
                sophia_protocol::shell_files::ShellImagePacket::decode(packet.bytes()),
                Ok(sophia_protocol::shell_files::ShellImagePacket::Hello { connection_epoch, channel_generation })
                    if connection_epoch == self.epoch && channel_generation == self.generation),
            ImageChannelRole::Lock => matches!(
                sophia_protocol::lock_files::LockImagePacket::decode(packet.bytes()),
                Ok(sophia_protocol::lock_files::LockImagePacket::Hello { connection_epoch, channel_generation })
                    if connection_epoch == self.epoch && channel_generation == self.generation),
        };
        if !matches {
            return Err(std::io::Error::from(Errno::PROTO));
        }
        self.checked = true;
        Ok(true)
    }

    /// Call only after ChannelReady has been journalled. This type records
    /// the Hello check; journal ordering remains the role owner's obligation.
    pub fn into_peer(self) -> Result<ImageChannelPeer, Self> {
        if self.checked && !self.failed {
            Ok(self.peer)
        } else {
            Err(self)
        }
    }
}

impl ImageChannelListener {
    pub fn bind(directory: &Path) -> std::io::Result<Self> {
        let metadata = std::fs::symlink_metadata(directory)?;
        if !metadata.is_dir()
            || metadata.mode() & 0o077 != 0
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(std::io::Error::from(Errno::ACCESS));
        }
        let path = directory.join(IMAGE_ENDPOINT_BASENAME);
        let socket = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::SEQPACKET,
            rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
            None,
        )?;
        rustix::net::bind(&socket, &rustix::net::SocketAddrUnix::new(&path)?)?;
        let listener = Self { socket, path };
        std::fs::set_permissions(&listener.path, std::fs::Permissions::from_mode(0o600))?;
        rustix::net::listen(&listener.socket, 4)?;
        Ok(listener)
    }

    pub fn readiness(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }

    /// One accept per call bounds work under a stream of unauthorized peers.
    /// A caller must close a returned socket unless its Hello joins live 9P
    /// custody. Credential equality alone never grants image authority.
    fn accept(&self) -> Result<Option<OwnedFd>, Errno> {
        match rustix::net::accept_with(
            &self.socket,
            rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
        ) {
            Ok(socket) => Ok(Some(socket)),
            Err(Errno::AGAIN) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Joins one connector to the role's active 9P connection. Refusal drops
    /// only this connector; it cannot replace or close another live channel.
    /// The caller still validates Hello's epoch/generation before ChannelReady.
    pub fn accept_authenticated<E>(
        &self,
        expected: crate::PolicyPeerIdentity,
        connection: sophia_9p::ConnectionId,
        files: &mut crate::authenticated_files::AuthenticatedFiles<E>,
        now: std::time::Instant,
    ) -> std::io::Result<Option<ImageChannelPeer>> {
        let Some(socket) = self.accept()? else {
            return Ok(None);
        };
        let credentials = rustix::net::sockopt::socket_peercred(&socket)?;
        if credentials.uid.as_raw() != expected.uid
            || credentials.pid.as_raw_pid() as u32 != expected.pid
        {
            return Err(std::io::Error::from(Errno::ACCESS));
        }
        let process = sophia_linux_peer::socket_peer_pidfd(socket.as_fd())?;
        if !process_alive(&process)? {
            return Err(std::io::Error::from(Errno::STALE));
        }
        let admission = files
            .check_side_channel(connection, now)
            .map_err(|_| std::io::Error::from(Errno::ACCESS))?;
        Ok(Some(ImageChannelPeer {
            socket,
            process,
            connection,
            admission,
        }))
    }
}

impl Drop for ImageChannelListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One complete datagram. Dropping it closes every received descriptor.
pub struct ImageDatagram {
    bytes: [u8; IMAGE_PACKET_BYTES],
    length: usize,
    descriptors: [Option<OwnedFd>; IMAGE_PACKET_FDS],
    count: usize,
}

impl ImageDatagram {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }

    pub fn descriptor_count(&self) -> usize {
        self.count
    }

    /// Takes a received reference once; the packet closes all untaken ones.
    pub fn take_descriptor(&mut self, index: usize) -> Option<OwnedFd> {
        self.descriptors.get_mut(index)?.take()
    }
}

/// Receives at most one packet without blocking or allocating. `None` means
/// the socket would block. Truncation, empty messages and unexpected ancillary
/// types fail closed; all references delivered with those messages are closed.
pub fn receive_image_datagram(socket: BorrowedFd<'_>) -> Result<Option<ImageDatagram>, Errno> {
    let mut packet = ImageDatagram {
        bytes: [0; IMAGE_PACKET_BYTES],
        length: 0,
        descriptors: std::array::from_fn(|_| None),
        count: 0,
    };
    let message = match sophia_linux_peer::receive_rights(socket, &mut packet.bytes) {
        Ok(message) => message,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(error) => return Err(Errno::from_raw_os_error(error.raw_os_error().unwrap_or(5))),
    };
    if message.bytes == 0 && message.count == 0 {
        let mut state = [rustix::event::PollFd::new(
            &socket,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(
            &mut state,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )?;
        if state[0].revents().contains(rustix::event::PollFlags::HUP) {
            return Err(Errno::PIPE);
        }
    }
    if message.bytes == 0 || message.bytes > IMAGE_PACKET_BYTES {
        return Err(Errno::PROTO);
    }
    packet.length = message.bytes;
    packet.descriptors = message.descriptors;
    packet.count = message.count;
    Ok(Some(packet))
}
