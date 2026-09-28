//! File transport adapters for the shared descriptor exchange owner. Accepted
//! candidates reserve outcomes before changing presentation state.
use super::descriptor_state::PendingShellCandidate;
use super::outbound::OutboundRecord;
use super::*;
use sophia_protocol::shell_files::*;
use sophia_protocol::{
    ShellV1Candidate, ShellV1CandidateOutcome, ShellV1CandidateOutcomeKind,
    ShellV1DescriptorSnapshot,
};

impl ShellComponentTransport {
    /// Diagnostic count for this epoch; consuming a stale acknowledgement
    /// cannot launch an action or remove another activation's obligation.
    pub fn descriptor_unmatched_acks(&self) -> u64 {
        self.descriptor_state.unmatched_acks
    }

    pub(super) fn file_descriptor_activation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        activation: sophia_protocol::ShellV1Activation,
    ) -> Result<(), ShellTransportError> {
        let admitted = self.admit_record(
            OutboundRecord::Descriptor(ShellFileDescriptorRecord {
                transaction,
                record: ShellDescriptorRecord::DescriptorActivation(activation),
            }),
            control_budget::Class::Bulk,
        )?;
        if !self.bulk_capacity_available(epochs, admitted.charge) {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        self.transfer_record(admitted);
        Ok(())
    }

    pub(super) fn poll_file_descriptor_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<sophia_protocol::ShellV1ActivationAck>, ShellTransportError> {
        self.poll_io(epochs)?;
        let Some(value) = self
            .files_mut()
            .expect("file role")
            .export_mut()
            .take_descriptor(ShellFileKind::DescriptorActivationAck)
        else {
            return self.nothing_inbound();
        };
        let ShellDescriptorRecord::DescriptorActivationAck(ack) = value.record else {
            unreachable!("selected acknowledgement");
        };
        if self.descriptor_state.pending_activations.front().copied()
            != Some((value.transaction, ack.activation))
        {
            self.descriptor_state.unmatched_acks =
                self.descriptor_state.unmatched_acks.saturating_add(1);
            return Ok(None);
        }
        self.descriptor_state.pending_activations.pop_front();
        Ok(Some(ack))
    }

    pub(super) fn file_descriptor(&self) -> bool {
        matches!(&self.wire, Some(wire::Wire::Files(files)) if files.export().is_descriptor())
    }

    fn descriptor_capacity(&self, epochs: &crate::ContentEpochRegistry, additional: usize) -> bool {
        if self.content_limits.is_some() {
            self.control_capacity_available(epochs, additional)
        } else {
            let credits = self.descriptor_state.response_credits + additional;
            let other = usize::from(self.indicator_response.is_some());
            self.fifo_records() + credits + other <= 64
                && self.fifo_bytes() + (credits + other) * self.control_record_bytes()
                    <= 2 * 1024 * 1024
        }
    }

    pub(super) fn begin_file_descriptor_request(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellV1DescriptorSnapshot,
    ) -> Result<(), ShellTransportError> {
        if !self.descriptor_capacity(epochs, 2) {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        let (kind, body) = encode_shell_file_descriptor_body(&ShellFileDescriptorRecord {
            transaction,
            record: ShellDescriptorRecord::Descriptors(snapshot.clone()),
        })
        .map_err(|_| ShellTransportError::WrongCandidate)?;
        self.publish_object(kind, &body)?;
        self.descriptor_state.response_credits += 2;
        self.descriptor_state.requested_candidate = Some((transaction, snapshot.clone()));
        Ok(())
    }

    /// Moves one pre-reserved response into the existing typed FIFO. No I/O
    /// follows custody transfer; the owner can now retire the exact credit.
    pub(super) fn file_descriptor_outcome(
        &mut self,
        transaction: TransactionId,
        outcome: ShellV1CandidateOutcome,
    ) -> Result<(), ShellTransportError> {
        if self.descriptor_state.response_credits == 0 {
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
        self.descriptor_state.response_credits -= 1;
        Ok(())
    }

    pub(super) fn poll_file_descriptor_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1Candidate>, ShellTransportError> {
        self.poll_io(epochs)?;
        let Some(value) = self
            .files_mut()
            .and_then(|files| {
                files
                    .export()
                    .peek_descriptor(ShellFileKind::DescriptorCandidate)
            })
            .cloned()
        else {
            return self.nothing_inbound();
        };
        let ShellDescriptorRecord::DescriptorCandidate(candidate) = value.record else {
            unreachable!("selected candidate");
        };
        let request = self.descriptor_state.requested_candidate.as_ref();
        let matching_request = request.is_some_and(|(tx, _)| *tx == value.transaction);
        let valid = request.is_some_and(|(tx, snapshot)| {
            *tx == value.transaction
                && snapshot.snapshot_generation == candidate.snapshot_generation
                && snapshot.output == candidate.output
                && candidate.candidate_generation > self.descriptor_state.last_candidate_generation
                && candidate.entries.iter().all(|entry| {
                    snapshot
                        .descriptors
                        .iter()
                        .any(|d| d.slot == entry.slot && d.generation == entry.generation)
                })
        });
        if !valid {
            // An unsolicited/stale transaction does not steal the credits or
            // cancel the request belonging to another transaction.
            if !matching_request {
                if !self.descriptor_capacity(epochs, 1) {
                    return Ok(None);
                }
                self.descriptor_state.response_credits += 1;
            }
            self.file_descriptor_outcome(
                value.transaction,
                ShellV1CandidateOutcome {
                    connection_epoch: self.connection_epoch,
                    candidate_generation: candidate.candidate_generation,
                    presentation_epoch: 0,
                    kind: ShellV1CandidateOutcomeKind::Rejected,
                },
            )?;
            if matching_request {
                self.descriptor_state.requested_candidate = None;
                // Rejected skips Prepared, releasing its unused second credit.
                self.descriptor_state.response_credits -= 1;
            }
            self.files_mut()
                .expect("file role")
                .export_mut()
                .take_descriptor(ShellFileKind::DescriptorCandidate);
            return Ok(None);
        }
        self.files_mut()
            .expect("file role")
            .export_mut()
            .take_descriptor(ShellFileKind::DescriptorCandidate);
        self.descriptor_state.requested_candidate = None;
        self.descriptor_state.last_candidate_generation = candidate.candidate_generation;
        self.descriptor_state.pending_candidate = Some(PendingShellCandidate {
            transaction: value.transaction,
            generation: candidate.candidate_generation,
            visible: candidate.visible,
            prepared: false,
        });
        Ok(Some(candidate))
    }
}
