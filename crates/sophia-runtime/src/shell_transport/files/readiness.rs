//! One owner-wait observation; never authority and never reusable after a turn.
use std::os::fd::{AsFd, AsRawFd};

use rustix::event::PollFd;

use super::image::ListenerSubscription;

/// Registration for the wire and, when stable, its image listener.
pub struct ShellReadinessSubscription {
    pub(super) wire: sophia_9p::unix::Subscription,
    pub(super) listener: Option<ListenerSubscription>,
}

/// Readiness from one successful poll of the registered transport descriptors.
pub struct ShellReadinessObservation {
    pub(super) wire: sophia_9p::unix::Observation,
    pub(super) listener_idle: Option<ListenerSubscription>,
}

impl ShellReadinessSubscription {
    /// Call after polling the unmodified descriptor slice from `visit_poll_fds`.
    /// Missing or changed image registration falls back to an ordinary probe.
    pub fn observe(self, fds: &[PollFd<'_>]) -> Option<ShellReadinessObservation> {
        let wire = self.wire.observe(fds.get(..2)?)?;
        let listener_idle = self.listener.filter(|listener| {
            fds.get(2).is_some_and(|fd| {
                fd.as_fd().as_raw_fd() == listener.descriptor && fd.revents().is_empty()
            })
        });
        Some(ShellReadinessObservation {
            wire,
            listener_idle,
        })
    }
}
