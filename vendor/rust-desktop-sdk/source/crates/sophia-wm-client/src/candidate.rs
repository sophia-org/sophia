use sophia_wm_protocol::{wm_files::*, *};

/// Caller-owned values for the WM candidate lane. Negotiation belongs to the
/// session bootstrap and is deliberately absent from application submissions.
#[derive(Clone, Debug, PartialEq)]
pub enum Candidate {
    ProfilePrepared(PolicyProfileCompletion),
    ProfileActive(PolicyProfileCompletion),
    ProfileRolledBack(PolicyProfileCompletion),
    Configuration(WmFileConfiguration),
    Dirty(PolicyDirtyRequest),
    Projection(Box<PolicyProjectionProposal>),
    SessionOperation(WmFileSessionOperation),
}

/// Validated, immutable wire bytes. This value establishes neither local
/// admission nor server custody. Only the issuing session may assign an epoch
/// and submission ID; callers using this encoder directly must supply them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedCandidate {
    bytes: Vec<u8>,
    header: WmFileHeader,
    domain: Option<TransactionId>,
}

impl Candidate {
    pub fn kind(&self) -> WmFileKind {
        match self {
            Self::ProfilePrepared(_) => WmFileKind::ProfilePrepared,
            Self::ProfileActive(_) => WmFileKind::ProfileActive,
            Self::ProfileRolledBack(_) => WmFileKind::ProfileRolledBack,
            Self::Configuration(_) => WmFileKind::Configuration,
            Self::Dirty(_) => WmFileKind::Dirty,
            Self::Projection(_) => WmFileKind::Projection,
            Self::SessionOperation(_) => WmFileKind::SessionOperation,
        }
    }

    /// The independent domain sequence applies only to configuration,
    /// projection and session operations. Profile completions echo the
    /// server's transaction; Dirty carries a policy generation instead.
    pub fn domain_transaction(&self) -> Option<TransactionId> {
        match self {
            Self::Configuration(value) => Some(value.transaction),
            Self::Projection(value) => Some(value.transaction),
            Self::SessionOperation(value) => Some(value.transaction),
            _ => None,
        }
    }

    /// Validate against negotiated capabilities and copy all caller-owned
    /// data. Retry this result unchanged, rather than encoding mutable policy
    /// state again under an existing submission ID.
    pub fn encode(
        &self,
        epoch: u64,
        submission_id: u64,
        capabilities: u64,
    ) -> Result<EncodedCandidate, WmFilePayloadError> {
        let header = WmFileHeader {
            kind: self.kind(),
            connection_epoch: epoch,
            submission_id,
            sequence: 0,
        };
        let bytes = match self {
            Self::ProfilePrepared(value)
            | Self::ProfileActive(value)
            | Self::ProfileRolledBack(value) => {
                encode_wm_file_profile_completion(header, *value, capabilities)?
            }
            Self::Configuration(value) => {
                encode_wm_file_configuration(header, value, capabilities)?
            }
            Self::Dirty(value) => encode_wm_file_dirty(header, value, capabilities)?,
            Self::Projection(value) => encode_wm_file_projection(header, value, capabilities)?,
            Self::SessionOperation(value) => {
                encode_wm_file_session_operation(header, value, capabilities)?
            }
        };
        Ok(EncodedCandidate {
            bytes,
            header,
            domain: self.domain_transaction(),
        })
    }
}

impl EncodedCandidate {
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn header(&self) -> WmFileHeader {
        self.header
    }
    pub fn domain_transaction(&self) -> Option<TransactionId> {
        self.domain
    }
}
