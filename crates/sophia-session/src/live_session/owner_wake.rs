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

use std::cell::Cell;
use std::io;
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SendError, SyncSender, TryRecvError, TrySendError,
};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags};

mod attribution;
pub(super) use attribution::{WaitPlan, WaitReason};

pub(super) struct OwnerWake {
    wake: sophia_wake::Wake,
    statistics: Cell<OwnerWakeStatistics>,
    native_fault: Cell<Option<u64>>,
    native_progress_before_wait: Cell<Option<(u64, u64)>>,
    wait_plan: Cell<Option<WaitPlan>>,
    wait_attribution: Cell<attribution::WaitAttribution>,
}

/// Owner-thread observations, not scheduler wakeups or per-producer attribution.
/// Ring and descriptor readiness can overlap on one poll return.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct OwnerWakeStatistics {
    pub passes: u64,
    pub waits: u64,
    pub ring_ready: u64,
    pub fd_ready: u64,
    pub wait_deadlines: u64,
    pub immediate_items: u64,
    pub native_ready: u64,
    pub native_ready_consumed: u64,
    pub native_ready_idle: u64,
    pub native_errors: u64,
    pub native_event_waits: u64,
    pub native_short_waits: u64,
    pub service_waits: u64,
}

impl OwnerWake {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            wake: sophia_wake::Wake::new()?,
            statistics: Cell::default(),
            native_fault: Cell::new(None),
            native_progress_before_wait: Cell::new(None),
            wait_plan: Cell::new(None),
            wait_attribution: Cell::default(),
        })
    }

    pub(super) fn notifier(&self) -> sophia_wake::Notifier {
        self.wake.notifier()
    }

    pub(super) fn statistics(&self) -> OwnerWakeStatistics {
        self.statistics.get()
    }

    pub(super) fn plan_wait(&self, plan: WaitPlan) {
        self.wait_plan.set(Some(plan));
    }

    pub(super) fn wait_attribution(&self) -> attribution::WaitAttribution {
        self.wait_attribution.get()
    }

    /// Consumes the rings delivered so far. Call before inspecting any
    /// producer; never between an inspection and the wait that follows it.
    pub(super) fn begin_pass(&self) -> io::Result<()> {
        self.wake.clear()?;
        let mut stats = self.statistics.get();
        stats.passes = stats.passes.saturating_add(1);
        self.statistics.set(stats);
        Ok(())
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
    #[cfg(test)]
    pub(super) fn receive_with_fds<'a, T>(
        &'a self,
        receiver: &Receiver<T>,
        timeout: Duration,
        fds: Vec<PollFd<'a>>,
    ) -> io::Result<Result<T, RecvTimeoutError>> {
        self.receive_with_native(receiver, timeout, fds, Vec::new(), None)
    }

    /// Native readiness is attributed separately; it grants no retirement.
    pub(super) fn receive_with_native<'a, T>(
        &'a self,
        receiver: &Receiver<T>,
        timeout: Duration,
        fds: Vec<PollFd<'a>>,
        native: Vec<PollFd<'a>>,
        progress: Option<(u64, u64)>,
    ) -> io::Result<Result<T, RecvTimeoutError>> {
        match receiver.try_recv() {
            Ok(item) => {
                self.wait_plan.set(None);
                let mut stats = self.statistics.get();
                stats.immediate_items = stats.immediate_items.saturating_add(1);
                self.statistics.set(stats);
                return Ok(Ok(item));
            }
            Err(TryRecvError::Disconnected) => {
                self.wait_plan.set(None);
                return Ok(Err(RecvTimeoutError::Disconnected));
            }
            Err(TryRecvError::Empty) => {}
        }
        self.wait(timeout, fds, native, progress)?;
        Ok(match receiver.try_recv() {
            Ok(item) => Ok(item),
            Err(TryRecvError::Empty) => Err(RecvTimeoutError::Timeout),
            Err(TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
        })
    }
    /// Fair service turns must not consume the authority queue. Rings and native
    /// fds still wake this wait, unlike the old blind 1 ms sleep.
    pub(super) fn wait_for_service<'a>(
        &'a self,
        timeout: Duration,
        native: Vec<PollFd<'a>>,
        progress: Option<(u64, u64)>,
    ) -> io::Result<()> {
        let mut stats = self.statistics.get();
        stats.service_waits = stats.service_waits.saturating_add(1);
        self.statistics.set(stats);
        self.plan_wait(WaitPlan::new(timeout, WaitReason::Service));
        self.wait(timeout, Vec::new(), native, progress)
    }

    pub(super) fn record_native_wait(&self, short: bool, events: bool) {
        let mut stats = self.statistics.get();
        stats.native_short_waits = stats.native_short_waits.saturating_add(u64::from(short));
        stats.native_event_waits = stats.native_event_waits.saturating_add(u64::from(events));
        self.statistics.set(stats);
    }

    /// Called after seat/lifecycle service and before another subscription.
    /// A revoked fd wakes once so seat handling can run. If it is still an
    /// active owner, fail rather than resubscribe a permanently failing fd.
    pub(super) fn observe_native_progress(
        &self,
        current: Option<(u64, u64)>,
        active: bool,
    ) -> io::Result<()> {
        if let Some((owner, before)) = self.native_progress_before_wait.take() {
            let consumed =
                current.is_some_and(|(now_owner, after)| now_owner == owner && after > before);
            let mut stats = self.statistics.get();
            stats.native_ready_consumed = stats
                .native_ready_consumed
                .saturating_add(u64::from(consumed));
            stats.native_ready_idle = stats.native_ready_idle.saturating_add(u64::from(!consumed));
            self.statistics.set(stats);
        }
        if self.native_fault.take().is_some_and(|owner| {
            active && current.is_some_and(|(current_owner, _)| current_owner == owner)
        }) {
            return Err(io::Error::other("native completion descriptor failed"));
        }
        Ok(())
    }

    fn wait<'a>(
        &'a self,
        timeout: Duration,
        mut fds: Vec<PollFd<'a>>,
        native: Vec<PollFd<'a>>,
        progress: Option<(u64, u64)>,
    ) -> io::Result<()> {
        let now = Instant::now();
        let plan = self
            .wait_plan
            .take()
            .unwrap_or_else(|| WaitPlan::new(timeout, WaitReason::Maintenance));
        debug_assert_eq!(plan.timeout, timeout);
        let native_start = fds.len();
        fds.extend(native);
        let ring_index = fds.len();
        fds.push(PollFd::new(&self.wake, PollFlags::IN));
        let ready = sophia_wake::wait(&mut fds, Some(now.checked_add(timeout).unwrap_or(now)))?;
        let mut stats = self.statistics.get();
        stats.waits = stats.waits.saturating_add(1);
        stats.ring_ready = stats
            .ring_ready
            .saturating_add(u64::from(!fds[ring_index].revents().is_empty()));
        stats.fd_ready = stats.fd_ready.saturating_add(u64::from(
            fds[..native_start]
                .iter()
                .any(|fd| !fd.revents().is_empty()),
        ));
        let native_ready = fds[native_start..ring_index]
            .iter()
            .any(|fd| !fd.revents().is_empty());
        let native_error = fds[native_start..ring_index].iter().any(|fd| {
            fd.revents()
                .intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
        });
        if native_ready {
            self.native_progress_before_wait.set(progress);
            stats.native_ready = stats.native_ready.saturating_add(1);
        }
        if native_error {
            self.native_fault.set(progress.map(|(owner, _)| owner));
            stats.native_errors = stats.native_errors.saturating_add(1);
        }
        stats.wait_deadlines = stats.wait_deadlines.saturating_add(u64::from(!ready));
        let mut attribution = self.wait_attribution.get();
        attribution.observe(plan, !ready);
        self.wait_attribution.set(attribution);
        self.statistics.set(stats);
        Ok(())
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
