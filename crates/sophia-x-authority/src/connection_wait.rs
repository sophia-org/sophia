//! Waiting without sleeping, for connections that must stay cancellable.
//!
//! A connection that sleeps observes nothing: not the peer leaving, not a
//! revocation that should cancel what it awaits, not the completion it awaits.
//! The primitives here let a paused connection wait on all three at once, in
//! one `poll`.

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};

/// Flags that mean the peer is gone rather than merely quiet.
const DEPARTED: PollFlags = PollFlags::HUP.union(PollFlags::ERR).union(PollFlags::NVAL);

/// A wake source that can share a `poll` set with a connection's socket.
///
/// An ordinary channel cannot be polled alongside a descriptor, so a connection
/// waiting on one would have to choose between noticing a completion and
/// noticing that its peer left. This notices both.
#[derive(Debug)]
pub struct ConnectionNotifier {
    descriptor: Arc<OwnedFd>,
}

impl ConnectionNotifier {
    pub fn new() -> io::Result<Self> {
        let descriptor = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?;
        Ok(Self {
            descriptor: Arc::new(descriptor),
        })
    }

    /// A handle a registry can keep without keeping the waiter alive.
    ///
    /// Registries outlive individual connections, so holding a strong
    /// reference would pin every departed connection's descriptor until the
    /// namespace itself went away.
    pub fn subscription(&self) -> NotifierSubscription {
        NotifierSubscription {
            descriptor: Arc::downgrade(&self.descriptor),
        }
    }

    fn drain(&self) {
        let mut counter = [0u8; 8];
        let _ = rustix::io::read(&*self.descriptor, &mut counter);
    }
}

/// A registry's reference to one waiter.
#[derive(Clone, Debug)]
pub struct NotifierSubscription {
    descriptor: Weak<OwnedFd>,
}

impl NotifierSubscription {
    fn is_same(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.descriptor, &other.descriptor)
    }

    /// Wake the subscriber, reporting whether it still exists.
    pub fn notify(&self) -> bool {
        match self.descriptor.upgrade() {
            Some(descriptor) => {
                notify_descriptor(&descriptor);
                true
            }
            None => false,
        }
    }
}

/// Everyone waiting on one shared condition.
#[derive(Clone, Debug, Default)]
pub struct NotifierRegistry {
    subscribers: Vec<NotifierSubscription>,
}

impl NotifierRegistry {
    /// Subscribe a waiter, at most once.
    ///
    /// A connection that wakes, finds the condition still unmet and parks
    /// again would otherwise add an entry per round, so a client blocked
    /// across many wakes would grow the list without bound.
    pub fn register(&mut self, notifier: &ConnectionNotifier) {
        let subscription = notifier.subscription();
        if self
            .subscribers
            .iter()
            .any(|existing| existing.is_same(&subscription))
        {
            return;
        }
        self.subscribers.push(subscription);
    }

    /// Wake every live subscriber and forget the departed ones.
    ///
    /// Pruning here rather than on disconnect means a connection that dies
    /// without unregistering costs one dead entry until the next wake, not a
    /// permanent leak.
    pub fn notify_all(&mut self) {
        self.subscribers.retain(NotifierSubscription::notify);
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }
}

fn notify_descriptor(descriptor: &OwnedFd) {
    // The only write a healthy eventfd refuses is one that would overflow the
    // counter, which means a wake is already pending and this one would add
    // nothing.
    let _ = rustix::io::write(descriptor, &1u64.to_ne_bytes());
}

/// Why a wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionWake {
    /// The notifier fired: an acceptance, a completion, or a revocation.
    Notified,
    /// The peer is fully gone. Whatever was awaited is moot.
    Departed,
    /// The deadline arrived with nothing else to report.
    Deadline,
}

/// A cancellable wait on one connection's socket.
pub struct ConnectionWait<'fd> {
    stream: BorrowedFd<'fd>,
    notifier: &'fd ConnectionNotifier,
    half_closed: bool,
    /// How many times `poll` has returned during this wait.
    ///
    /// The latch below has no effect on any outcome -- a wait that spun on a
    /// permanently set `POLLRDHUP` would still end for the same reason, just
    /// after burning a core to get there. Counting rounds is what makes the
    /// difference observable, and therefore testable.
    poll_rounds: u32,
}

impl<'fd> ConnectionWait<'fd> {
    pub fn new(stream: BorrowedFd<'fd>, notifier: &'fd ConnectionNotifier) -> Self {
        Self {
            stream,
            notifier,
            half_closed: false,
            poll_rounds: 0,
        }
    }

    /// Whether the peer has closed its writing half.
    ///
    /// This is not a reason to stop. Bytes the peer already sent remain
    /// buffered, and it is still entitled to replies for them.
    pub fn half_closed(&self) -> bool {
        self.half_closed
    }

    pub fn poll_rounds(&self) -> u32 {
        self.poll_rounds
    }

    /// Wait until the notifier fires, the peer departs, or `deadline` passes.
    ///
    /// `POLLIN` on the stream is deliberately not a wake reason. With ingress
    /// paused, ordinary unread pipelined bytes keep it set, so a wait that
    /// included it would return at once and forever without the peer having
    /// done anything. Departure is concluded from the hangup flags instead,
    /// which is also the only way to tell a peer that left from one that is
    /// merely talkative.
    pub fn wait_until(&mut self, deadline: Option<Instant>) -> io::Result<ConnectionWake> {
        loop {
            let remaining = match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Ok(ConnectionWake::Deadline);
                    }
                    Some(timespec_of(deadline - now))
                }
                None => None,
            };

            // `POLLRDHUP` stays set once the peer half-closes, so keeping
            // interest in it past the first sighting would spin this loop for
            // as long as the wait lasts. It is recorded once and dropped.
            let interest = if self.half_closed {
                PollFlags::empty()
            } else {
                PollFlags::RDHUP
            };
            let stream = self.stream;
            let notifier = self.notifier.descriptor.as_fd();
            let mut descriptors = [
                PollFd::new(&stream, interest),
                PollFd::new(&notifier, PollFlags::IN),
            ];

            self.poll_rounds = self.poll_rounds.saturating_add(1);
            let ready = match poll(&mut descriptors, remaining.as_ref()) {
                Ok(ready) => ready,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            };
            let stream_events = descriptors[0].revents();
            let notifier_events = descriptors[1].revents();

            if stream_events.intersects(DEPARTED) {
                return Ok(ConnectionWake::Departed);
            }
            if notifier_events.intersects(PollFlags::IN) {
                self.notifier.drain();
                return Ok(ConnectionWake::Notified);
            }
            if notifier_events.intersects(DEPARTED) {
                // Losing the notifier means revocation can no longer be
                // observed, which is a reason to give up rather than to wait
                // on a signal that can never arrive.
                return Ok(ConnectionWake::Departed);
            }
            if stream_events.contains(PollFlags::RDHUP) {
                self.half_closed = true;
                continue;
            }
            if ready == 0 {
                return Ok(ConnectionWake::Deadline);
            }
        }
    }
}

fn timespec_of(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: duration.subsec_nanos().into(),
    }
}
