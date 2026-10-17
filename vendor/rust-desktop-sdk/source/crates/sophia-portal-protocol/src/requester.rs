//! Trusted requester records, distinct from recipient discovery and pixels.
//! Passive codecs establish neither admission, custody nor permission to execute.
use crate::RecordError;
pub const CANDIDATE_MAX: usize = 344;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum TransferKind {
    Clipboard = 1,
    DragAndDrop,
    FileHandoff,
    ScreenCapture,
    ScreenRecording,
    UriOpen,
    Notification,
}
fn kind(v: u16) -> Result<TransferKind, RecordError> {
    Ok(match v {
        1 => TransferKind::Clipboard,
        2 => TransferKind::DragAndDrop,
        3 => TransferKind::FileHandoff,
        4 => TransferKind::ScreenCapture,
        5 => TransferKind::ScreenRecording,
        6 => TransferKind::UriOpen,
        7 => TransferKind::Notification,
        _ => return Err(RecordError),
    })
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferRequest<'a> {
    pub transfer: u64,
    pub source: u64,
    pub target: u64,
    pub kind: TransferKind,
    pub mime: Option<&'a str>,
    pub byte_size: u64,
    pub generation: u64,
    pub deadline_msec: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Candidate<'a> {
    Negotiate { minimum: u16, maximum: u16 },
    Request(TransferRequest<'a>),
    Begin { transfer: u64, total_bytes: u64 },
    End { transfer: u64, total_bytes: u64 },
    Cancel { transfer: u64 },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_transfers: u32,
    pub max_payload_bytes: u32,
    pub journal_records: u32,
    pub journal_bytes: u32,
    pub assembly_timeout_ms: u32,
    pub ack_progress_timeout_ms: u32,
    pub max_retained_transfers: u32,
    pub broker_generation: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grant {
    pub transfer: u64,
    pub kind: TransferKind,
    pub source: u64,
    pub target: u64,
    pub source_generation: u64,
    pub broker_generation: u64,
    pub deadline_msec: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Completed,
    Revoked,
    Expired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Negotiated,
    Refused,
    Submitted {
        submission: u64,
        candidate_kind: u16,
    },
    Denied {
        transfer: u64,
    },
    Allowed(Grant),
    TransferOutcome {
        transfer: u64,
        outcome: Outcome,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventRecord {
    pub epoch: u64,
    pub sequence: u64,
    pub event: Event,
}
fn u16at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
fn u32at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn p16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn p32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn p64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
fn zero(b: &[u8]) -> bool {
    b.iter().all(|v| *v == 0)
}
fn header(b: &[u8], event: bool) -> Result<(u16, u64, u64), RecordError> {
    if b.len() < 32
        || u32at(b, 0) as usize != b.len()
        || u16at(b, 4) != 1
        || u64at(b, 8) == 0
        || u64at(b, 16) != 0
        || (event && u64at(b, 24) == 0)
        || (!event && u64at(b, 24) != 0)
    {
        return Err(RecordError);
    }
    Ok((u16at(b, 6), u64at(b, 8), u64at(b, 24)))
}
/// Exact canonical discovery; the returned epoch confers no authority.
pub fn decode_api(b: &[u8]) -> Result<u64, RecordError> {
    let digits = b
        .strip_prefix(b"sophia-portal-files version=1 role=portal-requester epoch=")
        .and_then(|v| v.strip_suffix(b" fd_transfer=none\n"))
        .ok_or(RecordError)?;
    if digits.is_empty() || digits.len() > 20 || digits[0] == b'0' {
        return Err(RecordError);
    }
    digits.iter().try_fold(0u64, |v, d| {
        if d.is_ascii_digit() {
            v.checked_mul(10)
                .and_then(|v| v.checked_add(u64::from(d - b'0')))
                .ok_or(RecordError)
        } else {
            Err(RecordError)
        }
    })
}
pub fn decode_limits(b: &[u8]) -> Result<(u64, Limits), RecordError> {
    let (tag, epoch, _) = header(b, false)?;
    if tag != 1 || b.len() != 80 {
        return Err(RecordError);
    }
    let b = &b[32..];
    let v = Limits {
        max_transfers: u32at(b, 4),
        max_payload_bytes: u32at(b, 8),
        journal_records: u32at(b, 16),
        journal_bytes: u32at(b, 20),
        assembly_timeout_ms: u32at(b, 24),
        ack_progress_timeout_ms: u32at(b, 28),
        max_retained_transfers: u32at(b, 36),
        broker_generation: u64at(b, 40),
    };
    if u16at(b, 0) != 1
        || u16at(b, 2) != 1
        || u16at(b, 12) != 255
        || u16at(b, 14) != 0
        || u32at(b, 32) != 344
        || !(1..=64).contains(&v.max_transfers)
        || !(1..=65536).contains(&v.max_payload_bytes)
        || !(4..=256).contains(&v.journal_records)
        || !(280..=32768).contains(&v.journal_bytes)
        || !(1..=12000).contains(&v.assembly_timeout_ms)
        || !(1..=2000).contains(&v.ack_progress_timeout_ms)
        || !(v.max_transfers..=4096).contains(&v.max_retained_transfers)
        || v.broker_generation == 0
    {
        return Err(RecordError);
    }
    Ok((epoch, v))
}
pub fn decode_event(bytes: &[u8]) -> Result<EventRecord, RecordError> {
    let (tag, epoch, sequence) = header(bytes, true)?;
    let b = &bytes[32..];
    let event = match (tag, b.len()) {
        (16, 16) if u16at(b, 0) == 1 && zero(&b[2..8]) && u64at(b, 8) == epoch => Event::Negotiated,
        (17, 8) if u16at(b, 0) == 1 && zero(&b[2..]) => Event::Refused,
        (18, 16) if u64at(b, 0) > 0 && (256..=260).contains(&u16at(b, 8)) && zero(&b[10..]) => {
            Event::Submitted {
                submission: u64at(b, 0),
                candidate_kind: u16at(b, 8),
            }
        }
        (32, 56) if b[8] == 1 && zero(&b[9..]) => Event::Denied {
            transfer: u64at(b, 0),
        },
        (32, 56) if b[8] == 2 && b[9] == 1 && zero(&b[12..16]) => {
            let g = Grant {
                transfer: u64at(b, 0),
                kind: kind(u16at(b, 10))?,
                source: u64at(b, 16),
                target: u64at(b, 24),
                source_generation: u64at(b, 32),
                broker_generation: u64at(b, 40),
                deadline_msec: u64at(b, 48),
            };
            if g.transfer == 0
                || g.source == 0
                || g.target == 0
                || g.source == g.target
                || g.source_generation == 0
                || g.broker_generation == 0
                || g.deadline_msec == 0
            {
                return Err(RecordError);
            }
            Event::Allowed(g)
        }
        (33, 16) if u64at(b, 0) > 0 && zero(&b[9..]) => Event::TransferOutcome {
            transfer: u64at(b, 0),
            outcome: match b[8] {
                2 => Outcome::Completed,
                3 => Outcome::Revoked,
                4 => Outcome::Expired,
                _ => return Err(RecordError),
            },
        },
        _ => return Err(RecordError),
    };
    Ok(EventRecord {
        epoch,
        sequence,
        event,
    })
}
/// Writes one complete immutable candidate; output is unchanged on failure.
/// Zero request ID/generation/deadline remain representable for lifecycle Denied.
/// An unsupported negotiation range is also representable for protocol Refused.
pub fn encode_candidate(
    epoch: u64,
    submission: u64,
    candidate: Candidate<'_>,
    out: &mut [u8],
) -> Result<usize, RecordError> {
    if epoch == 0 || submission == 0 {
        return Err(RecordError);
    }
    let (tag, length) = match candidate {
        Candidate::Negotiate { .. } => (256, 40),
        Candidate::Request(_) => (257, 344),
        Candidate::Begin { .. } => (258, 56),
        Candidate::End { .. } => (259, 48),
        Candidate::Cancel { .. } => (260, 40),
    };
    if out.len() < length {
        return Err(RecordError);
    }
    let mut record = [0; CANDIDATE_MAX];
    p32(&mut record, 0, length as u32);
    p16(&mut record, 4, 1);
    p16(&mut record, 6, tag);
    p64(&mut record, 8, epoch);
    p64(&mut record, 16, submission);
    let b = &mut record[32..];
    match candidate {
        Candidate::Negotiate { minimum, maximum } => {
            p16(b, 0, minimum);
            p16(b, 2, maximum);
        }
        Candidate::Request(r) => {
            if r.source == 0
                || r.target == 0
                || r.source == r.target
                || r.mime.is_some_and(|m| m.len() > 255)
            {
                return Err(RecordError);
            }
            p64(b, 0, r.transfer);
            p64(b, 8, r.source);
            p64(b, 16, r.target);
            p16(b, 24, r.kind as u16);
            if let Some(m) = r.mime {
                b[26] = 1;
                p16(b, 28, m.len() as u16);
                b[56..56 + m.len()].copy_from_slice(m.as_bytes());
            }
            p64(b, 32, r.byte_size);
            p64(b, 40, r.generation);
            p64(b, 48, r.deadline_msec);
        }
        Candidate::Begin {
            transfer,
            total_bytes,
        }
        | Candidate::End {
            transfer,
            total_bytes,
        } => {
            if transfer == 0 || total_bytes > 65536 {
                return Err(RecordError);
            }
            p64(b, 0, transfer);
            p64(b, if tag == 258 { 16 } else { 8 }, total_bytes);
        }
        Candidate::Cancel { transfer } => {
            if transfer == 0 {
                return Err(RecordError);
            }
            p64(b, 0, transfer);
        }
    }
    out[..length].copy_from_slice(&record[..length]);
    Ok(length)
}
pub fn encode_submit(
    epoch: u64,
    submission: u64,
    candidate_bytes: u32,
) -> Result<[u8; 24], RecordError> {
    if epoch == 0 || submission == 0 || !(40..=344).contains(&candidate_bytes) {
        return Err(RecordError);
    }
    let mut b = [0; 24];
    p64(&mut b, 0, epoch);
    p64(&mut b, 8, submission);
    p32(&mut b, 16, candidate_bytes);
    Ok(b)
}
pub fn encode_ack(epoch: u64, sequence: u64) -> Result<[u8; 16], RecordError> {
    if epoch == 0 || sequence == 0 {
        return Err(RecordError);
    }
    let mut b = [0; 16];
    p64(&mut b, 0, epoch);
    p64(&mut b, 8, sequence);
    Ok(b)
}
