use super::super::codec::{u16_at, u32_at, u64_at};
use super::*;
use crate::shell::encoding::content::{
    ShellContentValueKind, decode_content_candidate, decode_shell_content_value,
};

fn key(bytes: &[u8]) -> Result<ShellFrameStreamKey, ShellFilePayloadError> {
    Ok(ShellFrameStreamKey {
        grant: ContentGrant {
            connection_epoch: u64_at(bytes, 8)?,
            content_grant_epoch: u64_at(bytes, 16)?,
        },
        output: ContentOutputId {
            id: u64_at(bytes, 24)?,
            generation: u64_at(bytes, 32)?,
        },
        allocation: ContentAllocationId {
            id: u64_at(bytes, 40)?,
            generation: u64_at(bytes, 48)?,
        },
        facts_generation: u64_at(bytes, 56)?,
        stream_id: u64_at(bytes, 64)?,
    })
}

fn fixed(bytes: &[u8], len: usize) -> Result<(), ShellFilePayloadError> {
    if bytes.len() != len {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(())
}

fn commit(
    bytes: &[u8],
    transaction: TransactionId,
) -> Result<ShellFrameCommit, ShellFilePayloadError> {
    let scene_bytes = u32_at(bytes, 8)? as usize;
    let flags = u16_at(bytes, 12)?;
    if flags & !3 != 0 {
        return Err(ShellFilePayloadError::Value);
    }
    if u16_at(bytes, 14)? != 0 {
        return Err(ShellFileCodecError::Reserved.into());
    }
    let scene_end = 16usize
        .checked_add(scene_bytes)
        .ok_or(ShellFileCodecError::Length)?;
    let total = scene_end
        .checked_add(if flags & 1 != 0 { 152 } else { 0 })
        .and_then(|n| n.checked_add(if flags & 2 != 0 { 24 } else { 0 }))
        .ok_or(ShellFileCodecError::Length)?;
    fixed(bytes, total)?;
    let scene = bytes
        .get(16..scene_end)
        .ok_or(ShellFileCodecError::Length)?;
    // Prove the complete extent and all row counts before allocating any row.
    let surfaces = usize::from(u16_at(scene, 64)?);
    let placements = usize::from(u16_at(scene, 66)?);
    let targets = usize::from(u16_at(scene, 68)?);
    if surfaces != 1 || placements > 32 || targets > 64 {
        return Err(ShellFilePayloadError::Value);
    }
    fixed(scene, 72 + 64 + placements * 32 + targets * 48)?;
    let candidate = decode_content_candidate(scene)?;
    let image = if flags & 1 != 0 {
        let RingControl::Offer(offer) = RingControl::decode(&bytes[scene_end..scene_end + 152])
            .map_err(|_| ShellFilePayloadError::Value)?
        else {
            return Err(ShellFilePayloadError::Value);
        };
        Some(offer)
    } else {
        None
    };
    let retire = if flags & 2 != 0 {
        let at = total - 24;
        Some(ShellFrameRetire {
            transaction: TransactionId::from_raw(u64_at(bytes, at)?),
            resource: ContentResourceId {
                id: u64_at(bytes, at + 8)?,
                generation: u64_at(bytes, at + 16)?,
            },
        })
    } else {
        None
    };
    Ok(ShellFrameCommit {
        transaction,
        candidate,
        image,
        retire,
    })
}

/// Decode only the explicit r13 vocabulary. `max_bytes` is the granted record
/// limit (at most 8192), checked before any scene allocation. Capability and
/// current authority must be checked by the connection before calling this.
pub fn decode_shell_frame_record(
    bytes: &[u8],
    expected: ShellFileClass,
    revision: u16,
    max_bytes: usize,
) -> Result<ShellFrameRecord, ShellFilePayloadError> {
    if revision != SHELL_FRAME_REVISION {
        return Err(ShellFileCodecError::Version.into());
    }
    if bytes.len() > max_bytes.min(SHELL_FILE_MAX_CANDIDATE_BYTES) {
        return Err(ShellFileCodecError::Length.into());
    }
    let record = decode_shell_file_record_at_revision(bytes, expected, revision)?;
    let body = record.body;
    let transaction = TransactionId::from_raw(u64_at(body, 0)?);
    let value = match record.header.kind {
        ShellFileKind::FrameStreamOpen | ShellFileKind::FrameStreamClose => {
            fixed(body, 72)?;
            let key = key(body)?;
            if record.header.kind == ShellFileKind::FrameStreamOpen {
                ShellFrameRecord::Open { transaction, key }
            } else {
                ShellFrameRecord::Close { transaction, key }
            }
        }
        ShellFileKind::FrameStreamOpened => {
            fixed(body, 80)?;
            if u32_at(body, 72)? != 1 {
                return Err(ShellFilePayloadError::Value);
            }
            ShellFrameRecord::Opened {
                transaction,
                key: key(body)?,
                ttl_millis: u32_at(body, 76)?,
            }
        }
        ShellFileKind::FrameStreamClosed => {
            fixed(body, 80)?;
            super::super::payload::reserved(&body[74..])?;
            ShellFrameRecord::Closed {
                transaction,
                key: key(body)?,
                reason: u16_at(body, 72)?,
            }
        }
        ShellFileKind::FrameRefused => {
            fixed(body, 64)?;
            super::super::payload::reserved(&body[60..])?;
            ShellFrameRecord::Refused {
                transaction,
                grant: ContentGrant {
                    connection_epoch: u64_at(body, 8)?,
                    content_grant_epoch: u64_at(body, 16)?,
                },
                output: ContentOutputId {
                    id: u64_at(body, 24)?,
                    generation: u64_at(body, 32)?,
                },
                stream_id: u64_at(body, 40)?,
                candidate_generation: u64_at(body, 48)?,
                request_kind: super::super::codec::kind_at_revision(u16_at(body, 56)?, revision)?,
                reason: u16_at(body, 58)?,
            }
        }
        ShellFileKind::FrameCommit => ShellFrameRecord::Commit(commit(body, transaction)?),
        ShellFileKind::FrameOutcome => {
            fixed(body, 116)?;
            let crate::ShellContentRecord::CandidateOutcome(outcome) =
                decode_shell_content_value(ShellContentValueKind::CandidateOutcome, &body[8..76])?
            else {
                return Err(ShellFilePayloadError::Value);
            };
            ShellFrameRecord::Outcome {
                transaction,
                outcome,
                stream_id: u64_at(body, 76)?,
                allocation: ContentAllocationId {
                    id: u64_at(body, 84)?,
                    generation: u64_at(body, 92)?,
                },
                facts_generation: u64_at(body, 100)?,
                credit_expires_msec: u64_at(body, 108)?,
            }
        }
        _ => return Err(ShellFileCodecError::Kind.into()),
    };
    envelope(record.header, &value, revision)?;
    record_bytes(&value)?;
    Ok(value)
}
