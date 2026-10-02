//! The Session owner's one wake.
//!
//! Producers publish into their own queues, then ring this. The owner clears
//! it at the top of every pass, before it inspects any queue, so a ring that
//! lands after an inspection stays readable and the following wait returns at
//! once. A ring is only a reason to look again: it carries no work, no
//! decision and no authority.
//!
//! Sockets the owner serves inline, which no producer can ring for, join the
//! same wait as borrowed descriptors. Their readiness is a reason to look
//! again in the same sense.

use std::io;
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SendError, SyncSender, TryRecvError, TrySendError,
};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags};

pub(super) struct OwnerWake {
    wake: sophia_wake::Wake,
}

impl OwnerWake {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            wake: sophia_wake::Wake::new()?,
        })
    }

    pub(super) fn notifier(&self) -> sophia_wake::Notifier {
        self.wake.notifier()
    }

    /// Consumes the rings delivered so far. Call before inspecting any
    /// producer; never between an inspection and the wait that follows it.
    pub(super) fn begin_pass(&self) -> io::Result<()> {
        self.wake.clear()
    }

    /// Takes one item, or sleeps until any producer rings or `timeout` ends.
    ///
    /// A ring for other work ends the wait as `Timeout`, so the owner's next
    /// pass inspects that work. Rings never extend the deadline.
    #[cfg(test)]
    pub(super) fn receive<T>(
        &self,
        receiver: &Receiver<T>,
        timeout: Duration,
    ) -> Result<T, RecvTimeoutError> {
        self.receive_with_fds(receiver, timeout, Vec::new())
            .unwrap()
    }

    /// [`Self::receive`], also ending the wait when a borrowed descriptor is
    /// ready. The ring and the descriptors share one poll(2) and one deadline.
    ///
    /// Readiness carries no work either: the wait ends as `Timeout` and the
    /// next pass serves the socket. Socket readiness is level-triggered, so a
    /// caller subscribes only what that pass consumes; anything else would end
    /// every wait at once.
    pub(super) fn receive_with_fds<T>(
        &self,
        receiver: &Receiver<T>,
        timeout: Duration,
        fds: Vec<PollFd<'_>>,
    ) -> io::Result<Result<T, RecvTimeoutError>> {
        match receiver.try_recv() {
            Ok(item) => return Ok(Ok(item)),
            Err(TryRecvError::Disconnected) => return Ok(Err(RecvTimeoutError::Disconnected)),
            Err(TryRecvError::Empty) => {}
        }
        let now = Instant::now();
        let mut fds: Vec<PollFd<'_>> = fds;
        fds.push(PollFd::new(&self.wake, PollFlags::IN));
        // A failed wait is a Session error, not a delivered wake or a reason
        // to silently fall back to polling with a fresh deadline.
        sophia_wake::wait(&mut fds, Some(now.checked_add(timeout).unwrap_or(now)))?;
        Ok(match receiver.try_recv() {
            Ok(item) => Ok(item),
            Err(TryRecvError::Empty) => Err(RecvTimeoutError::Timeout),
            Err(TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
        })
    }
}

/// A queue into a worker that sleeps until rung.
///
/// Session holds the notifying sender for every such worker. Helpers take this
/// rather than a concrete type so a bare channel, which the worker keeps
/// polling, still drives them in isolation.
pub(super) trait SessionSender<T> {
    fn send(&self, value: T) -> Result<(), SendError<T>>;
    fn try_send(&self, value: T) -> Result<(), TrySendError<T>>;
}

impl<T> SessionSender<T> for SyncSender<T> {
    fn send(&self, value: T) -> Result<(), SendError<T>> {
        SyncSender::send(self, value)
    }

    fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        SyncSender::try_send(self, value)
    }
}

impl<T> SessionSender<T> for sophia_wake::SignalSender<T> {
    fn send(&self, value: T) -> Result<(), SendError<T>> {
        sophia_wake::SignalSender::send(self, value)
    }

    fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        sophia_wake::SignalSender::try_send(self, value)
    }
}

#[cfg(test)]
#[path = "../../tests/support/owner_wake.rs"]
mod tests;
