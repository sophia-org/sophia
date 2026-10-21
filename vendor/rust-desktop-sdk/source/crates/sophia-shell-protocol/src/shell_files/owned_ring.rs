//! Owned-ring version 1 records: `docs/sophia-owned-ring.md` and
//! `protocol/sophia-owned-ring-v1.kdl` are normative. Shell revision 10
//! CPU uploads only; lock never uses these records.
//!
//! Every record is a u32 total length, four magic bytes and little-endian
//! u64 words. Parsing proves shape only, never authorization, source
//! custody or freshness. Capacity is fixed by the single registration for
//! a connection; each offer names its exact `width * height * 4` bytes,
//! which admission (not this codec) checks against that capacity.

use super::ShellImageCodecError;

fn invalid(field: &'static str) -> ShellImageCodecError {
    ShellImageCodecError::Invalid(field)
}

fn nonzero(value: u64, field: &'static str) -> Result<u64, ShellImageCodecError> {
    if value == 0 {
        return Err(invalid(field));
    }
    Ok(value)
}

/// The writable shell 9P file carrying D4C1 control records.
pub const OWNED_RING_CONTROL_FILE: &[u8] = b"owned-ring-v1";
/// Per-slot header bytes before the pixels; the generation word is first.
pub const OWNED_RING_HEADER_BYTES: u64 = 4096;
/// Largest per-slot pixel capacity a registration may fix (4 MiB).
pub const OWNED_RING_MAX_CAPACITY_BYTES: u64 = 4 << 20;

const CONTROL_MAGIC: &[u8; 4] = b"D4C1";
const SOURCE_MAGIC: &[u8; 4] = b"D4F1";
const EVENT_MAGIC: &[u8; 4] = b"D4E1";

fn encode(magic: &[u8; 4], words: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + words.len() * 8);
    bytes.extend_from_slice(&u32::try_from(8 + words.len() * 8).unwrap().to_le_bytes());
    bytes.extend_from_slice(magic);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn words<const N: usize>(bytes: &[u8], magic: &[u8; 4]) -> Result<[u64; N], ShellImageCodecError> {
    let total = 8 + N * 8;
    if bytes.len() != total {
        return Err(invalid("length"));
    }
    let declared = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    if declared != total {
        return Err(invalid("total_bytes"));
    }
    if &bytes[4..8] != magic {
        return Err(invalid("magic"));
    }
    let mut out = [0u64; N];
    for (index, word) in out.iter_mut().enumerate() {
        *word = u64::from_le_bytes(bytes[8 + index * 8..16 + index * 8].try_into().unwrap());
    }
    Ok(out)
}

/// The identity every owned-ring record shares: the connection and grant
/// epochs, the image channel generation, and the ring within it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingIdentity {
    pub connection_epoch: u64,
    pub content_grant_epoch: u64,
    pub channel_generation: u64,
    pub ring: u64,
    pub generation: u64,
}

impl RingIdentity {
    fn words(self, kind: u64) -> [u64; 6] {
        [
            kind,
            self.connection_epoch,
            self.content_grant_epoch,
            self.channel_generation,
            self.ring,
            self.generation,
        ]
    }

    fn decode(words: &[u64]) -> Result<Self, ShellImageCodecError> {
        Ok(Self {
            connection_epoch: nonzero(words[1], "connection_epoch")?,
            content_grant_epoch: nonzero(words[2], "content_grant_epoch")?,
            channel_generation: nonzero(words[3], "channel_generation")?,
            ring: nonzero(words[4], "ring_id")?,
            generation: nonzero(words[5], "ring_generation")?,
        })
    }
}

/// The one D4C1 registration for a connection: slot count and the fixed
/// per-slot pixel capacity every later offer is bounded by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingRegistration {
    pub identity: RingIdentity,
    pub slots: u32,
    pub capacity: u64,
}

impl RingRegistration {
    fn validate(self) -> Result<Self, ShellImageCodecError> {
        if !(1..=4).contains(&self.slots) {
            return Err(invalid("slots"));
        }
        if self.capacity < 4
            || self.capacity > OWNED_RING_MAX_CAPACITY_BYTES
            || !self.capacity.is_multiple_of(4)
        {
            return Err(invalid("capacity"));
        }
        Ok(self)
    }

