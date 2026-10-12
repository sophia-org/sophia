//! Cross-worker observation of one supervisor-owned launch.
use std::{
    io,
    os::fd::OwnedFd,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// A custody check, not an authentication verdict. Only the supervisor can
/// create it, after matching the exact reservation and authorized custody role. Kernel
/// handles prevent PID reuse; the owner flag withdraws custody before signals.
/// Clones cannot extend the launch lifetime or reactivate a retired launch.
/// Host namespace clients have no containment evidence; both handles name the
/// directly spawned child. Protected roles retain wrapper and peer handles.
#[derive(Clone, Debug)]
pub struct LaunchCustody {
    inner: Arc<Observation>,
}

#[derive(Debug)]
struct Observation {
    active: Arc<AtomicBool>,
    wrapper: OwnedFd,
    peer: OwnedFd,
}

impl LaunchCustody {
    pub(super) fn new(active: Arc<AtomicBool>, wrapper: OwnedFd, peer: OwnedFd) -> Self {
        Self {
            inner: Arc::new(Observation {
                active,
                wrapper,
                peer,
            }),
        }
    }

    /// Zero-time poll of both kernel identities. An exit is observed without
    /// waiting for Session to reap it. Any poll error refuses custody.
    pub fn is_live(&self) -> io::Result<bool> {
        if !self.inner.active.load(Ordering::Acquire) {
            return Ok(false);
        }
        let mut fds = [
            rustix::event::PollFd::new(&self.inner.wrapper, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&self.inner.peer, rustix::event::PollFlags::IN),
        ];
        let quiet = rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )? == 0;
        Ok(quiet && self.inner.active.load(Ordering::Acquire))
    }
}
