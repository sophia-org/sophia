//! Shortcut publication and reference-sheet response custody. Session still
//! owns projection, output validity and the actual presentation boundary.
use super::outbound::OutboundRecord;
use super::*;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;

#[derive(Default)]
pub(super) struct ReferenceState {
    catalog: Option<u64>,
    request: Option<(TransactionId, ShellReferenceRequest)>,
    pending: Option<(TransactionId, ShellReferenceCandidate, bool)>,
    cancelled: bool,
    last_candidate: u64,
    pub(super) response_credits: usize,
}

/// A refused current request ends Session's wait just as a candidate does.
/// Refusals for other transactions must not cancel its current request.
pub enum ShellReferenceCandidateEvent {
    Candidate(TransactionId, ShellReferenceCandidate),
    Refused(TransactionId),
}

impl ShellComponentTransport {
    pub fn publish_shortcuts(
        &mut self,
        _epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        catalog: &ShellShortcutCatalog,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(catalog.connection_epoch)?;
        if !self.supports_shortcut_catalog() {
            return Err(ShellTransportError::MissingCapability);
        }
        if !self.file_descriptor() {
            return Err(ShellTransportError::NotConnected);
        }
        if self.reference_state.request.is_some() || self.reference_state.pending.is_some() {
            return Err(ShellTransportError::WrongCandidate);
        }
        let (kind, body) = encode_shell_file_descriptor_body(&ShellFileDescriptorRecord {
            transaction,
            record: ShellDescriptorRecord::Shortcuts(catalog.clone()),
        })
        .map_err(|_| ShellTransportError::WrongCandidate)?;
        self.publish_object(kind, &body)?;
        self.reference_state.catalog = Some(catalog.generation);
        Ok(())
    }

    pub fn begin_reference_request(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        request: ShellReferenceRequest,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(request.connection_epoch)?;
        if !self.supports_reference() {
            return Err(ShellTransportError::MissingCapability);
        }
        if !self.file_descriptor() {
            return Err(ShellTransportError::NotConnected);
        }
        if self.reference_state.catalog != Some(request.catalog_generation)
            || self.reference_state.request.is_some()
            || self.reference_state.pending.is_some()
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        // The request and both responses fit control records. Reserving all
        // three before transfer makes this admission indivisible.
        let admitted = self.admit_record(
            OutboundRecord::Descriptor(ShellFileDescriptorRecord {
                transaction,
                record: ShellDescriptorRecord::ReferenceRequest(request),
            }),
            control_budget::Class::Control {
                limit: self.control_record_bytes(),
                oversize: ShellTransportError::WrongCandidate,
            },
        )?;
        if !self.descriptor_capacity(epochs, 3) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        self.transfer_record(admitted);
        self.reference_state.request = Some((transaction, request));
        self.reference_state.cancelled = false;
        self.reference_state.response_credits = 2;
        Ok(())
    }

    /// Cancellation preserves the response obligation until the peer answers.
    pub fn cancel_reference_request(&mut self) {
        if self.reference_state.request.is_some() {
            self.reference_state.cancelled = true;
        }
    }

    pub fn poll_reference_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellReferenceCandidateEvent>, ShellTransportError> {
        if !self.file_descriptor() {
            return Err(ShellTransportError::NotConnected);
        }
        self.poll_io(epochs)?;
        let Some(value) = self
            .files_mut()
            .expect("file role")
            .export()
            .peek_descriptor(ShellFileKind::ReferenceCandidate)
            .cloned()
        else {
            return self.nothing_inbound();
        };
        let ShellDescriptorRecord::ReferenceCandidate(candidate) = value.record else {
            unreachable!("selected reference candidate");
        };
        let request = self.reference_state.request;
        let matching = request.is_some_and(|(tx, _)| tx == value.transaction);
        let cancelled = matching && self.reference_state.cancelled;
        let valid = request.is_some_and(|(tx, request)| {
            tx == value.transaction
                && candidate.catalog_generation == request.catalog_generation
                && candidate.request_generation == request.request_generation
                && candidate.output == request.output
                && candidate.candidate_generation > self.reference_state.last_candidate
        });
        if cancelled || !valid {
            if !matching {
                if !self.descriptor_capacity(epochs, 1) {
                    return Ok(None);
                }
                self.reference_state.response_credits += 1;
            }
            self.file_reference_outcome(
                value.transaction,
                ShellReferenceOutcome {
                    connection_epoch: self.connection_epoch,
                    catalog_generation: candidate.catalog_generation,
                    request_generation: candidate.request_generation,
                    candidate_generation: candidate.candidate_generation,
                    presentation_epoch: 0,
                    page: 0,
                    pages: 1,
                    kind: if cancelled {
                        ShellV1CandidateOutcomeKind::Superseded
                    } else {
                        ShellV1CandidateOutcomeKind::Rejected
                    },
                },
            )?;
            if matching {
                self.reference_state.request = None;
                self.reference_state.cancelled = false;
                self.reference_state.response_credits -= 1;
            }
            self.files_mut()
                .expect("file role")
                .export_mut()
                .take_descriptor(ShellFileKind::ReferenceCandidate);
            return Ok(Some(ShellReferenceCandidateEvent::Refused(
                value.transaction,
            )));
        }
        self.files_mut()
            .expect("file role")
            .export_mut()
            .take_descriptor(ShellFileKind::ReferenceCandidate);
        self.reference_state.request = None;
        self.reference_state.last_candidate = candidate.candidate_generation;
        self.reference_state.pending = Some((value.transaction, candidate.clone(), false));
        Ok(Some(ShellReferenceCandidateEvent::Candidate(
            value.transaction,
            candidate,
        )))
    }