    fn from_words(words: &[u64]) -> Result<Self, ShellImageCodecError> {
        if words[0] != 1 {
            return Err(invalid("kind"));
        }
        Self {
            identity: RingIdentity::decode(words)?,
            slots: u32::try_from(words[6]).map_err(|_| invalid("slots"))?,
            capacity: words[7],
        }
        .validate()
    }

    /// The 72-byte D4C1 Register record for the control file.
    pub fn encode(self) -> Vec<u8> {
        let mut words = self.identity.words(1).to_vec();
        words.extend([u64::from(self.slots), self.capacity]);
        encode(CONTROL_MAGIC, &words)
    }

    /// The 72-byte D4F1 source packet for the image channel; its words are
    /// identical to the accepted registration's.
    pub fn source_packet(self) -> Vec<u8> {
        let mut words = self.identity.words(1).to_vec();
        words.extend([u64::from(self.slots), self.capacity]);
        encode(SOURCE_MAGIC, &words)
    }

    /// Decode a D4C1 Register record.
    pub fn decode(bytes: &[u8]) -> Result<Self, ShellImageCodecError> {
        Self::from_words(&words::<8>(bytes, CONTROL_MAGIC)?)
    }

    /// Decode a D4F1 source packet.
    pub fn decode_source(bytes: &[u8]) -> Result<Self, ShellImageCodecError> {
        Self::from_words(&words::<8>(bytes, SOURCE_MAGIC)?)
    }

    /// Bytes one slot occupies: header plus capacity rounded up to it.
    pub fn slot_extent(self) -> u64 {
        OWNED_RING_HEADER_BYTES + self.capacity.next_multiple_of(OWNED_RING_HEADER_BYTES)
    }

    /// The exact source file size, or `None` on overflow.
    pub fn source_bytes(self) -> Option<u64> {
        self.slot_extent().checked_mul(u64::from(self.slots))
    }
}

/// One D4C1 slot offer: the registered identity plus the complete
/// resource, allocation, output and topology identities and dimensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingOffer {
    pub identity: RingIdentity,
    pub offer_id: u64,
    pub slot: u32,
    /// Even, nonzero, strictly increasing per slot; equals the slot's
    /// generation word before and after the Session's read.
    pub slot_generation: u64,
    pub resource: u64,
    pub resource_generation: u64,
    pub allocation: u64,
    pub allocation_generation: u64,
    pub output: u64,
    pub output_generation: u64,
    pub topology_generation: u64,
    pub width: u32,
    pub height: u32,
}

impl RingOffer {
    /// The exact resource extent, `width * height * 4`, when it fits.
    pub fn resource_bytes(self) -> Option<u64> {
        u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|bytes| bytes.checked_mul(4))
    }

    /// The 152-byte D4C1 Offer record for the control file.
    pub fn encode(self) -> Vec<u8> {
        let mut words = self.identity.words(2).to_vec();
        words.extend([
            self.offer_id,
            u64::from(self.slot),
            self.slot_generation,
            self.resource,
            self.resource_generation,
            self.allocation,
            self.allocation_generation,
            self.output,
            self.output_generation,
            self.topology_generation,
            u64::from(self.width),
            u64::from(self.height),
        ]);
        encode(CONTROL_MAGIC, &words)
    }

    fn from_words(words: &[u64]) -> Result<Self, ShellImageCodecError> {
        if words[0] != 2 {
            return Err(invalid("kind"));
        }
        let identity = RingIdentity::decode(words)?;
        let slot = u32::try_from(words[7]).map_err(|_| invalid("slot"))?;
        if slot > 3 {
            return Err(invalid("slot"));
        }
        let slot_generation = nonzero(words[8], "slot_generation")?;
        if !slot_generation.is_multiple_of(2) {
            return Err(invalid("slot_generation"));
        }
        Ok(Self {
            identity,
            offer_id: nonzero(words[6], "offer_id")?,
            slot,
            slot_generation,
            resource: nonzero(words[9], "resource_id")?,
            resource_generation: nonzero(words[10], "resource_generation")?,
            allocation: nonzero(words[11], "allocation_id")?,
            allocation_generation: nonzero(words[12], "allocation_generation")?,
            output: nonzero(words[13], "output_id")?,
            output_generation: nonzero(words[14], "output_generation")?,
            topology_generation: nonzero(words[15], "topology_generation")?,
            width: u32::try_from(nonzero(words[16], "width")?).map_err(|_| invalid("width"))?,
            height: u32::try_from(nonzero(words[17], "height")?).map_err(|_| invalid("height"))?,
        })
    }
}

