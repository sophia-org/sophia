//! Descriptor launcher custody. A peer acknowledgement never authorizes an
//! exec: Session still revalidates the catalog and owns process admission.
use super::outbound::OutboundRecord;
use super::*;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;

#[derive(Default)]
pub(super) struct LauncherState {
    catalog: Option<(u64, std::collections::BTreeSet<u16>)>,
    request: Option<(TransactionId, ShellLauncherRequest)>,
    pending: Option<(TransactionId, ShellLauncherCandidate, bool)>,
    presented: Option<(ShellLauncherCandidate, u64)>,
    activation: Option<(TransactionId, ShellLauncherActivation, Option<bool>)>,
    revoked: bool,
    last_candidate: u64,
    pub(super) response_credits: usize,
    unmatched_acks: u64,
}

/// Refused replies end only the corresponding Session wait.
pub enum ShellLauncherCandidateEvent {
    Candidate(TransactionId, ShellLauncherCandidate),
    Refused(TransactionId),
}

impl ShellComponentTransport {
    pub fn publish_launcher_catalog(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        catalog: &ShellApplicationCatalog,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(catalog.connection_epoch)?;
        if !self.supports_launcher() {
            return Err(ShellTransportError::MissingCapability);
        }
        if self.file_descriptor()
            && (self.launcher_state.request.is_some()
                || self.launcher_state.pending.is_some()
                || self.launcher_state.activation.is_some())
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        self.publish_catalog(
            epochs,
            transaction,
            &ShellPersistentCatalog {
                catalog: catalog.clone(),
                identities: Default::default(),
            },
        )?;
        if self.file_descriptor() {
            self.launcher_state.catalog = Some((
                catalog.generation,
                catalog.entries.iter().map(|e| e.slot).collect(),
            ));
            self.launcher_state.presented = None;
        }
        Ok(())
    }

