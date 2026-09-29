//! Atomic join of output domain admission and file-event custody.
//!
//! A candidate's trial reducer is bounded by the advertised history limit.
//! It replaces the live reducer only after every required journal record and
//! terminal credit is reserved. Backpressure therefore spends no domain ID.
use sophia_9p::Errno;
use sophia_9p::journal::JournalBounds;
use sophia_protocol::output_files::*;
use sophia_protocol::{
    OutputAuthoritySnapshot, OutputV1Outcome, OutputV1OutcomeKind, OutputV1ServerWelcome,
    SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT, TransactionId,
};

use crate::{
    AdmittedOutputProposal, OutputConnectionState, OutputFileJournal, OutputFileReplacement,
    OutputProposalAdmission, OutputTransferError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputFileSubmission {
    /// An exact repeat has no new owner delivery or journal side effect.
    Replayed,
    Negotiated(OutputV1ServerWelcome),
    Refused(OutputFileRefusal),
    Proposal {
        proposal: AdmittedOutputProposal,
        admission: OutputProposalAdmission,
    },
    Rejected(TransactionId),
}

struct Accepted {
    submission: u64,
    bytes: Vec<u8>,
}

pub struct OutputFileAdmission {
    connection: OutputConnectionState,
    journal: OutputFileJournal,
    accepted: Option<Accepted>,
    refused: bool,
}

impl OutputFileAdmission {
    pub fn new(epoch: u64, limits: OutputFileLimits) -> Result<Self, Errno> {
        limits.validate().map_err(|_| Errno::EINVAL)?;
        let mut connection =
            OutputConnectionState::with_transaction_limit(limits.max_domain_transactions as usize);
        connection.connect(epoch).map_err(|_| Errno::EINVAL)?;
        Ok(Self {
            connection,
            journal: OutputFileJournal::new(
                epoch,
                JournalBounds {
                    records: limits.journal_records as usize,
                    bytes: limits.journal_bytes as usize,
                },
            )?,
            accepted: None,
            refused: false,
        })
    }

    pub fn connection(&self) -> &OutputConnectionState {
        &self.connection
    }

    pub fn journal(&self) -> &OutputFileJournal {
        &self.journal
    }

    pub(crate) fn last_submission_id(&self) -> Option<u64> {
        self.accepted.as_ref().map(|accepted| accepted.submission)
    }

    pub fn acknowledge(&mut self, ack: OutputFileAck) -> Result<bool, Errno> {
        self.journal.acknowledge(ack)
    }

    pub fn publish(&mut self, publication: OutputFilePublication) -> Result<u64, Errno> {
        self.connection
            .require_observe()
            .map_err(|_| Errno::EACCES)?;
        self.journal.publish_topology(publication)
    }

    /// The caller has staged exactly this complete candidate. A successful
    /// return acquires file custody; the proposal still awaits its owner.
    /// The supplied publication identifies the exact current snapshot bytes.
    pub fn submit(
        &mut self,
        bytes: &[u8],
        snapshot: &OutputAuthoritySnapshot,
        publication: OutputFilePublication,
    ) -> Result<OutputFileSubmission, Errno> {
        let record = decode_output_file_record(bytes, OutputFileClass::Candidate)
            .map_err(|_| Errno::EINVAL)?;
        if record.header.connection_epoch != self.connection.connection_epoch() {
            return Err(Errno::ESTALE);
        }
        let submission = record.header.submission_id;
        if let Some(accepted) = &self.accepted {
            if submission == accepted.submission {
                return if bytes == accepted.bytes {
                    Ok(OutputFileSubmission::Replayed)
                } else {
                    Err(Errno::EINVAL)
                };
            }
            if submission < accepted.submission {
                return Err(Errno::ESTALE);
            }
        }
        if self.refused {
            return Err(Errno::EACCES);
        }
        let mut trial = self.connection.clone();
        let result = match record.header.kind {
            OutputFileKind::Negotiate => {
                if publication.topology_epoch != snapshot.topology_epoch {
                    return Err(Errno::EINVAL);
                }
                let hello = decode_output_file_negotiate(record.body).map_err(|_| Errno::EINVAL)?;
                match trial.negotiate(hello) {
                    Ok(welcome) => {
                        self.journal.negotiated(submission, welcome, publication)?;
                        OutputFileSubmission::Negotiated(welcome)
                    }
                    Err(error) => {
                        let reason = match error {
                            OutputTransferError::UnsupportedRevision => {
                                OutputFileRefusal::UnsupportedRevision
                            }
                            OutputTransferError::UnsupportedCapability => {
                                OutputFileRefusal::ObservationRequired
                            }
                            _ => return Err(Errno::EINVAL),
                        };
                        self.journal.refused(submission, reason)?;
                        self.refused = true;
                        OutputFileSubmission::Refused(reason)
                    }
                }
            }
            OutputFileKind::Proposal => {
                let (transaction, message) =
                    decode_output_file_proposal(record.body, record.header.connection_epoch)
                        .map_err(|_| Errno::EINVAL)?;
                let proposal = AdmittedOutputProposal {
                    transaction,
                    message,
                };
                match trial.admit_proposal(transaction, proposal.message.clone(), snapshot) {
                    Ok(admission) => {
                        let replacement = match &admission {
                            OutputProposalAdmission::Queued {
                                replaced: Some(old),
                            } => Some(OutputFileReplacement {
                                transaction: old.transaction,
                                topology_epoch: snapshot.topology_epoch,
                            }),
                            _ => None,
                        };
                        self.journal
                            .prepare_proposal(submission, transaction, replacement)?
                            .commit();
                        OutputFileSubmission::Proposal {
                            proposal,
                            admission,
                        }
                    }
                    Err(OutputTransferError::InvalidCandidate(_)) => {
                        self.journal
                            .prepare_rejection(
                                submission,
                                transaction,
                                OutputV1Outcome {
                                    connection_epoch: record.header.connection_epoch,
                                    topology_epoch: snapshot.topology_epoch,
                                    kind: OutputV1OutcomeKind::Rejected,
                                    reason: SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT,
                                },
                            )?
                            .commit();
                        OutputFileSubmission::Rejected(transaction)
                    }
                    Err(OutputTransferError::TransactionCapacityExceeded) => {
                        return Err(Errno::ENOSPC);
                    }
                    Err(
                        OutputTransferError::NotNegotiated
                        | OutputTransferError::UnsupportedCapability,
                    ) => return Err(Errno::EACCES),
                    Err(_) => return Err(Errno::EINVAL),
                }
            }
            _ => return Err(Errno::EINVAL),
        };
        self.connection = trial;
        self.accepted = Some(Accepted {
            submission,
            bytes: bytes.to_vec(),
        });
        Ok(result)
    }

    /// Validate the active identity before spending a terminal credit. The
    /// caller revalidates any promoted proposal against the current topology.
    pub fn settle(
        &mut self,
        transaction: TransactionId,
        outcome: OutputV1Outcome,
    ) -> Result<Option<AdmittedOutputProposal>, Errno> {
        let mut trial = self.connection.clone();
        let promoted = trial
            .settle_active(transaction)
            .map_err(|_| Errno::EINVAL)?
            .cloned();
        self.journal.finish(transaction, outcome)?;
        self.connection = trial;
        Ok(promoted)
    }
}
