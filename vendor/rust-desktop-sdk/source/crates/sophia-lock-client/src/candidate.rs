use sophia_lock_protocol::{BinaryCodecError, lock_files::*};

/// Typed candidate bodies. Upload lifecycle and admission checks belong to
/// the developing session; constructing a value grants no resource authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Candidate {
    Negotiate(LockNegotiate),
    ResourceBegin(LockResourceBegin),
    ResourceEnd(LockResourceStep),
    ResourceCancel(LockResourceStep),
    ResourceRetire(LockResourceStep),
    Present(LockCandidate),
    FrameDemand(LockFrameDemand),
}

/// Bounded immutable bytes owned by one submission. Moving this value copies
/// at most 128 bytes; it owns no heap storage and borrows no caller data.
pub struct Prepared {
    bytes: [u8; LOCK_FILE_MAX_CANDIDATE_BYTES],
    len: usize,
    kind: LockFileKind,
}
impl Prepared {
    pub(crate) fn image(
        epoch: u64,
        submission: u64,
        body: &[u8; 96],
    ) -> Result<Self, BinaryCodecError> {
        LockImageMetadata::decode(body)?;
        let encoded = encode_lock_file_record_at_revision(
            LockFileHeader {
                kind: LockFileKind::ImageBegin,
                connection_epoch: epoch,
                submission_id: submission,
                sequence: 0,
            },
            body,
            2,
        )?;
        let mut bytes = [0; LOCK_FILE_MAX_CANDIDATE_BYTES];
        bytes[..encoded.len()].copy_from_slice(&encoded);
        Ok(Self {
            bytes,
            len: encoded.len(),
            kind: LockFileKind::ImageBegin,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    pub fn kind(&self) -> LockFileKind {
        self.kind
    }
}
impl Candidate {
    pub fn prepare(&self, epoch: u64, submission: u64) -> Result<Prepared, BinaryCodecError> {
        if epoch == 0 || submission == 0 {
            return Err(BinaryCodecError::InvalidRecord("candidate_identity"));
        }
        let (kind, body) = match self {
            Self::Negotiate(offer) => {
                let caps = offer.requested_capabilities;
                if offer.minimum_revision == 0
                    || offer.minimum_revision > offer.maximum_revision
                    || caps & LOCK_FILE_CAPABILITY_PRESENT == 0
                    || caps
                        & !(LOCK_FILE_CAPABILITY_PRESENT
                            | LOCK_FILE_CAPABILITY_CHORDS
                            | LOCK_FILE_CAPABILITY_SEALED_MEMORY)
                        != 0
                    || (caps & LOCK_FILE_CAPABILITY_SEALED_MEMORY != 0
                        && offer.maximum_revision < 2)
                    || (!offer.chords.is_empty() && caps & LOCK_FILE_CAPABILITY_CHORDS == 0)
                {
                    return Err(BinaryCodecError::InvalidRecord("negotiation_offer"));
                }
                (LockFileKind::Negotiate, offer.encode()?)
            }
            Self::ResourceBegin(v) => (LockFileKind::ResourceBegin, v.encode()?),
            Self::ResourceEnd(v) => {
                if v.total_bytes.is_none() {
                    return Err(BinaryCodecError::InvalidRecord("end_total"));
                }
                (LockFileKind::ResourceEnd, v.encode()?)
            }
            Self::ResourceCancel(v) | Self::ResourceRetire(v) => {
                if v.total_bytes.is_some() {
                    return Err(BinaryCodecError::InvalidRecord("unexpected_total"));
                }
                (
                    if matches!(self, Self::ResourceCancel(_)) {
                        LockFileKind::ResourceCancel
                    } else {
                        LockFileKind::ResourceRetire
                    },
                    v.encode()?,
                )
            }
            Self::Present(v) => (LockFileKind::Candidate, v.encode()?),
            Self::FrameDemand(v) => (LockFileKind::FrameDemand, v.encode()?),
        };
        let len = LOCK_FILE_HEADER_BYTES + body.len();
        if len > LOCK_FILE_MAX_CANDIDATE_BYTES {
            return Err(BinaryCodecError::InvalidRecord("candidate_size"));
        }
        // Write the fixed envelope directly, avoiding a second temporary Vec.
        // Body codecs above remain the single source of value validation.
        let mut bytes = [0; LOCK_FILE_MAX_CANDIDATE_BYTES];
        bytes[..4].copy_from_slice(&(len as u32).to_le_bytes());
        bytes[4..6].copy_from_slice(&LOCK_FILE_API_VERSION.to_le_bytes());
        bytes[6..8].copy_from_slice(&(kind as u16).to_le_bytes());
        bytes[8..16].copy_from_slice(&epoch.to_le_bytes());
        bytes[16..24].copy_from_slice(&submission.to_le_bytes());
        bytes[32..len].copy_from_slice(&body);
        Ok(Prepared { bytes, len, kind })
    }
}
