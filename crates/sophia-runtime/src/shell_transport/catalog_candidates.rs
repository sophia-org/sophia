//! Revision-8 assembly borrows the existing candidate and response owners.
//! The published catalog comes from Session; no peer-supplied catalog is trusted.
use super::{ShellComponentTransport, ShellTransportConnection, ShellTransportError};
use crate::{ContentCandidateContext, ContentEpochRegistry, ContentRenderBundle};
use sophia_protocol::*;

enum CandidateRecord {
    Begin(CatalogCandidateBegin),
    Chunk(ContentCandidateChunk),
    End(ContentCandidateEnd),
}
impl CandidateRecord {
    fn decode(frame: &[u8]) -> Result<(TransactionId, Self), ShellTransportError> {
        if u16::from_le_bytes([frame[6], frame[7]]) == 174 {
            let (tx, ShellContentRecord::CandidateEnd(end)) = decode_shell_content_frame(frame)?
            else {
                return Err(ShellTransportError::WrongContentRecord);
            };
            return Ok((tx, Self::End(end)));
        }
        let (tx, record) = decode_shell_catalog_action_frame(frame)?;
        Ok((
            tx,
            match record {
                ShellCatalogActionRecord::CandidateBegin(value) => Self::Begin(value),
                ShellCatalogActionRecord::CandidateChunk(value) => Self::Chunk(value),
                _ => return Err(ShellTransportError::WrongContentRecord),
            },
        ))
    }
    fn grant(&self) -> ContentGrant {
        match self {
            Self::Begin(value) => value.content.grant,
            Self::Chunk(value) => value.grant,
            Self::End(value) => value.grant,
        }
    }
}

