use super::{BrokerFileRequest, get, invalid};
use crate::{AttentionState, BinaryCodecError, MetadataDisclosure, SurfaceId};

/// A validated borrowed row. Owner-state and enqueue-capacity checks still
/// precede application; successful decoding alone does not authorize an effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokerFileResponseRow<'a> {
    pub kind: u16,
    pub value: u16,
    pub surface: SurfaceId,
    pub disclosure: u16,
    pub trust: u16,
    pub attention: u16,
    pub icon: Option<u64>,
    pub generation: u64,
    pub grant_token: u64,
    pub grant_revocation_epoch: u64,
    pub grant_target_generation: u64,
    pub label: Option<&'a str>,
    pub redacted: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerFileResponseSet<'a> {
    pub epoch: u64,
    pub submission: u64,
    pub request: u64,
    rows: [Option<BrokerFileResponseRow<'a>>; 2],
}
impl<'a> BrokerFileResponseSet<'a> {
    pub fn rows(&self) -> impl Iterator<Item = &BrokerFileResponseRow<'a>> {
        self.rows.iter().flatten()
    }
}
fn row(bytes: &[u8]) -> Result<BrokerFileResponseRow<'_>, BinaryCodecError> {
    if bytes.len() != 200
        || bytes[22] > 1
        || bytes[23] > 1
        || bytes[64] > 1
        || get(bytes, 4, 4) != 0
        || bytes[65] != 0
        || get(bytes, 68, 4) != 0
    {
        return Err(invalid());
    }
    let length = get(bytes, 66, 2) as usize;
    if length > 128
        || bytes[72 + length..].iter().any(|b| *b != 0)
        || (bytes[64] == 0 && (length != 0 || bytes[23] != 0))
        || (bytes[22] == 0 && get(bytes, 24, 8) != 0)
    {
        return Err(invalid());
    }
    let label = if bytes[64] == 1 {
        Some(std::str::from_utf8(&bytes[72..72 + length]).map_err(|_| invalid())?)
    } else {
        None
    };
    let value = BrokerFileResponseRow {
        kind: get(bytes, 0, 2) as u16,
        value: get(bytes, 2, 2) as u16,
        surface: SurfaceId::new(get(bytes, 8, 4) as u32, get(bytes, 12, 4) as u32),
        disclosure: get(bytes, 16, 2) as u16,
        trust: get(bytes, 18, 2) as u16,
        attention: get(bytes, 20, 2) as u16,
        icon: (bytes[22] == 1).then(|| get(bytes, 24, 8)),
        generation: get(bytes, 32, 8),
        grant_token: get(bytes, 40, 8),
        grant_revocation_epoch: get(bytes, 48, 8),
        grant_target_generation: get(bytes, 56, 8),
        label,
        redacted: bytes[23] == 1,
    };
    let no_grant = value.grant_token == 0
        && value.grant_revocation_epoch == 0
        && value.grant_target_generation == 0;
    let no_text = value.label.is_none() && !value.redacted;
    let valid = match value.kind {
        1 => {
            value.value == 0
                && (1..=3).contains(&value.disclosure)
                && (1..=4).contains(&value.trust)
                && value.attention == 0
                && no_grant
                && no_text
        }
        2 => {
            value.value == 0
                && value.disclosure == 0
                && (1..=4).contains(&value.trust)
                && (1..=3).contains(&value.attention)
                && value.generation > 0
                && value.grant_token > 0
                && value.grant_revocation_epoch > 0
                && value.grant_target_generation == value.generation
        }
        3..=5 => {
            value.disclosure == 0
                && value.trust == 0
                && value.attention == 0
                && value.icon.is_none()
                && value.generation == 0
                && no_grant
                && no_text
                && match value.kind {
                    3 => value.value == 0,
                    4 => (1..=5).contains(&value.value) && get(bytes, 8, 8) == 0,
                    _ => value.value == 0 && get(bytes, 8, 8) == 0,
                }
        }
        _ => false,
    };
    if !valid {
        return Err(invalid());
    }
    Ok(value)
}
/// Checks the complete native set and static request correlation before returning
/// either row. The owner must validate current generation, grants, disclosure
/// changes and all effect capacity together before taking custody.
pub fn decode_broker_file_response<'a>(
    bytes: &'a [u8],
    request_id: u64,
    request: BrokerFileRequest<'_>,
) -> Result<BrokerFileResponseSet<'a>, BinaryCodecError> {
    if bytes.len() != 448
        || get(bytes, 0, 4) != 448
        || get(bytes, 4, 2) != 1
        || get(bytes, 6, 2) != 257
        || get(bytes, 8, 8) == 0
        || get(bytes, 16, 8) == 0
        || get(bytes, 24, 8) != 0
        || request_id == 0
        || get(bytes, 32, 8) != request_id
        || get(bytes, 40, 2) != u64::from(request.kind())
        || !(1..=2).contains(&get(bytes, 42, 2))
        || get(bytes, 44, 4) != 0
    {
        return Err(invalid());
    }
    let first = row(&bytes[48..248])?;
    let second = if get(bytes, 42, 2) == 2 {
        Some(row(&bytes[248..])?)
    } else {
        if bytes[248..].iter().any(|b| *b != 0) {
            return Err(invalid());
        }
        None
    };
    for r in [Some(first), second].into_iter().flatten() {
        if matches!(r.kind, 1..=3) && r.surface != request.surface() {
            return Err(invalid());
        }
    }
    let valid = if first.kind == 4 {
        second.is_none()
            && (first.value == 5
                || match request.kind() {
                    1 => matches!(first.value, 1 | 3),
                    2 => matches!(first.value, 1 | 2 | 4),
                    _ => first.value == 1,
                })
    } else {
        match request {
            BrokerFileRequest::SurfaceAdmitted { .. } => {
                first.kind == 1
                    && second.is_none_or(|r| {
                        r.kind == 2 && r.label.is_none() && !r.redacted && first.disclosure == 2
                    })
            }
            BrokerFileRequest::SetDisclosure { disclosure, .. } => {
                first.kind == 1
                    && first.disclosure
                        == match disclosure {
                            MetadataDisclosure::None => 1,
                            MetadataDisclosure::ClassOnly => 2,
                            MetadataDisclosure::Full => 3,
                        }
                    && second.is_none_or(|r| r.kind == 2 && r.label.is_none() && !r.redacted)
            }
            BrokerFileRequest::CandidateReduced { generation, .. } => {
                second.is_none()
                    && (first.kind == 5 || (first.kind == 2 && first.generation == generation))
            }
            BrokerFileRequest::AttentionChanged { attention, .. } => {
                second.is_none()
                    && (first.kind == 5
                        || (first.kind == 2
                            && first.attention
                                == match attention {
                                    AttentionState::None => 1,
                                    AttentionState::Notice => 2,
                                    AttentionState::Critical => 3,
                                }))
            }
            BrokerFileRequest::SurfaceRemoved { .. } => second.is_none() && first.kind == 3,
        }
    };
    if !valid {
        return Err(invalid());
    }
    Ok(BrokerFileResponseSet {
        epoch: get(bytes, 8, 8),
        submission: get(bytes, 16, 8),
        request: request_id,
        rows: [Some(first), second],
    })
}
