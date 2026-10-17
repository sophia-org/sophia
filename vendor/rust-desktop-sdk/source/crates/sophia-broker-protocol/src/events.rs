use crate::Invalid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum RequestKind {
    SurfaceAdmitted = 1,
    CandidateReduced = 2,
    AttentionChanged = 3,
    SurfaceRemoved = 4,
    SetDisclosure = 5,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request<'a> {
    pub id: u64,
    pub kind: RequestKind,
    pub surface_index: u32,
    pub surface_generation: u32,
    /// Namespace profile, disclosure or attention according to kind; zero for removal.
    pub value: u16,
    pub redacted: bool,
    pub label: Option<&'a str>,
    pub candidate_generation: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    Negotiated {
        max_surfaces: u32,
        max_label_bytes: u16,
    },
    Refused,
    Submitted {
        submission: u64,
        candidate_kind: u16,
    },
    Request(Request<'a>),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventRecord<'a> {
    pub epoch: u64,
    pub sequence: u64,
    pub event: Event<'a>,
}
fn n(b: &[u8], at: usize, width: usize) -> u64 {
    b[at..at + width]
        .iter()
        .enumerate()
        .fold(0, |v, (i, b)| v | (u64::from(*b) << (8 * i)))
}
pub fn decode_event(b: &[u8], epoch: u64) -> Result<EventRecord<'_>, Invalid> {
    if b.len() < 32
        || epoch == 0
        || n(b, 0, 4) != b.len() as u64
        || n(b, 4, 2) != 1
        || n(b, 8, 8) != epoch
        || n(b, 16, 8) != 0
        || n(b, 24, 8) == 0
    {
        return Err(Invalid);
    }
    let event = match (n(b, 6, 2), b.len()) {
        (16, 56) => {
            if n(b, 32, 2) != 2
                || n(b, 34, 2) != 0
                || !(1..=1024).contains(&n(b, 36, 4))
                || n(b, 40, 8) != epoch
                || !(1..=128).contains(&n(b, 48, 2))
                || n(b, 50, 2) != 2
                || n(b, 52, 4) != 0
            {
                return Err(Invalid);
            }
            Event::Negotiated {
                max_surfaces: n(b, 36, 4) as u32,
                max_label_bytes: n(b, 48, 2) as u16,
            }
        }
        (17, 40) if n(b, 32, 2) == 1 && b[34..].iter().all(|b| *b == 0) => Event::Refused,
        (18, 48)
            if n(b, 32, 8) != 0
                && (256..=257).contains(&n(b, 40, 2))
                && b[42..].iter().all(|b| *b == 0) =>
        {
            Event::Submitted {
                submission: n(b, 32, 8),
                candidate_kind: n(b, 40, 2) as u16,
            }
        }
        (32, 200) => Event::Request(request(&b[32..])?),
        _ => return Err(Invalid),
    };
    Ok(EventRecord {
        epoch,
        sequence: n(b, 24, 8),
        event,
    })
}
fn request(b: &[u8]) -> Result<Request<'_>, Invalid> {
    let kind = match n(b, 8, 2) {
        1 => RequestKind::SurfaceAdmitted,
        2 => RequestKind::CandidateReduced,
        3 => RequestKind::AttentionChanged,
        4 => RequestKind::SurfaceRemoved,
        5 => RequestKind::SetDisclosure,
        _ => return Err(Invalid),
    };
    let value = n(b, 24, 2) as u16;
    let valid_value = match kind {
        RequestKind::SurfaceAdmitted => (1..=2).contains(&value),
        RequestKind::SurfaceRemoved => value == 0,
        _ => (1..=3).contains(&value),
    };
    if n(b, 0, 8) == 0
        || b[10..16].iter().any(|b| *b != 0)
        || !valid_value
        || n(b, 30, 2) != 0
        || b[26] > 1
        || b[27] > 1
        || (b[26] == 1 && b[27] == 0)
    {
        return Err(Invalid);
    }
    let length = n(b, 28, 2) as usize;
    if length > 128
        || (b[27] == 0 && length != 0)
        || b[40 + length..].iter().any(|b| *b != 0)
        || (kind != RequestKind::CandidateReduced && b[26..].iter().any(|b| *b != 0))
    {
        return Err(Invalid);
    }
    let label = if b[27] == 1 {
        Some(std::str::from_utf8(&b[40..40 + length]).map_err(|_| Invalid)?)
    } else {
        None
    };
    Ok(Request {
        id: n(b, 0, 8),
        kind,
        surface_index: n(b, 16, 4) as u32,
        surface_generation: n(b, 20, 4) as u32,
        value,
        redacted: b[26] != 0,
        label,
        candidate_generation: n(b, 32, 8),
    })
}
