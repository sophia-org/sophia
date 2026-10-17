use crate::{Invalid, Request, RequestKind};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u16)]
pub enum RowKind {
    PublishRule = 1,
    EmitDescriptor = 2,
    RetireSurface = 3,
    Rejected = 4,
    #[default]
    NoChange = 5,
}
/// Unused fields must be zero/absent. Text is borrowed only during encoding.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResponseRow<'a> {
    pub kind: RowKind,
    pub surface_index: u32,
    pub surface_generation: u32,
    pub value: u16,
    pub disclosure: u16,
    pub trust: u16,
    pub attention: u16,
    pub icon: Option<u64>,
    pub redacted: bool,
    pub label: Option<&'a str>,
    pub generation: u64,
    pub grant_token: u64,
    pub grant_revocation_epoch: u64,
    pub grant_target_generation: u64,
}
fn valid(r: &ResponseRow<'_>) -> bool {
    let no_grant =
        r.grant_token == 0 && r.grant_revocation_epoch == 0 && r.grant_target_generation == 0;
    let no_text = r.label.is_none() && !r.redacted;
    match r.kind {
        RowKind::PublishRule => {
            r.value == 0
                && (1..=3).contains(&r.disclosure)
                && (1..=4).contains(&r.trust)
                && r.attention == 0
                && no_text
                && no_grant
        }
        RowKind::EmitDescriptor => {
            r.value == 0
                && r.disclosure == 0
                && (1..=4).contains(&r.trust)
                && (1..=3).contains(&r.attention)
                && r.generation != 0
                && r.grant_token != 0
                && r.grant_revocation_epoch != 0
                && r.grant_target_generation == r.generation
                && (!r.redacted || r.label.is_some())
                && r.label.is_none_or(|s| s.len() <= 128)
        }
        _ => {
            r.disclosure == 0
                && r.trust == 0
                && r.attention == 0
                && r.icon.is_none()
                && no_text
                && no_grant
                && r.generation == 0
                && if r.kind == RowKind::RetireSurface {
                    r.value == 0
                } else {
                    r.surface_index == 0
                        && r.surface_generation == 0
                        && if r.kind == RowKind::Rejected {
                            (1..=5).contains(&r.value)
                        } else {
                            r.value == 0
                        }
                }
        }
    }
}
fn correlated(request: &Request<'_>, rows: &[ResponseRow<'_>]) -> bool {
    if request.id == 0 || rows.is_empty() || rows.len() > 2 || !rows.iter().all(valid) {
        return false;
    }
    for row in rows {
        if matches!(
            row.kind,
            RowKind::PublishRule | RowKind::EmitDescriptor | RowKind::RetireSurface
        ) && (row.surface_index != request.surface_index
            || row.surface_generation != request.surface_generation)
        {
            return false;
        }
    }
    let first = &rows[0];
    if first.kind == RowKind::Rejected {
        return rows.len() == 1
            && (first.value == 5
                || match request.kind {
                    RequestKind::SurfaceAdmitted => matches!(first.value, 1 | 3),
                    RequestKind::CandidateReduced => matches!(first.value, 1 | 2 | 4),
                    _ => first.value == 1,
                });
    }
    match request.kind {
        RequestKind::SurfaceAdmitted | RequestKind::SetDisclosure => {
            first.kind == RowKind::PublishRule
                && (request.kind != RequestKind::SetDisclosure || first.disclosure == request.value)
                && (rows.len() == 1
                    || (rows[1].kind == RowKind::EmitDescriptor
                        && rows[1].label.is_none()
                        && !rows[1].redacted
                        && (request.kind != RequestKind::SurfaceAdmitted || first.disclosure == 2)))
        }
        RequestKind::CandidateReduced => {
            rows.len() == 1
                && (first.kind == RowKind::NoChange
                    || (first.kind == RowKind::EmitDescriptor
                        && first.generation == request.candidate_generation))
        }
        RequestKind::AttentionChanged => {
            rows.len() == 1
                && (first.kind == RowKind::NoChange
                    || (first.kind == RowKind::EmitDescriptor && first.attention == request.value))
        }
        RequestKind::SurfaceRemoved => rows.len() == 1 && first.kind == RowKind::RetireSurface,
    }
}
fn put(b: &mut [u8], at: usize, value: u64, size: usize) {
    b[at..at + size].copy_from_slice(&value.to_le_bytes()[..size]);
}
/// Encodes one atomic ResponseSet, preserving output on failure. This validates
/// request-visible relationships only; Session must still check current rules,
/// grants, generations and enqueue capacity before applying either row.
pub fn encode_response(
    epoch: u64,
    submission: u64,
    request: &Request<'_>,
    rows: &[ResponseRow<'_>],
    out: &mut [u8],
) -> Result<usize, Invalid> {
    if epoch == 0 || submission == 0 || out.len() < 448 || !correlated(request, rows) {
        return Err(Invalid);
    }
    let b = &mut out[..448];
    b.fill(0);
    put(b, 0, 448, 4);
    put(b, 4, 1, 2);
    put(b, 6, 257, 2);
    put(b, 8, epoch, 8);
    put(b, 16, submission, 8);
    put(b, 32, request.id, 8);
    put(b, 40, request.kind as u64, 2);
    put(b, 42, rows.len() as u64, 2);
    for (i, r) in rows.iter().enumerate() {
        let b = &mut b[48 + i * 200..48 + (i + 1) * 200];
        put(b, 0, r.kind as u64, 2);
        put(b, 2, u64::from(r.value), 2);
        put(b, 8, u64::from(r.surface_index), 4);
        put(b, 12, u64::from(r.surface_generation), 4);
        put(b, 16, u64::from(r.disclosure), 2);
        put(b, 18, u64::from(r.trust), 2);
        put(b, 20, u64::from(r.attention), 2);
        b[22] = u8::from(r.icon.is_some());
        b[23] = u8::from(r.redacted);
        put(b, 24, r.icon.unwrap_or(0), 8);
        put(b, 32, r.generation, 8);
        put(b, 40, r.grant_token, 8);
        put(b, 48, r.grant_revocation_epoch, 8);
        put(b, 56, r.grant_target_generation, 8);
        if let Some(label) = r.label {
            b[64] = 1;
            put(b, 66, label.len() as u64, 2);
            b[72..72 + label.len()].copy_from_slice(label.as_bytes());
        }
    }
    Ok(448)
}
