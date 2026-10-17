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
    /// Latest unverified announcement per feed. Manual mode can retain several;
    /// choose any one with `request_snapshot`. Only one fetch runs at a time.
    pub fn pending_snapshots(
        &self,
    ) -> impl Iterator<Item = sophia_shell_protocol::shell_files::ShellFileObjectPublished> + '_
    {
        self.wire.pending_snapshots()
    }

    /// Fetch an explicitly supplied identity, even before its announcement.
    /// The feed must be disclosed by this connection. The SDK still verifies
    /// Qid, generation and epoch; only an exact later announcement discharges
    /// its ACK hold. A different active fetch refuses without changing it.
    pub fn prefetch_snapshot(
        &mut self,
        request: sophia_shell_protocol::shell_files::ShellFileObjectPublished,
    ) -> Result<bool, ShellClientError> {
        self.wire.prefetch_snapshot(request)
    }
    /// Automatic snapshot fetching is enabled by default. Disabling it leaves
    /// an already started fetch running, including its bounded retries. New
    /// announcements wait for `request_snapshot`. Later events may be retained,
    /// but ACKs cannot pass an unverified announcement. These controls never
    /// change publication authority.
    pub fn set_automatic_snapshot_fetch(&mut self, enabled: bool) -> Result<(), ShellClientError> {
        self.wire.set_automatic_snapshot_fetch(enabled)
    }

    /// The first pending feed awaiting verification, if any; not journal order.
    /// Recheck before requesting it; superseded announcements may be skipped.
    pub fn pending_snapshot(
        &self,
    ) -> Option<sophia_shell_protocol::shell_files::ShellFileObjectPublished> {
        self.wire.pending_snapshot()
    }

    /// Permit fetching the exact pending announcement without I/O. False means
    /// its fetch was already requested or started. Wrong/stale identities are
    /// rejected without mutation. Dispatch fetches, validates and delivers the
    /// typed snapshot through the ordinary receive API before releasing holds.
    pub fn request_snapshot(
        &mut self,
        request: sophia_shell_protocol::shell_files::ShellFileObjectPublished,
    ) -> Result<bool, ShellClientError> {
        self.wire.request_snapshot(request)
    }

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