    pub fn begin_launcher_request(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        request: &ShellLauncherRequest,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(request.connection_epoch)?;
        if !self.supports_launcher() {
            return Err(ShellTransportError::MissingCapability);
        }
        if self.launcher_catalog_pending() {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        if !self.file_descriptor() {
            return self.send_async(epochs, encode_shell_launcher_request(transaction, request)?);
        }
        if self
            .launcher_state
            .catalog
            .as_ref()
            .map(|(generation, _)| *generation)
            != Some(request.catalog_generation)
            || self.launcher_state.request.is_some()
            || self.launcher_state.pending.is_some()
            || self.launcher_state.activation.is_some()
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        let admitted = self.admit_launcher_control(
            transaction,
            ShellDescriptorRecord::LauncherRequest(request.clone()),
        )?;
        if !self.descriptor_capacity(epochs, 3) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        self.transfer_record(admitted);
        self.launcher_state.request = Some((transaction, request.clone()));
        self.launcher_state.revoked = false;
        self.launcher_state.response_credits = 2;
        Ok(())
    }

    /// Revokes interaction immediately but retains accepted response and
    /// activation obligations until their terminal outcome or disconnect.
    pub fn revoke_launcher(&mut self) {
        self.launcher_state.revoked = true;
        self.launcher_state.presented = None;
    }

    /// A socket catalog may span several bounded I/O visits. File snapshots
    /// publish atomically, so their announcement is already ahead of a request.
    pub fn launcher_catalog_pending(&self) -> bool {
        self.socket()
            .is_some_and(|socket| socket.publication_pending())
    }

    pub fn poll_launcher_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellLauncherCandidateEvent>, ShellTransportError> {
        if !self.file_descriptor() {
            return self
                .poll_kind(epochs, IpcMessageKind::ShellLauncherCandidate)?
                .map(|frame| {
                    decode_shell_launcher_candidate(&frame)
                        .map(|(tx, candidate)| {
                            ShellLauncherCandidateEvent::Candidate(tx, candidate)
                        })
                        .map_err(Into::into)
                })
                .transpose();
        }
        self.poll_io(epochs)?;
        let Some(value) = self
            .files_mut()
            .expect("file role")
            .export()
            .peek_descriptor(ShellFileKind::LauncherCandidate)
            .cloned()
        else {
            return self.nothing_inbound();
        };
        let ShellDescriptorRecord::LauncherCandidate(candidate) = value.record else {
            unreachable!("selected launcher candidate");
        };
        let request = self.launcher_state.request.as_ref();
        let matching = request.is_some_and(|(tx, _)| *tx == value.transaction);
        let valid = !self.launcher_state.revoked
            && request.is_some_and(|(tx, request)| {
                *tx == value.transaction
                    && candidate.catalog_generation == request.catalog_generation
                    && candidate.request_generation == request.request_generation
                    && candidate.output == request.output
                    && candidate.candidate_generation > self.launcher_state.last_candidate
                    && candidate.visible == (request.operation != ShellLauncherOperation::Dismiss)
                    && self
                        .launcher_state
                        .catalog
                        .as_ref()
                        .is_some_and(|(_, slots)| {
                            candidate.entries.iter().all(|slot| slots.contains(slot))
                        })
            });
        if !valid {
            if !matching {
                if !self.descriptor_capacity(epochs, 1) {
                    return Ok(None);
                }
                self.launcher_state.response_credits += 1;
            }
            self.file_launcher_response(
                value.transaction,
                ShellDescriptorRecord::LauncherOutcome(ShellLauncherOutcome {
                    connection_epoch: self.connection_epoch,
                    request_generation: candidate.request_generation,
                    candidate_generation: candidate.candidate_generation,
                    presentation_epoch: 0,
                    kind: ShellV1CandidateOutcomeKind::Superseded,
                }),
            )?;
            if matching {
                self.launcher_state.request = None;
                self.launcher_state.response_credits -= 1;
            }
            self.files_mut()
                .expect("file role")
                .export_mut()
                .take_descriptor(ShellFileKind::LauncherCandidate);
            return Ok(Some(ShellLauncherCandidateEvent::Refused(
                value.transaction,
            )));
        }
        self.files_mut()
            .expect("file role")
            .export_mut()
            .take_descriptor(ShellFileKind::LauncherCandidate);
        self.launcher_state.request = None;
        self.launcher_state.last_candidate = candidate.candidate_generation;
        self.launcher_state.pending = Some((value.transaction, candidate.clone(), false));
        Ok(Some(ShellLauncherCandidateEvent::Candidate(
            value.transaction,
            candidate,
        )))
    }

    pub fn send_launcher_outcome(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        outcome: ShellLauncherOutcome,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(outcome.connection_epoch)?;
        if !self.file_descriptor() {
            return self.send_async(epochs, encode_shell_launcher_outcome(transaction, outcome)?);
        }
        let (tx, candidate, prepared) = self
            .launcher_state
            .pending
            .as_ref()
            .ok_or(ShellTransportError::WrongCandidate)?;
        if *tx != transaction
            || candidate.request_generation != outcome.request_generation
            || candidate.candidate_generation != outcome.candidate_generation
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        let prepared = *prepared;
        match outcome.kind {
            ShellV1CandidateOutcomeKind::Prepared if !prepared && !self.launcher_state.revoked => {}
            ShellV1CandidateOutcomeKind::Presented if prepared && !self.launcher_state.revoked => {}
            ShellV1CandidateOutcomeKind::Rejected | ShellV1CandidateOutcomeKind::Superseded => {}
            _ => return Err(ShellTransportError::WrongCandidate),
        }
        self.file_launcher_response(transaction, ShellDescriptorRecord::LauncherOutcome(outcome))?;
        if outcome.kind == ShellV1CandidateOutcomeKind::Prepared {
            self.launcher_state
                .pending
                .as_mut()
                .expect("pending candidate")
                .2 = true;
        } else {
            let (_, candidate, _) = self
                .launcher_state
                .pending
                .take()
                .expect("pending candidate");
            if !prepared {
                self.launcher_state.response_credits -= 1;
            }
            if outcome.kind == ShellV1CandidateOutcomeKind::Presented {
                self.launcher_state.presented = candidate
                    .visible
                    .then_some((candidate, outcome.presentation_epoch));
            }
        }
        Ok(())
    }

    pub fn queue_launcher_activation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        activation: ShellLauncherActivation,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(activation.connection_epoch)?;
        if !self.file_descriptor() {
            return self.send_async(
                epochs,
                encode_shell_launcher_activation(transaction, activation)?,
            );
        }
        let valid = self
            .launcher_state
            .presented
            .as_ref()
            .is_some_and(|(candidate, epoch)| {
                candidate.catalog_generation == activation.catalog_generation
                    && candidate.request_generation == activation.request_generation
                    && candidate.candidate_generation == activation.candidate_generation
                    && *epoch == activation.presentation_epoch
                    && candidate.entries.contains(&activation.slot)
            });
        if !valid
            || self.launcher_state.revoked
            || self.launcher_state.activation.is_some()
            || self.launcher_state.request.is_some()
            || self.launcher_state.pending.is_some()
        {
            return Err(ShellTransportError::WrongActivation);
        }
        let admitted = self.admit_launcher_control(
            transaction,
            ShellDescriptorRecord::LauncherActivation(activation),
        )?;
        // Retain LaunchOutcome capacity through acknowledgement, verification
        // and Session admission. An ack alone cannot release this obligation.
        if !self.descriptor_capacity(epochs, 2) {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        self.transfer_record(admitted);
        self.launcher_state.activation = Some((transaction, activation, None));
        self.launcher_state.response_credits += 1;
        Ok(())
    }

    pub fn poll_launcher_activation_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellLauncherActivationAck)>, ShellTransportError> {
        if !self.file_descriptor() {
            return self
                .poll_kind(epochs, IpcMessageKind::ShellLauncherActivationAck)?
                .map(|frame| decode_shell_launcher_activation_ack(&frame).map_err(Into::into))
                .transpose();
        }
        self.poll_io(epochs)?;
        while let Some(value) = self
            .files_mut()
            .expect("file role")
            .export_mut()
            .take_descriptor(ShellFileKind::LauncherActivationAck)
        {
            let ShellDescriptorRecord::LauncherActivationAck(ack) = value.record else {
                unreachable!("selected launcher ack");
            };
            if let Some((tx, activation, consumed)) = self.launcher_state.activation.as_mut()
                && *tx == value.transaction
                && *activation == ack.activation
                && consumed.is_none()
            {
                *consumed = Some(ack.consumed);
                return Ok(Some((value.transaction, ack)));
            }
            self.launcher_state.unmatched_acks =
                self.launcher_state.unmatched_acks.saturating_add(1);
        }
        self.nothing_inbound()
    }

