use crate::{ShellClientError, ShellConnection};

/// Journal delivery progress, not application completion or presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalProgress {
    /// Highest event retained by the SDK; the application may not have read it.
    pub handled: u64,
    /// Highest safe ACK, bounded by every unverified snapshot announcement.
    pub ack_limit: u64,
    /// Highest ACK queued locally, which may not have reached the server.
    pub queued: u64,
    /// Highest ACK with a validated successful reply.
    pub acknowledged: u64,
    pub ack_pending: bool,
    pub object_pending: bool,
}

impl ShellConnection {
    pub fn journal_progress(&self) -> JournalProgress {
        self.wire.journal_progress()
    }

    /// Automatic ACKs are enabled by default. Disabling them does not retract
    /// an already queued ACK. This call performs no I/O. While disabled, call
    /// `acknowledge_journal` promptly after processing delivered values: delaying
    /// ACKs can stall submissions or trigger the server's progress deadline.
    /// Application input/action ACKs are separate protocol operations.
    pub fn set_automatic_journal_ack(&mut self, enabled: bool) -> Result<(), ShellClientError> {
        self.wire.set_automatic_journal_ack(enabled)
    }

    /// Queue an ACK through the current safe limit without socket I/O.
    /// False means no new ACK was queued (no progress or one already pending).
    /// Dispatch sends it; this never bypasses an unverified object's hold.
    pub fn acknowledge_journal(&mut self) -> Result<bool, ShellClientError> {
        self.wire.acknowledge_journal()
    }
}
