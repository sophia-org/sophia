//! Caller-consumed journal heads; snapshot results retain their ordinary APIs.
use sophia_shell_protocol::{
    CatalogActivationOutcome, ShellContentRecord, ShellIndicatorActivationOutcome, TransactionId,
    shell_files::{
        ShellFileDescriptorRecord, ShellFileNativeLauncherRecord, ShellFileObjectPublished,
    },
};

use crate::{ShellClientError, ShellConnection};

/// A borrowed, validated event. Its sequence is not acknowledged until consumed.
pub struct JournalEvent<'a> {
    pub sequence: u64,
    pub record: JournalRecord<'a>,
}

pub enum JournalRecord<'a> {
    ObjectPublished(&'a ShellFileObjectPublished),
    Content(TransactionId, &'a ShellContentRecord),
    Frame(&'a sophia_shell_protocol::shell_files::ShellFrameRecord),
    Native(&'a ShellFileNativeLauncherRecord),
    Descriptor(&'a ShellFileDescriptorRecord),
    IndicatorOutcome(TransactionId, &'a ShellIndicatorActivationOutcome),
    CatalogOutcome(TransactionId, &'a CatalogActivationOutcome),
}

impl ShellConnection {
    /// Automatic delivery is enabled by default. Disable it to retain one
    /// journal head until `consume_journal_event`. This does not retract events
    /// already delivered or ACKs already queued. Startup/custody remain SDK-owned.
    /// A retained head must be consumed before automatic delivery can resume.
    pub fn set_automatic_event_delivery(&mut self, enabled: bool) -> Result<(), ShellClientError> {
        self.wire.set_automatic_event_delivery(enabled)
    }

    /// Defer re-arming the `events` read until the next queued request or
    /// the next `wait_for_io`/`dispatch` call, so a host that answers reads as
    /// soon as it appends is woken once for the re-arm and the following
    /// write together. Off by default. While enabled, a caller that sleeps on
    /// the borrowed descriptor without `wait_for_io` or `dispatch` may wait
    /// with no read outstanding; use those entry points before sleeping.
    pub fn set_deferred_event_reads(&mut self, enabled: bool) -> Result<(), ShellClientError> {
        self.wire.set_deferred_event_reads(enabled)
    }

    /// Borrow the same validated head across dispatches, without socket I/O.
    /// Object publications can be prefetched before consumption. Snapshot
    /// results still use the ordinary receive APIs. A head survives local close.
    pub fn journal_event(&self) -> Option<JournalEvent<'_>> {
        self.wire.journal_event()
    }

    /// Consume the retained head without socket I/O. No copy is added to the
    /// ordinary receive queues. False means there was no head. Dispatch applies
    /// subsequent events and sends eligible ACKs; snapshot holds still apply.
    pub fn consume_journal_event(&mut self) -> Result<bool, ShellClientError> {
        self.wire.consume_journal_event()
    }
}
