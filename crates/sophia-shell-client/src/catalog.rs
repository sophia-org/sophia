//! Revision-8 adapters share the connection's sole input/output FIFO. Catalog
//! assembly is bounded and becomes observable only after its exact End.
use crate::*;
use sophia_protocol::*;

pub enum CatalogObservation {
    Content(TransactionId, ShellContentRecord),
    Catalog(TransactionId, ShellPersistentCatalog),
    Outcome(TransactionId, CatalogActivationOutcome),
}

/// One connection-scoped partial publication, never an active target owner.
pub struct CatalogInbox {
    epoch: u64,
    generation: u64,
    frames: Vec<Vec<u8>>,
    bytes: usize,
}
impl CatalogInbox {
    pub fn new(connection_epoch: u64) -> Result<Self, ShellClientError> {
        if connection_epoch == 0 {
            return Err(ShellClientError::WrongDirection);
        }
        Ok(Self {
            epoch: connection_epoch,
            generation: 0,
            frames: Vec::new(),
            bytes: 0,
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

    /// Visit at most 64 buffered records, in wire order, without socket I/O.
    /// A partial publication is retained across visits; content observations do
    /// not overtake earlier content records. Caller dispatches Content through
    /// its ordinary ContentLifecycle before processing an Action.
    pub fn take_catalog_observation(
        &mut self,
        assembly: &mut CatalogInbox,
    ) -> Result<Option<CatalogObservation>, ShellClientError> {
        self.require_catalog()?;
        if assembly.epoch != self.connection_epoch() {
            return Err(ShellClientError::WrongDirection);
        }
        for _ in 0..64 {
            let Some(frame) = self.inbox.front() else {
                return if self.peer_closed {
                    Err(ShellClientError::PeerClosed)
                } else {
                    Ok(None)
                };
            };
            let (header, _) = decode_frame(frame)?;
            let kind = header.message_kind;
            if content_kind(kind) {
                let (tx, record) = decode_shell_content_frame(frame)?;
                if !server_record(&record) {
                    return Err(ShellClientError::WrongDirection);
                }
                self.inbox.pop_front();
                return Ok(Some(CatalogObservation::Content(tx, record)));
            }
            if kind == IpcMessageKind::ShellCatalogActivationOutcome {
                let (tx, ShellCatalogActionRecord::ActivationOutcome(outcome)) =
                    decode_shell_catalog_action_frame(frame)?
                else {
                    return Err(ShellClientError::WrongDirection);
                };
                if outcome.activation.action.grant.connection_epoch != assembly.epoch {
                    return Err(ShellClientError::WrongDirection);
                }
                self.inbox.pop_front();
                return Ok(Some(CatalogObservation::Outcome(tx, outcome)));
            }
            let beginning = kind == IpcMessageKind::ShellApplicationsBegin;
            if !matches!(
                kind,
                IpcMessageKind::ShellApplicationsBegin
                    | IpcMessageKind::ShellApplicationsEntry
                    | IpcMessageKind::ShellCatalogIdentity
                    | IpcMessageKind::ShellApplicationsEnd
            ) || beginning != assembly.frames.is_empty()
                || assembly.frames.first().is_some_and(|first| {
                    decode_frame(first)
                        .is_ok_and(|(first, _)| first.transaction != header.transaction)
                })
            {
                return Err(ShellClientError::WrongDirection);
            }
            if assembly.frames.len() >= 2 * SOPHIA_SHELL_MAX_APPLICATIONS + 2
                || assembly.bytes.saturating_add(frame.len()) > 4 * 1024 * 1024
            {
                return Err(ShellClientError::QueueSaturated);
            }
            assembly
                .frames
                .try_reserve(1)
                .map_err(|_| ShellClientError::QueueSaturated)?;
            assembly.bytes += frame.len();
            assembly
                .frames
                .push(self.inbox.pop_front().expect("inspected front"));
            if kind == IpcMessageKind::ShellApplicationsEnd {
                let (tx, catalog) = decode_shell_persistent_catalog(&assembly.frames)?;
                if catalog.catalog.connection_epoch != assembly.epoch
                    || catalog.catalog.generation <= assembly.generation
                {
                    return Err(ShellClientError::WrongDirection);
                }
                assembly.generation = catalog.catalog.generation;
                assembly.frames.clear();
                assembly.bytes = 0;
                return Ok(Some(CatalogObservation::Catalog(tx, catalog)));
            }
        }
        Ok(None)
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
        let mut frames = vec![encode_shell_catalog_action_frame(
            transaction,
            &ShellCatalogActionRecord::CandidateBegin(begin.clone()),
        )?];
        for chunk in chunks {
            frames.push(encode_shell_catalog_action_frame(
                transaction,
                &ShellCatalogActionRecord::CandidateChunk(chunk.clone()),
            )?);
        }
        frames.push(encode_shell_content_frame(
            transaction,
            &ShellContentRecord::CandidateEnd(end.clone()),
        )?);
        self.output.enqueue_after(frames, false, || {
            lifecycle
                .register(metadata)
                .map_err(ShellClientError::Lifecycle)
        })
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
        let mut frames = vec![encode_shell_content_frame(
            transaction,
            &ShellContentRecord::ActionAck(ack.clone()),
        )?];
        if let Some((tx, activation)) = activation {
            let action = &activation.action;
            let expected = ContentActionAck {
                grant: action.grant,
                output: action.output,
                candidate_generation: action.candidate_generation,
                presentation_epoch: action.presentation_epoch,
                interaction_generation: action.interaction_generation,
                allocation: action.allocation,
                target_id: action.target_id,
                target_generation: action.target_generation,
                action_id: action.action_id,
                event_id: action.event_id,
                disposition: 1,
            };
            if ack != &expected || ack.grant.connection_epoch != self.connection_epoch() {
                return Err(ShellClientError::WrongDirection);
            }
            frames.push(encode_shell_catalog_action_frame(
                tx,
                &ShellCatalogActionRecord::Activate(activation.clone()),
            )?);
        }
        self.output.enqueue(frames, true)
    }
}
