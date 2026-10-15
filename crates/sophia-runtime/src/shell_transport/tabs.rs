//! Tabs use one immutable snapshot and exact Prepared/terminal credits. Scene
//! projection and the actual presentation boundary remain Session's decisions.
use super::descriptor_state::PendingShellCandidate;
use super::outbound::OutboundRecord;
use super::*;
use sophia_protocol::shell_files::*;
use sophia_protocol::{
    ShellTabCandidate, ShellTabSnapshot, ShellV1CandidateOutcome, ShellV1CandidateOutcomeKind,
};

impl ShellComponentTransport {
    pub fn queue_tab_activation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        activation: sophia_protocol::ShellV1Activation,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(activation.connection_epoch)?;
        if !self.file_descriptor() {
            return self.send_socket_descriptor(
                epochs,
                transaction,
                ShellDescriptorRecord::DescriptorActivation(activation),
            );
        }
        if self.tab_state.presented_candidate
            != Some((
                activation.candidate_generation,
                activation.presentation_epoch,
            ))
            || activation.action.recipient_epoch != self.connection_epoch
        {
            return Err(ShellTransportError::WrongActivation);
        }
        if self.tab_state.pending_activations.len() >= SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        self.file_descriptor_activation(epochs, transaction, activation)?;
        self.tab_state
            .pending_activations
            .push_back((transaction, activation.activation));
        Ok(())
    }

    pub fn poll_tab_activation_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
    ) -> Result<Option<sophia_protocol::ShellV1ActivationAck>, ShellTransportError> {
        if !self.file_descriptor() {
            return self.poll_socket_descriptor_ack(epochs, transaction);
        }
        let index = self
            .tab_state
            .pending_activations
            .iter()
            .position(|(tx, _)| *tx == transaction);
        let expected = index.map(|i| self.tab_state.pending_activations[i]);
        let ack = self.take_file_activation_ack(epochs, expected)?;
        if ack.is_some() {
            self.tab_state
                .pending_activations
                .remove(index.expect("matched pending activation"));
        }
        Ok(ack)
    }

    /// Supersedes an unanswered snapshot atomically with publication. A
    /// candidate already handed to Session needs its terminal outcome first.
    pub fn publish_tabs(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellTabSnapshot,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(snapshot.connection_epoch)?;
        if !self.supports_tabs() {
            return Err(ShellTransportError::MissingCapability);
        }
        if !self.file_descriptor() {
            return self.send_socket_descriptor(
                epochs,
                transaction,
                ShellDescriptorRecord::Tabs(snapshot.clone()),
            );
        }
        if self.tab_state.pending_candidate.is_some() {
            return Err(ShellTransportError::WrongCandidate);
        }
        let additional = if self.tab_state.requested_candidate.is_some() {
            0
        } else {
            2
        };
        if !self.descriptor_capacity(epochs, additional) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        let (kind, body) = encode_shell_file_descriptor_body(&ShellFileDescriptorRecord {
            transaction,
            record: ShellDescriptorRecord::Tabs(snapshot.clone()),
        })
        .map_err(|_| ShellTransportError::WrongCandidate)?;
        self.publish_object(kind, &body)?;
        self.tab_state.requested_candidate = Some((transaction, snapshot.clone()));
        // Tab snapshots follow scene/label identity changes. Unlike a pending
        // switcher replacement, publishing one revokes the old tab interaction.
        self.tab_state.presented_candidate = None;
        self.tab_state.response_credits += additional;
        Ok(())
    }

    /// File custody is consumed only with enough response capacity. Stale
    /// candidates receive Superseded and cannot replace the current request.
    pub fn poll_tabs_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellTabCandidate)>, ShellTransportError> {
        if !self.file_descriptor() {
            return self.poll_socket_tabs_candidate(epochs);
        }
        self.poll_io(epochs)?;
        let Some(value) = self
            .files_mut()
            .expect("file role")
            .export()
            .peek_descriptor(ShellFileKind::TabsCandidate)
            .cloned()
        else {
            return self.nothing_inbound();
        };
        let ShellDescriptorRecord::TabsCandidate(candidate) = value.record else {
            unreachable!("selected tabs")
        };
        let matching = self
            .tab_state
            .requested_candidate
            .as_ref()
            .is_some_and(|(tx, _)| *tx == value.transaction);
        let valid = self
            .tab_state
            .requested_candidate
            .as_ref()
            .is_some_and(|(tx, snapshot)| {
                *tx == value.transaction
                    && snapshot.generation == candidate.snapshot_generation
                    && snapshot
                        .groups
                        .iter()
                        .map(|g| g.slot)
                        .eq(candidate.groups.iter().copied())
                    && candidate.candidate_generation > self.tab_state.last_candidate_generation
                    && candidate.candidate_generation < (1 << 63)
            });
        if !valid {
            if !matching {
                if !self.descriptor_capacity(epochs, 1) {
                    return Ok(None);
                }
                self.tab_state.response_credits += 1;
            }
            self.file_tabs_outcome(
                value.transaction,
                ShellV1CandidateOutcome {
                    connection_epoch: self.connection_epoch,
                    candidate_generation: candidate.candidate_generation,
                    presentation_epoch: 0,
                    kind: ShellV1CandidateOutcomeKind::Superseded,
                },
            )?;
            if matching {
                self.tab_state.requested_candidate = None;
                self.tab_state.response_credits -= 1;
            }
            self.files_mut()
                .expect("file role")
                .export_mut()
                .take_descriptor(ShellFileKind::TabsCandidate);
            return Ok(None);
        }
        self.files_mut()
            .expect("file role")
            .export_mut()
            .take_descriptor(ShellFileKind::TabsCandidate);
        self.tab_state.requested_candidate = None;
        self.tab_state.last_candidate_generation = candidate.candidate_generation;
        self.tab_state.pending_candidate = Some(PendingShellCandidate {
            transaction: value.transaction,
            generation: candidate.candidate_generation,
            visible: !candidate.groups.is_empty(),
            prepared: false,
        });
        Ok(Some((value.transaction, candidate)))
    }

    pub fn send_tabs_outcome(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        outcome: ShellV1CandidateOutcome,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(outcome.connection_epoch)?;
        if !self.file_descriptor() {
            return self.send_socket_descriptor(
                epochs,
                transaction,
                ShellDescriptorRecord::DescriptorOutcome(outcome),
            );
        }
        let mut pending = self
            .tab_state
            .pending_candidate
            .ok_or(ShellTransportError::WrongCandidate)?;
        if pending.transaction != transaction || pending.generation != outcome.candidate_generation
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        match outcome.kind {
            ShellV1CandidateOutcomeKind::Prepared if !pending.prepared => {}
            ShellV1CandidateOutcomeKind::Presented if pending.prepared => {}
            ShellV1CandidateOutcomeKind::Rejected | ShellV1CandidateOutcomeKind::Superseded => {}
            _ => return Err(ShellTransportError::WrongCandidate),
        }
        self.file_tabs_outcome(transaction, outcome)?;
        if outcome.kind == ShellV1CandidateOutcomeKind::Prepared {
            pending.prepared = true;
            self.tab_state.pending_candidate = Some(pending);
        } else {
            if !pending.prepared {
                self.tab_state.response_credits -= 1;
            }
            self.tab_state.pending_candidate = None;
            if outcome.kind == ShellV1CandidateOutcomeKind::Presented {
                self.tab_state.presented_candidate = Some((
                    pending.generation,
                    if pending.visible {
                        outcome.presentation_epoch
                    } else {
                        0
                    },
                ));
            }
        }
        Ok(())
    }

    fn file_tabs_outcome(
        &mut self,
        transaction: TransactionId,
        outcome: ShellV1CandidateOutcome,
    ) -> Result<(), ShellTransportError> {
        if self.tab_state.response_credits == 0 {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        let admitted = self.admit_record(
            OutboundRecord::Descriptor(ShellFileDescriptorRecord {
                transaction,
                record: ShellDescriptorRecord::DescriptorOutcome(outcome),
            }),
            control_budget::Class::Control {
                limit: self.control_record_bytes(),
                oversize: ShellTransportError::WrongCandidate,
            },
        )?;
        self.transfer_record(admitted);
        self.tab_state.response_credits -= 1;
        Ok(())
    }
}

