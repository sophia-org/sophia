//! One poll set for work and peer exit. Notification is never authority.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
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
/// Clear BEFORE inspecting the queue. A later publication stays readable.
pub fn clear(work: impl AsFd) -> io::Result<()> {
    let mut bytes = [0; 8];
    match rustix::io::read(work, &mut bytes) {
        Ok(8) | Err(rustix::io::Errno::AGAIN) => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => Err(io::Error::other("short eventfd read")),
    }
}