impl ShellComponentTransport {
    pub(super) fn select_catalog_negotiation(
        &self,
        connection_epoch: u64,
        policy: super::ShellContentAdmissionPolicy,
        hello: ShellV1ClientHello,
    ) -> Result<(ShellV1ServerWelcome, Option<ContentLimits>), ShellTransportError> {
        const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
            | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        if hello.minimum_revision == 0
            || hello.minimum_revision > 8
            || hello.maximum_revision < 8
            || hello.minimum_revision > hello.maximum_revision
        {
            return Err(ShellTransportError::UnsupportedRevision);
        }
        if hello.required_capabilities != CAPS {
            return Err(ShellTransportError::MissingCapability);
        }
        let reason = match policy {
            super::ShellContentAdmissionPolicy::Unavailable => {
                Some(super::content_admission::UNAVAILABLE)
            }
            super::ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            } => None,
            _ => Some(super::content_admission::PERMISSION_DENIED),
        };
        if let Some(reason) = reason {
            return Err(ShellTransportError::ContentAdmissionRefused(
                ContentAdmissionRefused {
                    reason,
                    denied_capabilities: CAPS,
                },
            ));
        }
        let limits = self
            .reserved_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        if limits.grant.connection_epoch != connection_epoch {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok((
            ShellV1ServerWelcome {
                selected_revision: 8,
                connection_epoch,
                capabilities: CAPS,
                max_descriptors: SOPHIA_SHELL_MAX_DESCRIPTORS as u16,
                max_label_bytes: MAX_CHROME_LABEL_LEN as u16,
                max_pending_activations: SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS as u16,
            },
            Some(limits.clone()),
        ))
    }
    /// Service already buffered assembly records under one bounded visit. This
    /// does not admit a dock, allocate surfaces, grant permits or authorize input.
    pub fn service_catalog_candidates(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        contexts: &[ContentCandidateContext<'_>],
        catalog: &ShellApplicationCatalog,
        now: u64,
    ) -> Result<usize, ShellTransportError> {
        if !self.supports_persistent_catalog(epochs) {
            return Err(ShellTransportError::MissingCapability);
        }
        if catalog.connection_epoch != self.store_grant.connection_epoch || catalog.generation == 0
        {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let limits = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        let max_records = limits.max_frames_per_service_tick.min(32) as usize;
        let max_payload = limits.max_frame_payload as usize;
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now)?;
        self.flush_content_candidate_events(epochs)?;
        let mut processed = 0;
        let mut remaining = 64 * 1024;
        while processed < max_records {
            let Some(index) = self.inbox.iter().position(|frame| {
                matches!(
                    u16::from_le_bytes([frame[6], frame[7]]),
                    172..=174 | 189 | 190 | 198 | 199
                )
            }) else {
                break;
            };
            let frame = &self.inbox[index];
            let kind = u16::from_le_bytes([frame[6], frame[7]]);
            // A different candidate family cannot silently bypass the catalog
            // binding or sit forever as an unserviceable inbox record.
            if !matches!(kind, 174 | 198 | 199) {
                return Err(ShellTransportError::WrongContentRecord);
            }
            let bytes = frame.len() - SOPHIA_IPC_HEADER_LEN;
            if bytes > max_payload {
                return Err(ShellTransportError::WrongContentRecord);
            }
            if bytes > remaining || !self.control_capacity_available(epochs, 0) {
                break;
            }
            let (transaction, record) = CandidateRecord::decode(frame)?;
            if record.grant() != self.store_grant {
                return Err(ShellTransportError::WrongContentGrant);
            }
            let context = if let CandidateRecord::End(end) = &record {
                let output = epochs
                    .active_candidates(self.store_grant)
                    .and_then(|store| store.assembling_output(end.candidate_generation))
                    .ok_or(ShellTransportError::WrongCandidate)?;
                let mut matches = contexts.iter().filter(|context| context.output == output);
                let context = matches
                    .next()
                    .copied()
                    .ok_or(ShellTransportError::WrongCandidate)?;
                if matches.next().is_some() {
                    return Err(ShellTransportError::WrongCandidate);
                }
                Some(context)
            } else {
                None
            };
            // All decoding, identity and current-context checks precede dequeue.
            // The store already owns the permit's terminal response credit.
            self.inbox.remove(index);
            remaining -= bytes;
            let (resources, candidates) = epochs
                .active_parts_mut(self.store_grant)
                .ok_or(ShellTransportError::MissingCapability)?;
            let result = match record {
                CandidateRecord::Begin(value) => {
                    candidates.begin_persistent_catalog(transaction, value, catalog, now)
                }
                CandidateRecord::Chunk(value) => {
                    candidates.chunk_persistent_catalog(transaction, value, now)
                }
                CandidateRecord::End(value) => candidates.end_persistent_catalog(
                    transaction,
                    value,
                    context.expect("validated End context"),
                    catalog,
                    resources,
                    now,
                ),
            };
            let reported = candidates.pending_event().is_some();
            self.flush_content_candidate_events(epochs)?;
            if let Err(error) = result
                && !reported
            {
                return Err(error.into());
            }
            processed += 1;
        }
        if processed == 0 && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(processed)
    }
}

impl ShellTransportConnection<'_> {
    pub fn service_catalog_candidates(
        &mut self,
        contexts: &[ContentCandidateContext<'_>],
        catalog: &ShellApplicationCatalog,
        now: u64,
    ) -> Result<usize, ShellTransportError> {
        if !self.supports_persistent_catalog() {
            return Err(ShellTransportError::MissingCapability);
        }
        self.state.poll_io_bounded(self.content_epochs, 64 * 1024)?;
        self.state
            .service_catalog_candidates(self.content_epochs, contexts, catalog, now)
    }

    pub fn begin_catalog_submission(
        &mut self,
        generation: u64,
        context: ContentCandidateContext<'_>,
        catalog: &ShellApplicationCatalog,
        now: u64,
    ) -> Result<ContentRenderBundle, ShellTransportError> {
        if !self.supports_persistent_catalog() {
            return Err(ShellTransportError::MissingCapability);
        }
        self.content_epochs
            .active_candidates_mut(self.state.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .begin_persistent_catalog_submission(generation, context, catalog, now)
            .map_err(Into::into)
    }
}
