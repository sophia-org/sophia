//! Events from the bounded output file worker to its topology owner.
use crate::{AdmittedOutputProposal, OutputProposalAdmission};
use sophia_protocol::TransactionId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputFileServiceEvent {
    Connected {
        connection_epoch: u64,
    },
    Proposal {
        proposal: AdmittedOutputProposal,
        admission: OutputProposalAdmission,
    },
    Promoted(AdmittedOutputProposal),
    ProposalRejected {
        transaction: TransactionId,
        message: String,
    },
    Disconnected {
        connection_epoch: u64,
    },
    ConnectionRejected {
        message: String,
    },
    AssigneeReplaced {
        connection_epoch: u64,
        abandoned: Vec<AdmittedOutputProposal>,
    },
    Failed {
        message: String,
    },
}
