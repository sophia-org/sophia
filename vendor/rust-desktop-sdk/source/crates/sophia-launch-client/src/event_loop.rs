use super::*;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use sophia_9p_client::pipeline::IoInterest;
use std::os::fd::BorrowedFd;

impl Client {
    fn wire_fd(&self, relay: bool) -> Option<BorrowedFd<'_>> {
        if self.failed.is_some() {
            return None;
        }
        let index = usize::from(relay);
        if let Some(version) = &self.versions[index] {
            version.fd()
        } else if relay {
            self.relay.as_ref().map(AsFd::as_fd)
        } else {
            self.role.as_ref().map(AsFd::as_fd)
        }
    }

    /// Borrow only for readiness monitoring. The client owns all wire I/O.
    pub fn role_fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire_fd(false)
    }

    /// Borrow only for readiness monitoring; never delegate this capability.
    pub fn relay_fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire_fd(true)
    }

    fn interest(&self, relay: bool) -> IoInterest {
        if self.failed.is_some() || self.ready() {
            return IoInterest {
                readable: false,
                writable: false,
                immediate: false,
            };
        }
        if let Some(version) = &self.versions[usize::from(relay)] {
            return version.io_interest();
        }
        let pipe = if relay { &self.relay } else { &self.role };
        // A transport error must wake dispatch so it can latch the failure.
        let mut interest = pipe.as_ref().unwrap().io_interest().unwrap_or(IoInterest {
            readable: false,
            writable: false,
            immediate: true,
        });
        interest.immediate |= self.versions.iter().all(Option::is_none)
            && self.pending.is_none()
            && self.now >= self.retry;
        interest
    }

    pub fn role_interest(&self) -> IoInterest {
        self.interest(false)
    }

    pub fn relay_interest(&self) -> IoInterest {
        self.interest(true)
    }

    /// Absolute caller-clock milliseconds, or None after completion/failure.
    /// An already-due time means dispatch local work before sleeping. Monitor
    /// both descriptors for errors/hangup as well as these read/write interests.
    pub fn next_wake(&self) -> Option<u64> {
        if self.failed.is_some() || self.ready() {
            return None;
        }
        Some(
            if self.versions.iter().all(Option::is_none) && self.pending.is_none() {
                self.retry.max(self.now).min(self.deadline)
            } else {
                self.deadline
            },
        )
    }

    pub(super) fn wall_now(&self) -> u64 {
        self.origin_ms
            .saturating_add(u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX))
    }

    /// Convenience wait for `poll` callers, using its clock and both wires.
    /// External-loop callers use descriptors, interests and `next_wake` instead.
    pub fn wait_for_io(&self, maximum: Duration) -> Result<(), Error> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        let Some(wake) = self.next_wake() else {
            return Ok(());
        };
        let maximum = match self.wall_deadline {
            Some(end) => maximum.min(remaining(end)?),
            None => maximum,
        };
        let now = self.wall_now();
        if now >= self.deadline {
            return Err(Error::Timeout);
        }
        let role = self.role_interest();
        let relay = self.relay_interest();
        if role.immediate || relay.immediate || wake <= now {
            return Ok(());
        }
        let timeout = Timespec::try_from(Duration::from_millis(wake - now).min(maximum))
            .map_err(|_| Error::Invalid("wait duration"))?;
        let role_fd = self.role_fd().ok_or(Error::NotReady)?;
        let relay_fd = self.relay_fd().ok_or(Error::NotReady)?;
        let flags = |interest: IoInterest| {
            let mut flags = PollFlags::empty();
            if interest.readable {
                flags |= PollFlags::IN;
            }
            if interest.writable {
                flags |= PollFlags::OUT;
            }
            flags
        };
        match poll(
            &mut [
                PollFd::new(&role_fd, flags(role)),
                PollFd::new(&relay_fd, flags(relay)),
            ],
            Some(&timeout),
        ) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(e) => Err(Error::Wire(PipelineError::Io(
                std::io::Error::from(e).kind(),
            ))),
        }
    }
}
