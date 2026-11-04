//! Explicit SFT r13 values. These codecs prove wire shape, not stream authority,
//! negotiated capability, available credit, source custody or presentation.
mod commit;
mod decode;

use super::codec::validate_header;
use super::*;
use crate::{
    ContentAllocationId, ContentCandidate, ContentCandidateOutcome, ContentGrant, ContentOutputId,
    ContentResourceId, TransactionId,
};

pub const SHELL_FRAME_REVISION: u16 = 13;
pub const SHELL_FRAME_CAPABILITY: u64 = 1 << 18;

/// Exact binding of one stream; all words must be nonzero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFrameStreamKey {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub facts_generation: u64,
    pub stream_id: u64,
}

/// A predecessor intent is part of admission, not an immediate resource retire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFrameRetire {
    pub transaction: TransactionId,
    pub resource: ContentResourceId,
}

/// One complete CPU scene, optionally with one new registered-ring image.
/// Moving this value transfers the row vectors; decoding owns them exactly once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFrameCommit {
    pub transaction: TransactionId,
    pub candidate: ContentCandidate,
    pub image: Option<RingOffer>,
    pub retire: Option<ShellFrameRetire>,
}

/// SFT-specific records. Existing image/resource events retain their codecs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellFrameRecord {
    Open {
        transaction: TransactionId,
        key: ShellFrameStreamKey,
    },
    Close {
        transaction: TransactionId,
        key: ShellFrameStreamKey,
    },
    Opened {
        transaction: TransactionId,
        key: ShellFrameStreamKey,
        ttl_millis: u32,
    },
    Closed {
        transaction: TransactionId,
        key: ShellFrameStreamKey,
        reason: u16,
    },
    Refused {
        transaction: TransactionId,
        grant: ContentGrant,
        output: ContentOutputId,
        stream_id: u64,
        candidate_generation: u64,
        request_kind: ShellFileKind,
        reason: u16,
    },
    Commit(ShellFrameCommit),
    Outcome {
        transaction: TransactionId,
        outcome: ContentCandidateOutcome,
        stream_id: u64,
        allocation: ContentAllocationId,
        facts_generation: u64,
        /// Owner-clock deadline, opaque to the client; zero means no returned credit.
        credit_expires_msec: u64,
    },
}

impl ShellFrameRecord {
    pub fn kind(&self) -> ShellFileKind {
        match self {
            Self::Open { .. } => ShellFileKind::FrameStreamOpen,
            Self::Close { .. } => ShellFileKind::FrameStreamClose,
            Self::Opened { .. } => ShellFileKind::FrameStreamOpened,
            Self::Closed { .. } => ShellFileKind::FrameStreamClosed,
            Self::Refused { .. } => ShellFileKind::FrameRefused,
            Self::Commit(_) => ShellFileKind::FrameCommit,
            Self::Outcome { .. } => ShellFileKind::FrameOutcome,
        }
    }

    fn grant(&self) -> ContentGrant {
        match self {
            Self::Open { key, .. }
            | Self::Close { key, .. }
            | Self::Opened { key, .. }
            | Self::Closed { key, .. } => key.grant,
            Self::Refused { grant, .. } => *grant,
            Self::Commit(v) => v.candidate.grant,
            Self::Outcome { outcome, .. } => outcome.grant,
        }
    }

    fn transaction(&self) -> TransactionId {
        match self {
            Self::Open { transaction, .. }
            | Self::Close { transaction, .. }
            | Self::Opened { transaction, .. }
            | Self::Closed { transaction, .. }
            | Self::Refused { transaction, .. }
            | Self::Outcome { transaction, .. } => *transaction,
            Self::Commit(v) => v.transaction,
        }
    }
}

