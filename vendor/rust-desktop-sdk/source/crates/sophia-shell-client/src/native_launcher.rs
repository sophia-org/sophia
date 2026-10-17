//! Native launcher transport, without focus policy or UI effects.
use crate::wire::{Inbound, Outbound};
use crate::{Admission, ContentReservation, ShellClientError, ShellConnection};
use sophia_shell_protocol::shell_files::{ShellFileNativeCandidate, ShellFileNativeLauncherRecord};
use sophia_shell_protocol::{
    NativeLauncherActivation, NativeLauncherInputAck, ShellApplicationCatalog, ShellContentRecord,
    TransactionId,
};

/// Consume this stream exclusively to preserve order across native events,
/// content outcomes and fetched catalogs. Other take methods remove entries
/// from the same inbox. Snapshot feeds may coalesce superseded generations.
#[derive(Debug)]
pub enum NativeLauncherObservation {
    /// Disarm scenes based on older facts/catalogs immediately, even while
    /// fetching the announced snapshot. The complete object follows later.
    Published(sophia_shell_protocol::shell_files::ShellFileObjectPublished),
    Record(ShellFileNativeLauncherRecord),
    Content(TransactionId, ShellContentRecord),
    Catalog(TransactionId, ShellApplicationCatalog),
}

impl ShellConnection {
    /// Queue one native allocation request, input ACK or activation. No I/O;
    /// the returned ticket proves local admission only. Fragmented candidate
    /// records and server events are rejected. Use a whole native candidate.
    pub fn enqueue_native_launcher_tracked(
        &mut self,
        value: &ShellFileNativeLauncherRecord,
    ) -> Result<Admission, ShellClientError> {
        let outbound = Outbound::Native(value.clone());
        let control = outbound.is_control();
        self.admit(outbound, control)
    }

    /// Queue a whole native candidate. This does not register lifecycle state
    /// or establish presentation, focus, or activation eligibility.
    pub fn enqueue_native_candidate_tracked(
        &mut self,
        value: &ShellFileNativeCandidate,
    ) -> Result<Admission, ShellClientError> {
        self.admit(Outbound::NativeCandidate(value.clone()), false)
    }

    /// Prepare an input ACK and optional keyboard activation before a local
    /// edit. Both name the exact same event, with distinct transactions. The
    /// activation precedes the ACK, matching C. Local admission is atomic;
    /// each record has independent server custody and semantic results.
    /// Commit performs no I/O or allocation; Drop cancels. The caller still
    /// owns focus/revision eligibility and the input acknowledgement deadline.
    pub fn reserve_native_input_response(
        &mut self,
        transaction: TransactionId,
        ack: NativeLauncherInputAck,
        activation: Option<(TransactionId, NativeLauncherActivation)>,
    ) -> Result<ContentReservation<'_>, ShellClientError> {
        self.wire.io_interest()?;
        let units = self.wire.encode(Outbound::NativeResponse {
            transaction,
            ack,
            activation,
        })?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare(units, true, &mut self.ledger)?;
        Ok(ContentReservation {
            connection: self,
            prepared: Some(prepared),
        })
    }

    /// Admit the response without a local edit between preparation and commit.
    pub fn enqueue_native_input_response(
        &mut self,
        transaction: TransactionId,
        ack: NativeLauncherInputAck,
        activation: Option<(TransactionId, NativeLauncherActivation)>,
    ) -> Result<Admission, ShellClientError> {
        Ok(self
            .reserve_native_input_response(transaction, ack, activation)?
            .commit())
    }

    /// Drain native and shared observations in wire order without driving I/O.
    /// The transport acknowledges journal delivery, not application-level input.
    pub fn take_native_launcher_observation(
        &mut self,
    ) -> Result<Option<NativeLauncherObservation>, ShellClientError> {
        let at = self.inbox.iter().position(|v| {
            matches!(
                v,
                Inbound::NativePublication(_)
                    | Inbound::Native(_)
                    | Inbound::Content(..)
                    | Inbound::ApplicationCatalog(..)
            )
        });
        let Some(item) = at.and_then(|at| self.inbox.remove(at)) else {
            return if self.wire.peer_closed() {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        Ok(Some(match item {
            Inbound::NativePublication(value) => NativeLauncherObservation::Published(value),
            Inbound::Native(value) => NativeLauncherObservation::Record(value),
            Inbound::Content(tx, value) => NativeLauncherObservation::Content(tx, value),
            Inbound::ApplicationCatalog(tx, value) => NativeLauncherObservation::Catalog(tx, value),
            _ => unreachable!("only native observations selected"),
        }))
    }
}
