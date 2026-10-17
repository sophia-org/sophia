//! Bounded Linux image-channel transport primitives.
//!
//! Receiving bytes conveys no authority: the owner must join Hello to a live
//! authenticated 9P principal before admitting any source. Descriptor ownership
//! transfers into a packet only after the kernel message is known to fit.
use std::io::IoSliceMut;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rustix::io::Errno;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, recvmsg};

pub const IMAGE_PACKET_BYTES: usize = 200;
pub const IMAGE_PACKET_FDS: usize = 4;

/// Listener beneath an existing private role directory. It does not create or
/// authorize a role; accepted sockets still need authenticated Hello admission.
pub struct ImageChannelListener {
    socket: OwnedFd,
    path: PathBuf,
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
        let path = directory.join("image.sock");
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
    pub fn accept(&self) -> Result<Option<OwnedFd>, Errno> {
        match rustix::net::accept_with(
            &self.socket,
            rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
        ) {
            Ok(socket) => Ok(Some(socket)),
            Err(Errno::AGAIN) => Ok(None),
            Err(error) => Err(error),
        }
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
    let mut control = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(IMAGE_PACKET_FDS))];
    let mut ancillary = RecvAncillaryBuffer::new(&mut control);
    let message = match recvmsg(
        socket,
        &mut [IoSliceMut::new(&mut packet.bytes)],
        &mut ancillary,
        RecvFlags::CMSG_CLOEXEC | RecvFlags::DONTWAIT,
    ) {
        Ok(message) => message,
        Err(Errno::AGAIN) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut invalid = message.bytes == 0
        || message.bytes > IMAGE_PACKET_BYTES
        || message
            .flags
            .intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC);
    for message in ancillary.drain() {
        match message {
            RecvAncillaryMessage::ScmRights(rights) => {
                for fd in rights {
                    if packet.count == IMAGE_PACKET_FDS {
                        invalid = true;
                    } else {
                        packet.descriptors[packet.count] = Some(fd);
                        packet.count += 1;
                    }
                }
            }
            _ => invalid = true,
        }
    }
    if invalid {
        return Err(Errno::PROTO);
    }
    packet.length = message.bytes;
    Ok(Some(packet))
}