fn nonzero(words: &[u64]) -> Result<(), ShellFilePayloadError> {
    if words.contains(&0) {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(())
}

fn reason(value: u16) -> Result<(), ShellFilePayloadError> {
    if !(1..=12).contains(&value) {
        return Err(ShellFilePayloadError::Value);
    }
    Ok(())
}

fn words(bytes: &mut Vec<u8>, values: &[u64]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn key_words(key: ShellFrameStreamKey) -> [u64; 8] {
    [
        key.grant.connection_epoch,
        key.grant.content_grant_epoch,
        key.output.id,
        key.output.generation,
        key.allocation.id,
        key.allocation.generation,
        key.facts_generation,
        key.stream_id,
    ]
}

fn record_bytes(value: &ShellFrameRecord) -> Result<usize, ShellFilePayloadError> {
    nonzero(&[value.transaction().raw()])?;
    let body = match value {
        ShellFrameRecord::Open { key, .. } | ShellFrameRecord::Close { key, .. } => {
            nonzero(&key_words(*key))?;
            72
        }
        ShellFrameRecord::Opened {
            key, ttl_millis, ..
        } => {
            nonzero(&key_words(*key))?;
            if !(1..=250).contains(ttl_millis) {
                return Err(ShellFilePayloadError::Value);
            }
            80
        }
        ShellFrameRecord::Closed {
            key, reason: why, ..
        } => {
            nonzero(&key_words(*key))?;
            reason(*why)?;
            80
        }
        ShellFrameRecord::Refused {
            grant,
            output,
            stream_id,
            candidate_generation,
            request_kind,
            reason: why,
            ..
        } => {
            nonzero(&[
                grant.connection_epoch,
                grant.content_grant_epoch,
                output.id,
                output.generation,
                *stream_id,
            ])?;
            reason(*why)?;
            if !matches!(
                request_kind,
                ShellFileKind::FrameStreamOpen
                    | ShellFileKind::FrameStreamClose
                    | ShellFileKind::FrameCommit
            ) || (*candidate_generation != 0) != (*request_kind == ShellFileKind::FrameCommit)
            {
                return Err(ShellFilePayloadError::Value);
            }
            64
        }
        ShellFrameRecord::Commit(v) => commit::body_bytes(v)?,
        ShellFrameRecord::Outcome {
            outcome,
            stream_id,
            allocation,
            facts_generation,
            credit_expires_msec,
            ..
        } => {
            nonzero(&[
                *stream_id,
                allocation.id,
                allocation.generation,
                *facts_generation,
            ])?;
            crate::shell::content::validation::validate(
                &crate::ShellContentRecord::CandidateOutcome(outcome.clone()),
            )
            .map_err(crate::shell::encoding::ValueError::from)?;
            if outcome.kind == 1 && *credit_expires_msec != 0 {
                return Err(ShellFilePayloadError::Value);
            }
            116
        }
    };
    super::payload::sized_record(0, body, SHELL_FILE_MAX_CANDIDATE_BYTES)
}

fn envelope(
    header: ShellFileHeader,
    value: &ShellFrameRecord,
    revision: u16,
) -> Result<(), ShellFilePayloadError> {
    if revision != SHELL_FRAME_REVISION {
        return Err(ShellFileCodecError::Version.into());
    }
    validate_header(header)?;
    if header.kind != value.kind() {
        return Err(ShellFileCodecError::Kind.into());
    }
    super::payload::same_epoch(header, value.grant().connection_epoch)
}

/// Validated encoded size without copying scene rows. The grant's byte limit may
/// be smaller than the protocol cap and must be checked by the caller.
pub fn shell_frame_record_bytes(
    header: ShellFileHeader,
    value: &ShellFrameRecord,
    revision: u16,
) -> Result<usize, ShellFilePayloadError> {
    envelope(header, value, revision)?;
    record_bytes(value)
}

/// Encode into one allocation; no scene-row copy or temporary candidate buffer.
pub fn encode_shell_frame_record(
    header: ShellFileHeader,
    value: &ShellFrameRecord,
    revision: u16,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    let size = shell_frame_record_bytes(header, value, revision)?;
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(&(size as u32).to_le_bytes());
    bytes.extend_from_slice(&SHELL_FILE_API_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(header.kind as u16).to_le_bytes());
    words(
        &mut bytes,
        &[
            header.connection_epoch,
            header.submission_id,
            header.sequence,
            value.transaction().raw(),
        ],
    );
    match value {
        ShellFrameRecord::Open { key, .. }
        | ShellFrameRecord::Close { key, .. }
        | ShellFrameRecord::Opened { key, .. }
        | ShellFrameRecord::Closed { key, .. } => {
            words(&mut bytes, &key_words(*key));
            if let ShellFrameRecord::Opened { ttl_millis, .. } = value {
                bytes.extend_from_slice(&1u32.to_le_bytes());
                bytes.extend_from_slice(&ttl_millis.to_le_bytes());
            } else if let ShellFrameRecord::Closed { reason, .. } = value {
                bytes.extend_from_slice(&reason.to_le_bytes());
                bytes.extend_from_slice(&[0; 6]);
            }
        }
        ShellFrameRecord::Refused {
            grant,
            output,
            stream_id,
            candidate_generation,
            request_kind,
            reason,
            ..
        } => {
            words(
                &mut bytes,
                &[
                    grant.connection_epoch,
                    grant.content_grant_epoch,
                    output.id,
                    output.generation,
                    *stream_id,
                    *candidate_generation,
                ],
            );
            bytes.extend_from_slice(&(*request_kind as u16).to_le_bytes());
            bytes.extend_from_slice(&reason.to_le_bytes());
            bytes.extend_from_slice(&[0; 4]);
        }
        ShellFrameRecord::Commit(v) => commit::put(v, &mut bytes),
        ShellFrameRecord::Outcome {
            outcome,
            stream_id,
            allocation,
            facts_generation,
            credit_expires_msec,
            ..
        } => {
            use crate::shell::encoding::Wire;
            outcome.put(&mut bytes);
            words(
                &mut bytes,
                &[
                    *stream_id,
                    allocation.id,
                    allocation.generation,
                    *facts_generation,
                    *credit_expires_msec,
                ],
            );
        }
    }
    debug_assert_eq!(bytes.len(), size);
    Ok(bytes)
}

pub use decode::decode_shell_frame_record;
