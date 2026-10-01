//! Output-role admission and proposal custody, independent of either wire.
use std::collections::BTreeSet;

use sophia_protocol::{
    MAX_OUTPUT_AUTHORITY_GROUPS, MAX_OUTPUT_AUTHORITY_HEADS, MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP,
    MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD, OutputAuthoritySnapshot, OutputTopologyCandidateError,
    OutputV1ClientHello, OutputV1Proposal, OutputV1ServerWelcome,
    SOPHIA_OUTPUT_CAPABILITY_CONFIGURE, SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
    SOPHIA_OUTPUT_INTERFACE_REVISION, TransactionId,
};

const OUTPUT_SUPPORTED_CAPABILITIES: u64 =
    SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputTransferError {
    NotConnected,
    AlreadyConnected,
    NotNegotiated,
    AlreadyNegotiated,
    UnsupportedRevision,
    InvalidConnectionEpoch,
    InvalidTransaction,
    ReusedTransaction,
    TransactionCapacityExceeded,
    UnsupportedCapability,
    WrongActiveTransaction,
    InvalidCandidate(OutputTopologyCandidateError),
}

impl core::fmt::Display for OutputTransferError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OutputTransferError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedOutputProposal {
    pub transaction: TransactionId,
    pub message: OutputV1Proposal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputProposalAdmission {
    Active,
    Queued {
        replaced: Option<AdmittedOutputProposal>,
    },
}

/// Session-side reducer for the exclusive output-authority role.
///
/// The active candidate is never replaced in place. One complete latest
/// proposal may wait behind it; replacing that queued value returns the old
/// identity so the owner can emit an explicit stale terminal outcome.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct OutputConnectionState {
    connected: bool,
    negotiated: bool,
    connection_epoch: u64,
    selected_capabilities: u64,
    used_transactions: BTreeSet<TransactionId>,
    transaction_limit: Option<usize>,
    active: Option<AdmittedOutputProposal>,
    queued: Option<AdmittedOutputProposal>,
}

impl OutputConnectionState {
    /// Bound replay history without requiring ordered domain transaction IDs.
    /// The file export advertises this per-epoch limit. At exhaustion, new
    /// identities are refused before custody changes; already accepted work
    /// can still settle. A newer connection epoch starts a fresh history.
    pub fn with_transaction_limit(limit: usize) -> Self {
        Self {
            transaction_limit: Some(limit),
            ..Self::default()
        }
    }

    pub fn used_transaction_count(&self) -> usize {
        self.used_transactions.len()
    }

    pub fn connect(&mut self, connection_epoch: u64) -> Result<(), OutputTransferError> {
        if self.connected {
            return Err(OutputTransferError::AlreadyConnected);
        }
        if connection_epoch == 0 || connection_epoch <= self.connection_epoch {
            return Err(OutputTransferError::InvalidConnectionEpoch);
        }
        self.connected = true;
        self.negotiated = false;
        self.connection_epoch = connection_epoch;
        self.selected_capabilities = 0;
        self.used_transactions.clear();
        self.active = None;
        self.queued = None;
        Ok(())
    }

    pub fn negotiate(
        &mut self,
        hello: OutputV1ClientHello,
    ) -> Result<OutputV1ServerWelcome, OutputTransferError> {
        if !self.connected {
            return Err(OutputTransferError::NotConnected);
        }
        if self.negotiated {
            return Err(OutputTransferError::AlreadyNegotiated);
        }
        if hello.minimum_revision == 0
            || hello.minimum_revision > hello.maximum_revision
            || SOPHIA_OUTPUT_INTERFACE_REVISION < hello.minimum_revision
            || SOPHIA_OUTPUT_INTERFACE_REVISION > hello.maximum_revision
        {
            return Err(OutputTransferError::UnsupportedRevision);
        }
        if hello.capabilities & SOPHIA_OUTPUT_CAPABILITY_OBSERVE == 0 {
            return Err(OutputTransferError::UnsupportedCapability);
        }
        self.negotiated = true;
        self.selected_capabilities = hello.capabilities & OUTPUT_SUPPORTED_CAPABILITIES;
        Ok(OutputV1ServerWelcome {
            selected_revision: SOPHIA_OUTPUT_INTERFACE_REVISION,
            capabilities: self.selected_capabilities,
            connection_epoch: self.connection_epoch,
            max_heads: MAX_OUTPUT_AUTHORITY_HEADS as u16,
            max_groups: MAX_OUTPUT_AUTHORITY_GROUPS as u16,
            max_modes_per_head: MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD as u16,
            max_heads_per_group: MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP as u16,
        })
    }

    pub fn admit_proposal(
        &mut self,
        transaction: TransactionId,
        message: OutputV1Proposal,
        snapshot: &OutputAuthoritySnapshot,
    ) -> Result<OutputProposalAdmission, OutputTransferError> {
        self.require_capability(SOPHIA_OUTPUT_CAPABILITY_CONFIGURE)?;
        if message.connection_epoch != self.connection_epoch {
            return Err(OutputTransferError::InvalidConnectionEpoch);
        }
        if !transaction.is_valid() {
            return Err(OutputTransferError::InvalidTransaction);
        }
        if self.used_transactions.contains(&transaction) {
            return Err(OutputTransferError::ReusedTransaction);
        }
        if self
            .transaction_limit
            .is_some_and(|limit| self.used_transactions.len() >= limit)
        {
            return Err(OutputTransferError::TransactionCapacityExceeded);
        }
        self.used_transactions.insert(transaction);
        message
            .candidate
            .validate_against(snapshot)
            .map_err(OutputTransferError::InvalidCandidate)?;

        let proposal = AdmittedOutputProposal {
            transaction,
            message,
        };
        if self.active.is_none() {
            self.active = Some(proposal);
            Ok(OutputProposalAdmission::Active)
        } else {
            let replaced = self.queued.replace(proposal);
            Ok(OutputProposalAdmission::Queued { replaced })
        }
    }

    pub fn active(&self) -> Option<&AdmittedOutputProposal> {
        self.active.as_ref()
    }

    pub fn settle_active(
        &mut self,
        transaction: TransactionId,
    ) -> Result<Option<&AdmittedOutputProposal>, OutputTransferError> {
        if self.active.as_ref().map(|active| active.transaction) != Some(transaction) {
            return Err(OutputTransferError::WrongActiveTransaction);
        }
        self.active = self.queued.take();
        Ok(self.active.as_ref())
    }

    pub fn disconnect(&mut self) -> Result<Vec<AdmittedOutputProposal>, OutputTransferError> {
        if !self.connected {
            return Err(OutputTransferError::NotConnected);
        }
        self.connected = false;
        self.negotiated = false;
        self.selected_capabilities = 0;
        let mut abandoned = Vec::with_capacity(2);
        if let Some(active) = self.active.take() {
            abandoned.push(active);
        }
        if let Some(queued) = self.queued.take() {
            abandoned.push(queued);
        }
        Ok(abandoned)
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub const fn selected_capabilities(&self) -> u64 {
        self.selected_capabilities
    }

    pub fn require_observe(&self) -> Result<(), OutputTransferError> {
        self.require_capability(SOPHIA_OUTPUT_CAPABILITY_OBSERVE)
    }

    fn require_capability(&self, capability: u64) -> Result<(), OutputTransferError> {
        if !self.connected {
            return Err(OutputTransferError::NotConnected);
        }
        if !self.negotiated {
            return Err(OutputTransferError::NotNegotiated);
        }
        if self.selected_capabilities & capability == 0 {
            return Err(OutputTransferError::UnsupportedCapability);
        }
        Ok(())
    }
}