    pub fn launcher_unmatched_acks(&self) -> u64 {
        self.launcher_state.unmatched_acks
    }

    pub fn send_launch_outcome(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        outcome: ShellLaunchOutcome,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(outcome.activation.connection_epoch)?;
        if !self.file_descriptor() {
            return self.send_async(epochs, encode_shell_launch_outcome(transaction, outcome)?);
        }
        let valid = self
            .launcher_state
            .activation
            .is_some_and(|(tx, activation, consumed)| {
                tx == transaction
                    && activation == outcome.activation
                    && consumed.is_some()
                    && (outcome.status == ShellLaunchStatus::Rejected
                        || (consumed == Some(true) && !self.launcher_state.revoked))
            });
        if !valid {
            return Err(ShellTransportError::WrongActivation);
        }
        self.file_launcher_response(transaction, ShellDescriptorRecord::LaunchOutcome(outcome))?;
        self.launcher_state.activation = None;
        Ok(())
    }

    fn admit_launcher_control(
        &self,
        transaction: TransactionId,
        record: ShellDescriptorRecord,
    ) -> Result<super::outbound::Admitted, ShellTransportError> {
        self.admit_record(
            OutboundRecord::Descriptor(ShellFileDescriptorRecord {
                transaction,
                record,
            }),
            control_budget::Class::Control {
                limit: self.control_record_bytes(),
                oversize: ShellTransportError::WrongCandidate,
            },
        )
    }

    fn file_launcher_response(
        &mut self,
        transaction: TransactionId,
        record: ShellDescriptorRecord,
    ) -> Result<(), ShellTransportError> {
        if self.launcher_state.response_credits == 0 {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        let admitted = self.admit_launcher_control(transaction, record)?;
        self.transfer_record(admitted);
        self.launcher_state.response_credits -= 1;
        Ok(())
    }
}

macro_rules! facade {
    ($owner:ty) => {
        impl $owner {
            pub fn publish_launcher_catalog(
                &mut self,
                tx: TransactionId,
                catalog: &ShellApplicationCatalog,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .publish_launcher_catalog(&self.content_epochs, tx, catalog)
            }
            pub fn begin_launcher_request(
                &mut self,
                tx: TransactionId,
                request: &ShellLauncherRequest,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .begin_launcher_request(&mut self.content_epochs, tx, request)
            }
            pub fn revoke_launcher(&mut self) {
                self.state.revoke_launcher();
            }
            pub fn launcher_catalog_pending(&self) -> bool {
                self.state.launcher_catalog_pending()
            }
            pub fn poll_launcher_candidate(
                &mut self,
            ) -> Result<Option<ShellLauncherCandidateEvent>, ShellTransportError> {
                self.state.poll_launcher_candidate(&mut self.content_epochs)
            }
            pub fn send_launcher_outcome(
                &mut self,
                tx: TransactionId,
                outcome: ShellLauncherOutcome,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .send_launcher_outcome(&mut self.content_epochs, tx, outcome)
            }
            pub fn queue_launcher_activation(
                &mut self,
                tx: TransactionId,
                activation: ShellLauncherActivation,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .queue_launcher_activation(&mut self.content_epochs, tx, activation)
            }
            pub fn poll_launcher_activation_ack(
                &mut self,
            ) -> Result<Option<(TransactionId, ShellLauncherActivationAck)>, ShellTransportError>
            {
                self.state
                    .poll_launcher_activation_ack(&mut self.content_epochs)
            }
            pub fn send_launch_outcome(
                &mut self,
                tx: TransactionId,
                outcome: ShellLaunchOutcome,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .send_launch_outcome(&mut self.content_epochs, tx, outcome)
            }
        }
    };
}
facade!(ShellSessionTransport);
facade!(ShellTransportConnection<'_>);
