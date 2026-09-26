#![cfg(test)]
//! A worker with no transport thread: a test reads every command an owner
//! submits and supplies the events it would have received.

use super::*;

pub(in crate::live_session) fn capturing_worker() -> (
    PolicyTransportWorker,
    Receiver<PolicyTransportCommand>,
    SyncSender<PolicyTransportEvent>,
) {
    let (commands, submitted) = sync_channel(POLICY_TRANSPORT_CAPACITY);
    let (events, received) = sync_channel(POLICY_TRANSPORT_CAPACITY);
    (
        PolicyTransportWorker {
            commands: Some(commands),
            events: received,
            thread: None,
            stop: None,
        },
        submitted,
        events,
    )
}