macro_rules! facade {
    ($owner:ty) => {
        impl $owner {
            pub fn queue_tab_activation(
                &mut self,
                transaction: TransactionId,
                activation: sophia_protocol::ShellV1Activation,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .queue_tab_activation(&mut self.content_epochs, transaction, activation)
            }
            pub fn poll_tab_activation_ack(
                &mut self,
                transaction: TransactionId,
            ) -> Result<Option<sophia_protocol::ShellV1ActivationAck>, ShellTransportError> {
                self.state
                    .poll_tab_activation_ack(&mut self.content_epochs, transaction)
            }
            pub fn publish_tabs(
                &mut self,
                transaction: TransactionId,
                snapshot: &ShellTabSnapshot,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .publish_tabs(&mut self.content_epochs, transaction, snapshot)
            }
            pub fn poll_tabs_candidate(
                &mut self,
            ) -> Result<Option<(TransactionId, ShellTabCandidate)>, ShellTransportError> {
                self.state.poll_tabs_candidate(&mut self.content_epochs)
            }
            pub fn send_tabs_outcome(
                &mut self,
                transaction: TransactionId,
                outcome: ShellV1CandidateOutcome,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .send_tabs_outcome(&mut self.content_epochs, transaction, outcome)
            }
        }
    };
}
facade!(ShellSessionTransport);
facade!(ShellTransportConnection<'_>);
