//! Opt-in authority boundary, not a replacement for caller-fresh `is_live`.
use super::LaunchCustody;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::{
    borrow::Cow,
    io,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

/// Expired or Exhausted requires caller-fresh verification or a new bounded
/// dispatch before more work. Neither means the peer died. Unregistered
/// means the caller used the wrong launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyDispatchLimit {
    Expired,
    Exhausted,
    Unregistered,
}

/// One aggregate kernel observation followed by bounded in-memory checks.
///
/// This deliberately observes exit at dispatch boundaries. A process can die
/// after `observe` and remain authorized here until a subsequent observation.
/// Explicit withdrawal, or an exit applied by another observation, is visible
/// to every check immediately through the shared monotonic custody state.
///
/// Consumers must choose an explicit age/check budget and process every
/// nonblocking continuation. Each worker needs its own boundary. A check
/// bounds admission of work, not its execution time.
/// The object cannot be cloned; limits cannot be renewed without a new poll.
#[derive(Debug)]
pub struct CustodyDispatch<'a> {
    custodies: Cow<'a, [LaunchCustody]>,
    deadline: Instant,
    remaining: usize,
}

impl<'a> CustodyDispatch<'a> {
    /// Poll both exact process identities for each registered launch together.
    /// A positive exit observation revokes directly, without waiting for reap.
    /// Interrupted polls return no dispatch; other poll errors revoke the set.
    /// Allocation is currently per observation; this is a mechanism experiment.
    pub fn observe(
        custodies: &'a [LaunchCustody],
        maximum_age: Duration,
        maximum_checks: usize,
    ) -> io::Result<Self> {
        Self::observe_with(custodies, maximum_age, maximum_checks, |fds| {
            poll(fds, Some(&Timespec::default())).map(drop)
        })
    }

    // Private seam: public callers cannot fabricate successful observations.
    // INTR grants no new dispatch or credit; existing dispatches retain their
    // original limits. Other observer failures revoke the registered set.
    fn observe_with(
        custodies: &'a [LaunchCustody],
        maximum_age: Duration,
        maximum_checks: usize,
        observe: impl FnOnce(&mut [PollFd<'_>]) -> Result<(), rustix::io::Errno>,
    ) -> io::Result<Self> {
        if custodies.is_empty() || maximum_age.is_zero() || maximum_checks == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty custody dispatch",
            ));
        }
        // Distinct handles issued for the same managed launch share its flag.
        // Reject duplicate registration rather than polling that identity twice.
        for (i, custody) in custodies.iter().enumerate() {
            if custodies[..i]
                .iter()
                .any(|other| Arc::ptr_eq(&custody.inner.active, &other.inner.active))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "duplicate custody",
                ));
            }
        }
        let deadline = Instant::now()
            .checked_add(maximum_age)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "custody age overflow"))?;
        let mut fds = Vec::with_capacity(custodies.len() * 2);
        for custody in custodies {
            fds.push(PollFd::new(&custody.inner.wrapper, PollFlags::IN));
            fds.push(PollFd::new(&custody.inner.peer, PollFlags::IN));
        }
        if let Err(error) = observe(&mut fds) {
            if error != rustix::io::Errno::INTR {
                for custody in custodies {
                    custody.inner.active.store(false, Ordering::Release);
                }
            }
            return Err(error.into());
        }
        for (custody, pair) in custodies.iter().zip(fds.chunks_exact(2)) {
            if pair.iter().any(|fd| !fd.revents().is_empty()) {
                custody.inner.active.store(false, Ordering::Release);
            }
        }
        Ok(Self {
            custodies: Cow::Borrowed(custodies),
            deadline,
            remaining: maximum_checks,
        })
    }

    /// Retain the same observation and limits independently of its input slice.
    /// This does not poll, renew credit, or extend the observation's lifetime.
    pub fn into_owned(self) -> CustodyDispatch<'static> {
        CustodyDispatch {
            custodies: Cow::Owned(self.custodies.into_owned()),
            deadline: self.deadline,
            remaining: self.remaining,
        }
    }

    /// Does this dispatch still permit a check for this exact launch?
    /// No kernel liveness query occurs here. Admission, grant, epoch, connection
    /// and role checks remain the caller's separate obligations on every use.
    pub fn check(&mut self, custody: &LaunchCustody) -> Result<bool, CustodyDispatchLimit> {
        if !self
            .custodies
            .iter()
            .any(|registered| Arc::ptr_eq(&registered.inner.active, &custody.inner.active))
        {
            return Err(CustodyDispatchLimit::Unregistered);
        }
        if Instant::now() >= self.deadline {
            return Err(CustodyDispatchLimit::Expired);
        }
        if self.remaining == 0 {
            return Err(CustodyDispatchLimit::Exhausted);
        }
        self.remaining -= 1;
        Ok(custody.inner.active.load(Ordering::Acquire))
    }
}

#[cfg(test)]
#[path = "../../../tests/support/custody_dispatch_faults.rs"]
mod fault_controls;
