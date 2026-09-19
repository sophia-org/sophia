#![cfg(all(test, unix))]

//! Test-only lifetime faults, raised on the serving thread.
//!
//! THE BODIES LIVE HERE, OUTSIDE PRODUCTION SOURCE. Only the carrier that
//! `start` takes is in the crate proper, and it is an empty struct in release
//! builds, so there is no fault field, no fault API and nothing to configure
//! outside a test binary.
//!
//! WHY A SUBSCRIBER RATHER THAN A PANIC AFTER THE CALL. An unwind raised after
//! `serve_until_stopped` returns is not the case worth testing: by then the
//! service's own collection guard has already run, so the unwind passes
//! through nothing. What has to be proved is that an invocation which unwinds
//! WHILE the guard is live still leaves its keeper, its custody and its
//! obligations in order. The only way into that window without inventing a
//! hook is to act on something the serving thread genuinely does there, and it
//! emits a trace event when it disconnects a client.
//!
//! So the fault is armed after the service is really serving, a second probe
//! peer is reaped, and the panic is raised from inside that event on the
//! serving thread. It is not a simulation of an unwind: it is an unwind, in
//! the place a real one would happen.
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The exact event this fault acts on.
///
/// MATCHED ON ITS MESSAGE AND ITS SOURCE FILE, never on a line number: a line
/// moves whenever anything above it does, and a fault that silently stopped
/// matching would leave its case passing by testing the ordinary path twice.
const DISCONNECT_EVENT: &str = "Sophia X Server Frontend disconnected one client";

/// A one-shot unwind, armed from outside and raised on the serving thread.
#[derive(Default)]
pub(crate) struct PrivateInputUnwindFault {
    armed: AtomicBool,
    fired: AtomicBool,
    /// The thread that is actually serving, recorded by that thread.
    serving: Mutex<Option<std::thread::ThreadId>>,
}

impl PrivateInputUnwindFault {
    /// Called by the serving thread, before it serves.
    pub(crate) fn record_serving_thread(&self) {
        if let Ok(mut serving) = self.serving.lock() {
            *serving = Some(std::thread::current().id());
        }
    }

    /// Arm the fault. Only meaningful once the service is really serving.
    pub(crate) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    /// Whether the fault was actually raised.
    ///
    /// A CONTROL THAT CANNOT TELL WHETHER ITS FAULT HAPPENED IS NOT A
    /// CONTROL. An unwind case that silently never fired would pass by
    /// testing the ordinary path twice.
    pub(crate) fn fired(&self) -> bool {
        self.fired.load(Ordering::Acquire)
    }

    /// Decide whether this event is the one, on the thread it must be on.
    fn should_fire(&self, message_matched: bool) -> bool {
        if !message_matched || !self.armed.load(Ordering::Acquire) {
            return false;
        }
        let Ok(serving) = self.serving.lock() else {
            return false;
        };
        if *serving != Some(std::thread::current().id()) {
            return false;
        }
        // DISARMED BEFORE THE PANIC, so unwinding cleanup that emits the
        // same event cannot panic a second time while the first is still
        // travelling.
        self.armed.swap(false, Ordering::AcqRel)
    }
}

/// Reads events on the serving thread and raises the armed fault.
pub(crate) struct PrivateInputUnwindSubscriber {
    fault: std::sync::Arc<PrivateInputUnwindFault>,
}

impl PrivateInputUnwindSubscriber {
    pub(crate) fn over(fault: std::sync::Arc<PrivateInputUnwindFault>) -> Self {
        Self { fault }
    }
}

/// Reads one event's `message` field and says whether it is the one.
struct MessageMatch {
    matched: bool,
}

impl tracing::field::Visit for MessageMatch {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn core::fmt::Debug) {
        if field.name() == "message" && format!("{value:?}") == DISCONNECT_EVENT {
            self.matched = true;
        }
    }
}

impl tracing::Subscriber for PrivateInputUnwindSubscriber {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        // THE SOURCE OF THE EVENT, NOT ONLY ITS WORDS. A message is a string
        // anything could emit; requiring the file it was written in makes this
        // the frontend's own disconnect rather than any event that happens to
        // read the same. The line is deliberately not checked: it moves
        // whenever anything above it does, and would silently stop matching.
        let from_frontend = event
            .metadata()
            .file()
            .is_some_and(|path| path.ends_with("x11_socket/frontend/service.rs"));
        let mut seen = MessageMatch { matched: false };
        event.record(&mut seen);
        if self.fault.should_fire(from_frontend && seen.matched) {
            self.fault.fired.store(true, Ordering::Release);
            panic!("private input lifetime fault: unwind on the serving thread");
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}
