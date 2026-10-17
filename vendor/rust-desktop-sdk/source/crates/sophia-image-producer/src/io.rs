use std::ffi::CString;
use std::io::IoSlice;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags, fcntl_get_seals, fstat, open};
use rustix::net::{
    AddressFamily, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketAddrUnix,
    SocketFlags, SocketType, connect as net_connect, recv, sendmsg, socket_with,
};

use crate::ProducerError;

pub(crate) enum SendOutcome {
    Sent,
    Again,
    /// Peer disconnects read as true.
    Failed(bool),
}

pub(crate) enum Peek {
    Open,
    Data,
    Closed,
}

/// Connect a nonblocking close-on-exec SEQPACKET socket to a single
/// socket name inside `endpoint_dir`. The node is verified to be a
/// socket and reached through `/proc/self/fd`, so a swapped path
/// cannot redirect the connection. A full listener backlog is
/// `Again`; the caller retries within its startup deadline.
pub fn connect(endpoint_dir: &Path, endpoint: &str) -> Result<OwnedFd, ProducerError> {
    if endpoint.is_empty()
        || endpoint.len() > 64
        || endpoint.contains('/')
        || endpoint == "."
        || endpoint == ".."
    {
        return Err(ProducerError::Argument);
    }
    let node = open(
        endpoint_dir.join(endpoint),
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProducerError::Refused)?;
    let stat = fstat(&node).map_err(|_| ProducerError::Refused)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Socket {
        return Err(ProducerError::Refused);
    }
    let address = SocketAddrUnix::new(format!("/proc/self/fd/{}", node.as_fd().as_raw_fd()))
        .map_err(|_| ProducerError::Refused)?;
    let sock = socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::NONBLOCK | SocketFlags::CLOEXEC,
        None,
    )
    .map_err(|_| ProducerError::Refused)?;
    match net_connect(&sock, &address) {
        Ok(()) => Ok(sock),
        Err(error) if error == rustix::io::Errno::AGAIN => Err(ProducerError::Again),
        Err(_) => Err(ProducerError::Refused),
    }
}

pub(crate) fn send_packet(
    channel: BorrowedFd<'_>,
    bytes: &[u8],
    source: Option<BorrowedFd<'_>>,
) -> SendOutcome {
    let mut buffer_space = [std::mem::MaybeUninit::uninit(); 64];
    let mut ancillary = SendAncillaryBuffer::new(&mut buffer_space);
    let rights = source.map(|fd| [fd]);
    if let Some(rights) = &rights {
        ancillary.push(SendAncillaryMessage::ScmRights(rights));
    }
    // EINTR retries the same packet, as in the C producer: the send
    // did not happen, so this is not an ambiguous outcome.
    loop {
        match sendmsg(
            channel,
            &[IoSlice::new(bytes)],
            &mut ancillary,
            SendFlags::NOSIGNAL | SendFlags::DONTWAIT,
        ) {
            Ok(sent) if sent == bytes.len() => return SendOutcome::Sent,
            Ok(_) => return SendOutcome::Failed(false),
            Err(error) if error == rustix::io::Errno::INTR => continue,
            Err(error) if error == rustix::io::Errno::AGAIN => return SendOutcome::Again,
            Err(error) => {
                return SendOutcome::Failed(
                    error == rustix::io::Errno::PIPE || error == rustix::io::Errno::CONNRESET,
                );
            }
        }
    }
}

/// The owner never writes on this socket: readable data is a fault
/// and end-of-stream is the peer's disconnect.
pub(crate) fn peek_disconnect(channel: BorrowedFd<'_>) -> Peek {
    let mut byte = [0u8; 1];
    match recv(channel, &mut byte, RecvFlags::DONTWAIT | RecvFlags::PEEK) {
        Ok((0, _)) => Peek::Closed,
        Ok(_) => Peek::Data,
        Err(error) if error == rustix::io::Errno::AGAIN || error == rustix::io::Errno::INTR => {
            Peek::Open
        }
        Err(_) => Peek::Closed,
    }
}

/// A regular file of exactly the declared bytes, sealed against
/// every change. A read-only mapping alone would prove nothing.
pub(crate) fn sealed_source(fd: BorrowedFd<'_>, bytes: u64) -> bool {
    use rustix::fs::SealFlags;
    let required = SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL;
    let Ok(seals) = fcntl_get_seals(fd) else {
        return false;
    };
    if !seals.contains(required) {
        return false;
    }
    let Ok(stat) = fstat(fd) else {
        return false;
    };
    FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile
        && stat.st_size >= 0
        && stat.st_size as u64 == bytes
}

pub(crate) fn c_name(text: &str) -> Result<CString, ProducerError> {
    CString::new(text).map_err(|_| ProducerError::Argument)
}

/// True when the descriptor is a SEQPACKET socket. `attach` requires
/// packet semantics; accepting a stream would silently change them.
pub(crate) fn is_seqpacket(fd: BorrowedFd<'_>) -> bool {
    rustix::net::sockopt::socket_type(fd)
        .is_ok_and(|kind| kind == rustix::net::SocketType::SEQPACKET)
}
