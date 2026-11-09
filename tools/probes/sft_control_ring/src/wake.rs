//! One poll set for work and peer exit. Notification is never authority.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ready {
    Work,
    Exited,
    Deadline,
}

pub fn wait(work: BorrowedFd<'_>, peer: BorrowedFd<'_>, timeout: Timespec) -> io::Result<Ready> {
    let mut fds = [
        PollFd::new(&work, PollFlags::IN),
        PollFd::new(&peer, PollFlags::IN),
    ];
    poll(&mut fds, Some(&timeout))?;
    if fds
        .iter()
        .any(|fd| fd.revents().intersects(PollFlags::ERR | PollFlags::NVAL))
    {
        return Err(io::Error::other("invalid wake descriptor"));
    }
    if fds[1].revents().intersects(PollFlags::IN | PollFlags::HUP) {
        return Ok(Ready::Exited);
    }
    if fds[0].revents().contains(PollFlags::HUP) {
        return Err(io::Error::other("work descriptor closed"));
    }
    if fds[0].revents().contains(PollFlags::IN) {
        return Ok(Ready::Work);
    }
    Ok(Ready::Deadline)
}
/// The owner creates BOTH directions, retaining the SQ read end and receipt write
/// end. Only their opposite ends go to the peer; file flags are never shared.
pub fn pipe() -> io::Result<(Reader, Writer)> {
    let (read, write) = rustix::pipe::pipe_with(
        rustix::pipe::PipeFlags::CLOEXEC | rustix::pipe::PipeFlags::NONBLOCK,
    )?;
    Ok((Reader(read, std::cell::Cell::new(None)), Writer(write)))
}
pub struct Reader(OwnedFd, std::cell::Cell<Option<u64>>);
pub struct Writer(OwnedFd);
impl AsFd for Reader {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl AsFd for Writer {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl Reader {
    /// Only the peer receives a reader, from the trusted owner-created bootstrap.
    pub fn from_owner(fd: OwnedFd) -> io::Result<Self> {
        let flags = rustix::fs::fcntl_getfl(&fd)?;
        if !flags.contains(rustix::fs::OFlags::NONBLOCK)
            || flags & rustix::fs::OFlags::ACCMODE != rustix::fs::OFlags::RDONLY
        {
            return Err(io::Error::other("invalid doorbell reader"));
        }
        Ok(Self(fd, std::cell::Cell::new(None)))
    }
    /// Consume one publication identity before touching reused shared storage.
    /// Duplicate hints are idle; gaps, short reads and wrap close the carrier.
    pub fn next(&self, last: u64) -> io::Result<Option<u64>> {
        let mut bytes = [0; 8];
        let count = match rustix::io::read(&self.0, &mut bytes) {
            Err(rustix::io::Errno::AGAIN) => return Ok(None),
            result => result?,
        };
        if count != bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short publication hint",
            ));
        }
        let sequence = u64::from_le_bytes(bytes);
        if sequence != 0 && sequence == last {
            if self.1.replace(Some(last)) == Some(last) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "repeated stale publication hint",
                ));
            }
            return Ok(None);
        }
        if sequence == 0 || sequence == u64::MAX || last.checked_add(1) != Some(sequence) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "publication hint gap or wrap",
            ));
        }
        self.1.set(None);
        Ok(Some(sequence))
    }
}
impl Writer {
    /// Only the peer receives this write end; the owner retains the opposite end.
    pub fn from_owner(fd: OwnedFd) -> io::Result<Self> {
        let flags = rustix::fs::fcntl_getfl(&fd)?;
        if !flags.contains(rustix::fs::OFlags::NONBLOCK)
            || flags & rustix::fs::OFlags::ACCMODE != rustix::fs::OFlags::WRONLY
        {
            return Err(io::Error::other("invalid publication writer"));
        }
        Ok(Self(fd))
    }
    /// One eight-byte publication identity, below PIPE_BUF. EAGAIN is not delivery:
    /// dropping it could leave only stale hints, so the caller must close.
    pub fn notify(&self, sequence: u64) -> io::Result<()> {
        if sequence == 0 || sequence == u64::MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid publication sequence",
            ));
        }
        match rustix::io::write(&self.0, &sequence.to_le_bytes()) {
            Ok(8) => Ok(()),
            Err(e) => Err(e.into()),
            Ok(_) => Err(io::Error::other("short publication hint")),
        }
    }
}