    pub fn send_reference_outcome(
        &mut self,
        _epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        outcome: ShellReferenceOutcome,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(outcome.connection_epoch)?;
        if !self.file_descriptor() {
            return Err(ShellTransportError::NotConnected);
        }
        let (tx, candidate, prepared) = self
            .reference_state
            .pending
            .as_ref()
            .ok_or(ShellTransportError::WrongCandidate)?;
        if *tx != transaction
            || candidate.catalog_generation != outcome.catalog_generation
            || candidate.request_generation != outcome.request_generation
            || candidate.candidate_generation != outcome.candidate_generation
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        let prepared = *prepared;
        match outcome.kind {
            ShellV1CandidateOutcomeKind::Prepared if !prepared => {}
            ShellV1CandidateOutcomeKind::Presented if prepared => {}
            ShellV1CandidateOutcomeKind::Rejected | ShellV1CandidateOutcomeKind::Superseded => {}
            _ => return Err(ShellTransportError::WrongCandidate),
        }
        self.file_reference_outcome(transaction, outcome)?;
        if outcome.kind == ShellV1CandidateOutcomeKind::Prepared {
            self.reference_state
                .pending
                .as_mut()
                .expect("pending candidate")
                .2 = true;
        } else {
            self.reference_state.pending = None;
            if !prepared {
                self.reference_state.response_credits -= 1;
            }
        }
        Ok(())
    }

    fn file_reference_outcome(
        &mut self,
        transaction: TransactionId,
        outcome: ShellReferenceOutcome,
    ) -> Result<(), ShellTransportError> {
        if self.reference_state.response_credits == 0 {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        let admitted = self.admit_record(
            OutboundRecord::Descriptor(ShellFileDescriptorRecord {
                transaction,
                record: ShellDescriptorRecord::ReferenceOutcome(outcome),
            }),
            control_budget::Class::Control {
                limit: self.control_record_bytes(),
                oversize: ShellTransportError::WrongCandidate,
            },
        )?;
        self.transfer_record(admitted);
        self.reference_state.response_credits -= 1;
        Ok(())
    }
}

macro_rules! facade {
    ($owner:ty) => {
        impl $owner {
            pub fn publish_shortcuts(
                &mut self,
                transaction: TransactionId,
                catalog: &ShellShortcutCatalog,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .publish_shortcuts(&mut self.content_epochs, transaction, catalog)
            }
            pub fn begin_reference_request(
                &mut self,
                transaction: TransactionId,
                request: ShellReferenceRequest,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .begin_reference_request(&mut self.content_epochs, transaction, request)
            }
            pub fn cancel_reference_request(&mut self) {
                self.state.cancel_reference_request();
            }
            pub fn poll_reference_candidate(
                &mut self,
            ) -> Result<Option<ShellReferenceCandidateEvent>, ShellTransportError> {
                self.state
                    .poll_reference_candidate(&mut self.content_epochs)
            }
            pub fn send_reference_outcome(
                &mut self,
                transaction: TransactionId,
                outcome: ShellReferenceOutcome,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .send_reference_outcome(&mut self.content_epochs, transaction, outcome)
            }
        }
    };
}
facade!(ShellSessionTransport);
facade!(ShellTransportConnection<'_>);
