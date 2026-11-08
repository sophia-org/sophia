//! Component descriptors in the owner's bounded Service wait.
//!
//! The Service wait holds the turn while frame service or topology recovery
//! owns it. Without component descriptors in that wait, the pass that
//! follows it has no observation for any shell wire and probes each one with
//! a zero-timeout poll. Subscribing them here, exactly as the authority
//! Receive wait does, gives that pass an observation instead. Every wire
//! still turns once per visit; nothing is skipped and no cap, order or
//! custody check changes.
use rustix::event::PollFd;

use super::owner_wake::OwnerWake;
use crate::shell_component_connections::{ComponentWaits, ShellComponentConnections};

/// Append the component descriptors ahead of the relay descriptors, so the
/// relays' offset is the length after this call. A runtime-less session has
/// no component service and subscribes nothing.
pub(super) fn subscribe<'a>(
    components: Option<&'a ShellComponentConnections>,
    runtime_present: bool,
    fds: &mut smallvec::SmallVec<[PollFd<'a>; 64]>,
) -> Option<ComponentWaits<'a>> {
    components
        .filter(|_| runtime_present)
        .map(|components| components.subscribe_wait(fds))
}

/// Record the wait's component causes and hand each wire its observation.
pub(super) fn observe(
    owner_wake: &OwnerWake,
    waits: Option<ComponentWaits<'_>>,
    fds: &[PollFd<'_>],
) {
    if let Some(waits) = waits {
        owner_wake.note_wait_causes(waits.observe(fds));
    }
}

mod tests;