/// One decoded D4C1 control record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RingControl {
    Register(RingRegistration),
    Offer(RingOffer),
}

impl RingControl {
    pub fn decode(bytes: &[u8]) -> Result<Self, ShellImageCodecError> {
        match bytes.len() {
            72 => Ok(Self::Register(RingRegistration::decode(bytes)?)),
            152 => Ok(Self::Offer(RingOffer::from_words(&words::<18>(
                bytes,
                CONTROL_MAGIC,
            )?)?)),
            _ => Err(invalid("length")),
        }
    }

    pub fn identity(self) -> RingIdentity {
        match self {
            Self::Register(registration) => registration.identity,
            Self::Offer(offer) => offer.identity,
        }
    }
}

/// The four D4E1 journal event kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RingEventKind {
    RingAdmitted = 1,
    RingReady = 2,
    RingRefused = 3,
    SlotReleased = 4,
}

/// One 120-byte D4E1 journal event. For `RingAdmitted`, `RingReady` and
/// `RingRefused` the offer, slot and resource words are zero; a
/// `SlotReleased` names its exact offer, slot, generation and resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingEvent {
    pub kind: RingEventKind,
    pub identity: RingIdentity,
    pub sequence: u64,
    pub offer_id: u64,
    pub slot: u32,
    pub slot_generation: u64,
    pub resource: u64,
    pub resource_generation: u64,
    pub reason: u16,
    /// Generation mismatch marker; set only on `SlotReleased` with a
    /// nonzero reason. Never a claim of authenticated pixels.
    pub changed_generation: bool,
}

impl RingEvent {
    pub fn encode(self) -> Vec<u8> {
        let mut words = self.identity.words(self.kind as u64).to_vec();
        words.extend([
            self.sequence,
            self.offer_id,
            u64::from(self.slot),
            self.slot_generation,
            self.resource,
            self.resource_generation,
            u64::from(self.reason),
            u64::from(self.changed_generation),
        ]);
        encode(EVENT_MAGIC, &words)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ShellImageCodecError> {
        let words = words::<14>(bytes, EVENT_MAGIC)?;
        let kind = match words[0] {
            1 => RingEventKind::RingAdmitted,
            2 => RingEventKind::RingReady,
            3 => RingEventKind::RingRefused,
            4 => RingEventKind::SlotReleased,
            _ => return Err(invalid("kind")),
        };
        let identity = RingIdentity::decode(&words)?;
        let sequence = nonzero(words[6], "sequence")?;
        if words[12] > u64::from(u16::MAX) {
            return Err(invalid("reason"));
        }
        let reason = words[12] as u16;
        if words[13] > 1 {
            return Err(invalid("changed_generation"));
        }
        let changed_generation = words[13] != 0;
        if kind != RingEventKind::SlotReleased
            && (words[7..12].iter().any(|word| *word != 0) || changed_generation)
        {
            return Err(invalid("reserved"));
        }
        match kind {
            RingEventKind::RingAdmitted | RingEventKind::RingReady if reason != 0 => {
                return Err(invalid("reason"));
            }
            RingEventKind::RingRefused if reason == 0 => {
                return Err(invalid("reason"));
            }
            _ => {}
        }
        if kind == RingEventKind::SlotReleased {
            nonzero(words[7], "offer_id")?;
            if words[8] > 3 {
                return Err(invalid("slot"));
            }
            let generation = nonzero(words[9], "slot_generation")?;
            if !generation.is_multiple_of(2) {
                return Err(invalid("slot_generation"));
            }
            nonzero(words[10], "resource_id")?;
            nonzero(words[11], "resource_generation")?;
            if changed_generation && reason == 0 {
                return Err(invalid("reason"));
            }
        }
        Ok(Self {
            kind,
            identity,
            sequence,
            offer_id: words[7],
            slot: words[8] as u32,
            slot_generation: words[9],
            resource: words[10],
            resource_generation: words[11],
            reason,
            changed_generation,
        })
    }
}
