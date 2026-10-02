//! Coalesced notifications for a single owner of queues and pollable devices.
//!
//! Publish work before notifying. The consumer clears the wake BEFORE checking
//! its queues, then polls: a publication after that check remains readable.
//! Notifications are hints to inspect owned state, never work or authority.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};

pub mod channel;
mod sender;
pub use sender::SignalSender;

#[derive(Debug)]
pub struct Wake {
    fd: Arc<OwnedFd>,
}

impl Wake {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            fd: Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?),
        })
    }

    pub fn notifier(&self) -> Notifier {
        Notifier {
            fd: Arc::downgrade(&self.fd),
        }
    }

    /// Consume accumulated notifications, without discarding any queued work.
    pub fn clear(&self) -> io::Result<()> {
        let mut counter = [0u8; 8];
        loop {
            match rustix::io::read(&*self.fd, &mut counter) {
                Ok(_) | Err(rustix::io::Errno::AGAIN) => return Ok(()),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Wait without consuming the notification. `false` means the deadline.
    pub fn wait(&self, deadline: Option<Instant>) -> io::Result<bool> {
        wait(&mut [PollFd::new(self, PollFlags::IN)], deadline)
    }
}

impl AsFd for Wake {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

/// A producer does not keep an abandoned consumer's descriptor alive.
#[derive(Clone, Debug)]
pub struct Notifier {
    fd: Weak<OwnedFd>,
}

impl Notifier {
    pub fn notify(&self) {
        let Some(fd) = self.fd.upgrade() else { return };
        loop {
            match rustix::io::write(&*fd, &1u64.to_ne_bytes()) {
                Ok(8) | Err(rustix::io::Errno::AGAIN) => return,
                Err(rustix::io::Errno::INTR) => continue,
                // The strong reference owns a valid, writable eventfd. Other
                // failures are invariant failures, not a delivered wake.
                result => panic!("owned eventfd notification failed: {result:?}"),
            }
        }
    }
}

/// A producer can exist before the owner installs its wait. Attaching rings
/// once so work published before attachment is also inspected.
#[derive(Clone, Debug, Default)]
pub struct WakeSlot(Arc<Mutex<Option<Notifier>>>);

impl WakeSlot {
    pub fn set(&self, notifier: Notifier) {
        let mut slot = self.0.lock().unwrap_or_else(|error| error.into_inner());
        notifier.notify();
        *slot = Some(notifier);
    }

    pub fn notify(&self) {
        if let Some(notifier) = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            notifier.notify();
        }
    }

    pub fn attached(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some()
    }

    pub fn notifier(&self) -> Option<Notifier> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// Poll device and notification descriptors against one absolute deadline.
/// Signals never extend the deadline, and descriptor errors wake the owner.
pub fn wait(fds: &mut [PollFd<'_>], deadline: Option<Instant>) -> io::Result<bool> {
    loop {
        let remaining = deadline.map(|end| end.saturating_duration_since(Instant::now()));
        let timeout = remaining.map(|duration| Timespec {
            tv_sec: duration.as_secs().try_into().unwrap_or(i64::MAX),
            tv_nsec: i64::from(duration.subsec_nanos()),
        });
        match poll(fds, timeout.as_ref()) {
            Ok(count) => return Ok(count != 0),
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        }
    }
}
