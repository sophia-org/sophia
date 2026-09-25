/// How far a client's input writer has caught up with what was routed to it.
///
/// The registry counts every event it queues for the client; the writer
/// counts every event it has finished with, written, dropped or refused.
/// A connection that must not read its next request until an injection of
/// its own has reached its socket takes the queued count as a mark once the
/// routing is done and waits for the drained count to reach it. Without
/// this the FakeInput barrier ended at routing, and the reply to the next
/// request could be written before the event the injection owed (t229:
/// XTS Xlib11 KeymapNotify 1 read `No events received` on a loaded
/// machine and passed on a quiet one).
#[cfg(unix)]
#[derive(Debug, Default)]
pub(crate) struct X11InputWatermark {
    queued: std::sync::atomic::AtomicU64,
    drained: std::sync::atomic::AtomicU64,
    wake: std::sync::Mutex<Option<crate::NotifierSubscription>>,
    /// For a waiter with no notifier of its own: the request loop after a
    /// pointer replay.
    changed: std::sync::Condvar,
    waiting: std::sync::Mutex<()>,
}

#[cfg(unix)]
impl X11InputWatermark {
    /// One more event routed to the client's writer.
    pub(crate) fn queued(&self) {
        self.queued
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    /// The writer has finished with one more event, whatever became of it.
    pub(crate) fn drained(&self) {
        self.drained
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if let Ok(wake) = self.wake.lock()
            && let Some(wake) = wake.as_ref()
        {
            let _ = wake.notify();
        }
        self.changed.notify_all();
    }

    /// Wait until the writer has drained to `mark`, or the bound passes.
    /// A drain between the check and the sleep is caught by the bound, which
    /// is far beyond the microseconds a write takes.
    pub(crate) fn wait_drained(&self, mark: u64, bound: std::time::Duration) {
        let deadline = std::time::Instant::now() + bound;
        let Ok(mut guard) = self.waiting.lock() else {
            return;
        };
        while !self.reached(mark) {
            let now = std::time::Instant::now();
            if now >= deadline {
                return;
            }
            let Ok((next, _)) = self.changed.wait_timeout(guard, deadline - now) else {
                return;
            };
            guard = next;
        }
    }

    /// Everything routed so far, to wait for with [`Self::reached`].
    pub(crate) fn mark(&self) -> u64 {
        self.queued.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn reached(&self, mark: u64) -> bool {
        self.drained.load(std::sync::atomic::Ordering::Acquire) >= mark
    }

    /// Wake this notifier whenever the writer finishes with an event.
    pub(crate) fn wake_on_drain(&self, notifier: &crate::ConnectionNotifier) {
        if let Ok(mut wake) = self.wake.lock() {
            *wake = Some(notifier.subscription());
        }
    }
}

/// Counts one event drained when dropped, so every way out of the writer's
/// iteration counts it: written, skipped, or failed.
#[cfg(unix)]
pub(crate) struct X11InputDrainGuard<'a>(pub(crate) &'a X11InputWatermark);

#[cfg(unix)]
impl Drop for X11InputDrainGuard<'_> {
    fn drop(&mut self) {
        self.0.drained();
    }
}

/// How far a client's protocol event writer has caught up with what was
/// routed to it, the reply side of the same ordering: a request read after
/// an event was queued for the client has its reply written after that
/// event. The connection takes the queued count as a mark when it reads a
/// request and waits, bounded, for the drained count to reach it before
/// writing the request's outputs. Without this a MapRequest routed to a
/// redirecting client could be written after the reply to that client's
/// next request, and the client, having synced, found nothing pending
/// (t229; XTS Xlib4 XMapWindow 6 under load).
#[cfg(unix)]
#[derive(Debug, Default)]
pub(crate) struct X11ProtocolWatermark {
    counts: std::sync::Mutex<(u64, u64)>,
    changed: std::sync::Condvar,
}

#[cfg(unix)]
impl X11ProtocolWatermark {
    /// One more event queued for the client's protocol writer.
    pub(crate) fn queued(&self) {
        if let Ok(mut counts) = self.counts.lock() {
            counts.0 += 1;
        }
    }

    /// The writer has finished with one more event, written or not.
    pub(crate) fn drained(&self) {
        if let Ok(mut counts) = self.counts.lock() {
            counts.1 += 1;
            self.changed.notify_all();
        }
    }

    /// Everything queued so far.
    pub(crate) fn mark(&self) -> u64 {
        self.counts.lock().map_or(0, |counts| counts.0)
    }

    /// Wait until the writer has drained to `mark`, or the bound passes.
    pub(crate) fn wait_drained(&self, mark: u64, bound: std::time::Duration) {
        let deadline = std::time::Instant::now() + bound;
        let Ok(mut counts) = self.counts.lock() else {
            return;
        };
        while counts.1 < mark {
            let now = std::time::Instant::now();
            if now >= deadline {
                return;
            }
            let Ok((guard, _)) = self.changed.wait_timeout(counts, deadline - now) else {
                return;
            };
            counts = guard;
        }
    }
}

/// Counts one event drained when dropped, so every way out of the protocol
/// writer's iteration counts it.
#[cfg(unix)]
pub(crate) struct X11ProtocolDrainGuard<'a>(&'a X11ProtocolReceiver);

#[cfg(unix)]
impl Drop for X11ProtocolDrainGuard<'_> {
    fn drop(&mut self) {
        self.0.drained();
    }
}
