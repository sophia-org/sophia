//! Revision-8 adapters share the connection's sole typed inbox and outbox.
//! Catalog assembly happens inside the wire (see `socket`) and becomes
//! observable here only as one complete `Inbound::Catalog` value.
use crate::wire::{Inbound, Outbound};
use crate::*;
use sophia_shell_protocol::*;

pub enum CatalogObservation {
    Content(TransactionId, ShellContentRecord),
    Catalog(TransactionId, ShellPersistentCatalog),
    Outcome(TransactionId, CatalogActivationOutcome),
}

/// A connection-epoch token guarding one caller's view of the catalog: which
/// connection it was issued for, and the highest generation it has accepted.
/// Wire-level assembly (bounds, ordering) lives with the wire; this keeps
/// only the checks that outlive any one publication.
pub struct CatalogInbox {
    epoch: u64,
    generation: u64,
}
impl CatalogInbox {
    pub fn new(connection_epoch: u64) -> Result<Self, ShellClientError> {
        if connection_epoch == 0 {
            return Err(ShellClientError::WrongDirection);
        }
        Ok(Self {
            epoch: connection_epoch,
            generation: 0,
        })
    }
}

impl ShellConnection {
    fn require_catalog(&self) -> Result<(), ShellClientError> {
        if self.welcome.selected_revision != SOPHIA_SHELL_PERSISTENT_CATALOG_REVISION
            || self.welcome.capabilities & SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG == 0
        {
            return Err(ShellClientError::MissingCapability);
        }
        Ok(())
    }

    /// Take one already-assembled observation, in wire order, without socket
    /// I/O. Multi-frame assembly already happened inside the wire; content
    /// observations do not overtake earlier content records, and indicator
    /// workflows sharing the same inbox keep their own order untouched.
    /// Caller dispatches Content through its ordinary ContentLifecycle before
    /// processing an Action.
    pub fn take_catalog_observation(
        &mut self,
        assembly: &mut CatalogInbox,
    ) -> Result<Option<CatalogObservation>, ShellClientError> {
        self.require_catalog()?;
        if assembly.epoch != self.connection_epoch() {
            return Err(ShellClientError::WrongDirection);
        }
        let at = self.inbox.iter().position(|item| {
            matches!(
                item,
                Inbound::Content(_, _) | Inbound::Catalog(_, _) | Inbound::CatalogOutcome(_, _)
            )
        });
        let Some(item) = at.and_then(|index| self.inbox.remove(index)) else {
            return if self.wire.peer_closed() {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        match item {
            Inbound::Content(tx, record) => {
                if !server_record(&record) {
                    return Err(ShellClientError::WrongDirection);
                }
                Ok(Some(CatalogObservation::Content(tx, record)))
            }
            Inbound::CatalogOutcome(tx, outcome) => {
                if outcome.activation.action.grant.connection_epoch != assembly.epoch {
                    return Err(ShellClientError::WrongDirection);
                }
                Ok(Some(CatalogObservation::Outcome(tx, outcome)))
            }
            Inbound::Catalog(tx, catalog) => {
                if catalog.catalog.connection_epoch != assembly.epoch
                    || catalog.catalog.generation <= assembly.generation
                {
                    return Err(ShellClientError::WrongDirection);
                }
                assembly.generation = catalog.catalog.generation;
                Ok(Some(CatalogObservation::Catalog(tx, catalog)))
            }
            Inbound::Indicators(_, _) | Inbound::IndicatorOutcome(_, _) => {
                unreachable!("position() only matches Content, Catalog and CatalogOutcome variants")
            }
        }
    }

    /// Atomically queue the revision-8 Begin/chunks and common End while
    /// registering their exact ordinary presentation metadata. Catalog generation
    /// is immutable wire provenance; it does not make Prepared targets active.
    pub fn enqueue_catalog_candidate(
        &mut self,
        lifecycle: &mut ContentLifecycle,
        transaction: TransactionId,
        begin: &CatalogCandidateBegin,
        chunks: &[ContentCandidateChunk],
        end: &ContentCandidateEnd,
    ) -> Result<(), ShellClientError> {
        self.require_catalog()?;
        if chunks.len() > MAX_QUEUED_FRAMES / 2 - 2 {
            return Err(ShellClientError::QueueSaturated);
        }
        if begin.content.grant.connection_epoch != self.connection_epoch() {
            return Err(ShellClientError::WrongDirection);
        }
        let mut records = vec![ShellContentRecord::CandidateBegin(begin.content.clone())];
        records.extend(
            chunks
                .iter()
                .cloned()
                .map(ShellContentRecord::CandidateChunk),
        );
        records.push(ShellContentRecord::CandidateEnd(end.clone()));
        let metadata = candidate::metadata(transaction, &records)?;
        let units = self.wire.encode(Outbound::CatalogCandidateGroup {
            transaction,
            begin: begin.clone(),
            chunks: chunks.to_vec(),
            end: end.clone(),
        })?;
        self.output.enqueue_after(units, false, || {
            lifecycle
                .register(metadata)
                .map_err(ShellClientError::Lifecycle)
        })?;
        self.wire.commit_encoded();
        Ok(())
    }

    /// Reserve ACK and exact activation together before a UI effect. There is
    /// no I/O after admission. Cancellation must use neither ACK nor activation.
    pub fn enqueue_catalog_action_response(
        &mut self,
        transaction: TransactionId,
        ack: &ContentActionAck,
        activation: Option<(TransactionId, &CatalogActivation)>,
    ) -> Result<(), ShellClientError> {
        self.require_catalog()?;
        if ack.grant.connection_epoch != self.connection_epoch() {
            return Err(ShellClientError::WrongDirection);
        }
        let units = self.wire.encode(Outbound::CatalogActionResponse {
            transaction,
            ack: ack.clone(),
            activation: activation.map(|(tx, activation)| (tx, activation.clone())),
        })?;
        self.output.enqueue(units, true)?;
        self.wire.commit_encoded();
        Ok(())
    }
}
