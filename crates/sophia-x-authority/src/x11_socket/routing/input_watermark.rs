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
