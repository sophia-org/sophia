//! Whole portal records: a validated envelope around one typed body.
//! Decoders take the connection's admitted epoch, so the export refuses a
//! stale record (ESTALE) apart from a malformed one (EINVAL).

use super::bodies::{decode_negotiated, decode_refused, encode_negotiated, encode_refused};
use super::envelope::{decode_record, encode_record};
use super::{
    PortalFileClass, PortalFileError, PortalFileHeader, PortalFileKind, PortalFileLimits,
    PortalNegotiate, PortalPayloadBegin, PortalPayloadEnd, PortalSubmitted, PortalTransferOutcome,
    decode_portal_decision, decode_portal_transfer_cancel, decode_portal_transfer_request,
    encode_portal_decision, encode_portal_transfer_cancel, encode_portal_transfer_request, invalid,
};
use crate::{BinaryCodecError, PortalBrokerResponsePacket, PortalRequest, PortalTransferId};

/// One staged requester candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalFileCandidate {
    Negotiate(PortalNegotiate),
    /// Always Pending; the owner derives permissions and the decision.
    TransferRequest(PortalRequest),
    PayloadBegin(PortalPayloadBegin),
    PayloadEnd(PortalPayloadEnd),
    TransferCancel(PortalTransferId),
}

impl PortalFileCandidate {
    /// The kind a Submitted receipt names for this candidate.
    pub const fn kind(&self) -> PortalFileKind {
        match self {
            Self::Negotiate(_) => PortalFileKind::Negotiate,
            Self::TransferRequest(_) => PortalFileKind::TransferRequest,
            Self::PayloadBegin(_) => PortalFileKind::PayloadBegin,
            Self::PayloadEnd(_) => PortalFileKind::PayloadEnd,
            Self::TransferCancel(_) => PortalFileKind::TransferCancel,
        }
    }

    fn encode_body(&self) -> Result<Vec<u8>, BinaryCodecError> {
        match self {
            Self::Negotiate(negotiate) => Ok(negotiate.encode()),
            Self::TransferRequest(request) => encode_portal_transfer_request(request),
            Self::PayloadBegin(begin) => begin.encode(),
            Self::PayloadEnd(end) => end.encode(),
            Self::TransferCancel(transfer) => encode_portal_transfer_cancel(*transfer),
        }
    }

    fn decode_body(kind: PortalFileKind, body: &[u8]) -> Result<Self, BinaryCodecError> {
        Ok(match kind {
            PortalFileKind::Negotiate => Self::Negotiate(PortalNegotiate::decode(body)?),
            PortalFileKind::TransferRequest => {
                Self::TransferRequest(decode_portal_transfer_request(body)?)
            }
            PortalFileKind::PayloadBegin => Self::PayloadBegin(PortalPayloadBegin::decode(body)?),
            PortalFileKind::PayloadEnd => Self::PayloadEnd(PortalPayloadEnd::decode(body)?),
            PortalFileKind::TransferCancel => {
                Self::TransferCancel(decode_portal_transfer_cancel(body)?)
            }
            _ => return Err(invalid("record_class")),
        })
    }
}

/// One journal event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalFileEvent {
    /// Revision 1 selected for the record's own connection epoch.
    Negotiated,
    /// An invalid or unsupported revision range.
    Refused,
    Submitted(PortalSubmitted),
    Decision(PortalBrokerResponsePacket),
    TransferOutcome(PortalTransferOutcome),
}

impl PortalFileEvent {
    pub const fn kind(&self) -> PortalFileKind {
        match self {
            Self::Negotiated => PortalFileKind::Negotiated,
            Self::Refused => PortalFileKind::Refused,
            Self::Submitted(_) => PortalFileKind::Submitted,
            Self::Decision(_) => PortalFileKind::Decision,
            Self::TransferOutcome(_) => PortalFileKind::TransferOutcome,
        }
    }

    fn encode_body(&self, epoch: u64) -> Result<Vec<u8>, BinaryCodecError> {
        match self {
            Self::Negotiated => encode_negotiated(epoch),
            Self::Refused => Ok(encode_refused()),
            Self::Submitted(submitted) => submitted.encode(),
            Self::Decision(decision) => encode_portal_decision(decision),
            Self::TransferOutcome(outcome) => outcome.encode(),
        }
    }

    fn decode_body(
        kind: PortalFileKind,
        body: &[u8],
        epoch: u64,
    ) -> Result<Self, BinaryCodecError> {
        Ok(match kind {
            PortalFileKind::Negotiated => {
                // The body confirms the very epoch its header carries.
                if decode_negotiated(body)? != epoch {
                    return Err(invalid("connection_epoch"));
                }
                Self::Negotiated
            }
            PortalFileKind::Refused => {
                decode_refused(body)?;
                Self::Refused
            }
            PortalFileKind::Submitted => Self::Submitted(PortalSubmitted::decode(body)?),
            PortalFileKind::Decision => Self::Decision(decode_portal_decision(body)?),
            PortalFileKind::TransferOutcome => {
                Self::TransferOutcome(PortalTransferOutcome::decode(body)?)
            }
            _ => return Err(invalid("record_class")),
        })
    }
}

/// A staged `transaction` candidate for the admitted `epoch`.
pub fn decode_portal_file_candidate(
    bytes: &[u8],
    epoch: u64,
) -> Result<(PortalFileHeader, PortalFileCandidate), PortalFileError> {
    let (header, body) = decode_record(bytes, PortalFileClass::Candidate, epoch)?;
    Ok((header, PortalFileCandidate::decode_body(header.kind, body)?))
}

pub fn encode_portal_file_candidate(
    epoch: u64,
    submission_id: u64,
    candidate: &PortalFileCandidate,
) -> Result<Vec<u8>, BinaryCodecError> {
    let header = PortalFileHeader {
        kind: candidate.kind(),
        connection_epoch: epoch,
        submission_id,
        sequence: 0,
    };
    encode_record(header, &candidate.encode_body()?)
}

/// One `events` record for the admitted `epoch`.
pub fn decode_portal_file_event(
    bytes: &[u8],
    epoch: u64,
) -> Result<(PortalFileHeader, PortalFileEvent), PortalFileError> {
    let (header, body) = decode_record(bytes, PortalFileClass::Event, epoch)?;
    Ok((
        header,
        PortalFileEvent::decode_body(header.kind, body, epoch)?,
    ))
}

pub fn encode_portal_file_event(
    epoch: u64,
    sequence: u64,
    event: &PortalFileEvent,
) -> Result<Vec<u8>, BinaryCodecError> {
    let header = PortalFileHeader {
        kind: event.kind(),
        connection_epoch: epoch,
        submission_id: 0,
        sequence,
    };
    encode_record(header, &event.encode_body(epoch)?)
}

/// The `limits` object for the admitted `epoch`.
pub fn decode_portal_file_limits(
    bytes: &[u8],
    epoch: u64,
) -> Result<PortalFileLimits, PortalFileError> {
    let (_, body) = decode_record(bytes, PortalFileClass::Object, epoch)?;
    Ok(PortalFileLimits::decode(body)?)
}

pub fn encode_portal_file_limits(
    epoch: u64,
    limits: &PortalFileLimits,
) -> Result<Vec<u8>, BinaryCodecError> {
    let header = PortalFileHeader {
        kind: PortalFileKind::Limits,
        connection_epoch: epoch,
        submission_id: 0,
        sequence: 0,
    };
    encode_record(header, &limits.encode()?)
}
