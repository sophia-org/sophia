//! Passive exact-profile identities shared by transport adapters. Validation
//! keeps the established errors; neither records nor aliases own handoff phase.
use crate::{
    IpcCodecError, SOPHIA_WM_OUTCOME_PROFILE_ACCEPTED, SOPHIA_WM_OUTCOME_PROFILE_REJECTED_IDENTITY,
    SOPHIA_WM_OUTCOME_PROFILE_REJECTED_STATE, TransactionId,
};

pub const POLICY_PROFILE_DIGEST_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProfileIdentity {
    pub connection_epoch: u64,
    pub profile_generation: u64,
    pub profile_digest: [u8; POLICY_PROFILE_DIGEST_BYTES],
}

impl PolicyProfileIdentity {
    pub fn new(
        connection_epoch: u64,
        profile_generation: u64,
        profile_digest: [u8; POLICY_PROFILE_DIGEST_BYTES],
    ) -> Result<Self, IpcCodecError> {
        if connection_epoch == 0 {
            return Err(IpcCodecError::InvalidProfileIdentity("connection_epoch"));
        }
        if profile_generation == 0 {
            return Err(IpcCodecError::InvalidProfileIdentity("profile_generation"));
        }
        if profile_digest == [0; POLICY_PROFILE_DIGEST_BYTES] {
            return Err(IpcCodecError::InvalidProfileIdentity("profile_digest"));
        }
        Ok(Self {
            connection_epoch,
            profile_generation,
            profile_digest,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum PolicyProfileOutcome {
    Accepted = SOPHIA_WM_OUTCOME_PROFILE_ACCEPTED,
    RejectedIdentity = SOPHIA_WM_OUTCOME_PROFILE_REJECTED_IDENTITY,
    RejectedState = SOPHIA_WM_OUTCOME_PROFILE_REJECTED_STATE,
}

impl TryFrom<u16> for PolicyProfileOutcome {
    type Error = IpcCodecError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            SOPHIA_WM_OUTCOME_PROFILE_ACCEPTED => Ok(Self::Accepted),
            SOPHIA_WM_OUTCOME_PROFILE_REJECTED_IDENTITY => Ok(Self::RejectedIdentity),
            SOPHIA_WM_OUTCOME_PROFILE_REJECTED_STATE => Ok(Self::RejectedState),
            _ => Err(IpcCodecError::InvalidEnum {
                field: "profile_outcome",
                value: u32::from(value),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProfileCommand {
    pub transaction: TransactionId,
    pub identity: PolicyProfileIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProfileCompletion {
    pub transaction: TransactionId,
    pub identity: PolicyProfileIdentity,
    pub outcome: PolicyProfileOutcome,
}
