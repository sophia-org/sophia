//! Internal D4 experiment framing. Not a stable SDK ABI or sealed source kind.
//! All records are length-prefixed, canonical little-endian u64 words.
//! Parsing proves shape only, never authorization, source custody or freshness.
//! Only the explicitly enabled experimental file may accept these records.
use crate::{OwnedCopyImageRequest, OwnedRingGeometry, OwnedSlotIdentity};
use sophia_protocol::{ContentAllocationId, ContentGrant, ContentOutputId, ContentResourceId};

pub const CONTROL_FILE: &[u8] = b"experimental-owned-ring-v1";
pub const EVENT_BYTES: usize = 120;
const CONTROL: &[u8; 4] = b"D4C1";
const SOURCE: &[u8; 4] = b"D4F1";
const EVENT: &[u8; 4] = b"D4E1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub grant: ContentGrant,
    pub channel: u64,
    pub ring: u64,
    pub generation: u64,
}
impl Identity {
    fn words(self, kind: u64) -> [u64; 6] {
        [
            kind,
            self.grant.connection_epoch,
            self.grant.content_grant_epoch,
            self.channel,
            self.ring,
            self.generation,
        ]
    }
    fn decode(words: &[u64]) -> Option<Self> {
        if words[1..6].contains(&0) {
            return None;
        }
        Some(Self {
            grant: ContentGrant {
                connection_epoch: words[1],
                content_grant_epoch: words[2],
            },
            channel: words[3],
            ring: words[4],
            generation: words[5],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Registration {
    pub identity: Identity,
    pub slots: usize,
    pub pixels: usize,
}
impl Registration {
    pub fn geometry(self) -> std::io::Result<OwnedRingGeometry> {
        OwnedRingGeometry::new(self.slots, self.pixels)
    }
    fn encode_with(self, magic: &[u8; 4]) -> Vec<u8> {
        let mut words = self.identity.words(1).to_vec();
        words.extend([self.slots as u64, self.pixels as u64]);
        encode(magic, &words)
    }
    pub fn encode(self) -> Vec<u8> {
        self.encode_with(CONTROL)
    }
    pub fn source_packet(self) -> Vec<u8> {
        self.encode_with(SOURCE)
    }
    pub fn decode_source(bytes: &[u8]) -> Option<Self> {
        Self::from_words(&decode::<8>(bytes, SOURCE)?)
    }
    fn from_words(words: &[u64]) -> Option<Self> {
        if words[0] != 1 {
            return None;
        }
        let value = Self {
            identity: Identity::decode(words)?,
            slots: words[6].try_into().ok()?,
            pixels: words[7].try_into().ok()?,
        };
        value.geometry().ok()?;
        Some(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Offer {
    pub identity: Identity,
    pub request: OwnedCopyImageRequest,
}
impl Offer {
    pub fn encode(self) -> Vec<u8> {
        let mut w = self.identity.words(2).to_vec();
        let r = self.request;
        w.extend([
            r.slot.offer,
            r.slot.slot as u64,
            r.slot.generation,
            r.resource.id,
            r.resource.generation,
            r.allocation.id,
            r.allocation.generation,
            r.output.id,
            r.output.generation,
            r.topology_generation,
            u64::from(r.width),
            u64::from(r.height),
        ]);
        encode(CONTROL, &w)
    }
    fn from_words(w: &[u64]) -> Option<Self> {
        if w[0] != 2
            || w[6] == 0
            || w[7] >= 4
            || w[8] == 0
            || !w[8].is_multiple_of(2)
            || w[9..18].contains(&0)
        {
            return None;
        }
        let pixels = w[16].checked_mul(w[17])?.checked_mul(4)?;
        OwnedRingGeometry::new(1, pixels.try_into().ok()?).ok()?;
        Some(Self {
            identity: Identity::decode(w)?,
            request: OwnedCopyImageRequest {
                slot: OwnedSlotIdentity {
                    offer: w[6],
                    slot: w[7].try_into().ok()?,
                    generation: w[8],
                },
                resource: ContentResourceId {
                    id: w[9],
                    generation: w[10],
                },
                allocation: ContentAllocationId {
                    id: w[11],
                    generation: w[12],
                },
                output: ContentOutputId {
                    id: w[13],
                    generation: w[14],
                },
                topology_generation: w[15],
                width: w[16].try_into().ok()?,
                height: w[17].try_into().ok()?,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Register(Registration),
    Offer(Offer),
}
impl Control {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() == 72 {
            Registration::from_words(&decode::<8>(bytes, CONTROL)?).map(Self::Register)
        } else {
            Offer::from_words(&decode::<18>(bytes, CONTROL)?).map(Self::Offer)
        }
    }
    pub fn identity(self) -> Identity {
        match self {
            Self::Register(v) => v.identity,
            Self::Offer(v) => v.identity,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u64)]
pub enum EventKind {
    RingAdmitted = 1,
    RingReady = 2,
    RingRefused = 3,
    SlotReleased = 4,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub identity: Identity,
    pub sequence: u64,
    pub slot: OwnedSlotIdentity,
    pub resource: ContentResourceId,
    pub reason: u16,
    /// 1 means generation mismatch; 0 is not a claim of pixel authentication.
    pub changed_generation: bool,
}
impl Event {
    pub fn ring(kind: EventKind, identity: Identity, reason: u16) -> Self {
        Self {
            kind,
            identity,
            sequence: 0,
            slot: OwnedSlotIdentity {
                offer: 0,
                slot: 0,
                generation: 0,
            },
            resource: ContentResourceId::default(),
            reason,
            changed_generation: false,
        }
    }
    pub fn encode(self) -> Vec<u8> {
        let mut w = self.identity.words(self.kind as u64).to_vec();
        w.extend([
            self.sequence,
            self.slot.offer,
            self.slot.slot as u64,
            self.slot.generation,
            self.resource.id,
            self.resource.generation,
            u64::from(self.reason),
            u64::from(self.changed_generation),
        ]);
        encode(EVENT, &w)
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let w = decode::<14>(bytes, EVENT)?;
        let kind = match w[0] {
            1 => EventKind::RingAdmitted,
            2 => EventKind::RingReady,
            3 => EventKind::RingRefused,
            4 => EventKind::SlotReleased,
            _ => return None,
        };
        if w[6] == 0 || w[13] > 1 {
            return None;
        }
        match kind {
            EventKind::RingAdmitted | EventKind::RingReady if w[12] != 0 => return None,
            EventKind::RingRefused if w[12] == 0 => return None,
            EventKind::SlotReleased if w[13] != 0 && w[12] == 0 => return None,
            _ => {}
        }
        if kind != EventKind::SlotReleased && (w[7..12].iter().any(|v| *v != 0) || w[13] != 0) {
            return None;
        }
        if kind == EventKind::SlotReleased
            && (w[7] == 0
                || w[8] >= 4
                || w[9] == 0
                || !w[9].is_multiple_of(2)
                || w[10..12].contains(&0))
        {
            return None;
        }
        Some(Self {
            kind,
            identity: Identity::decode(&w)?,
            sequence: w[6],
            slot: OwnedSlotIdentity {
                offer: w[7],
                slot: w[8].try_into().ok()?,
                generation: w[9],
            },
            resource: ContentResourceId {
                id: w[10],
                generation: w[11],
            },
            reason: w[12].try_into().ok()?,
            changed_generation: w[13] != 0,
        })
    }
}

pub fn is_source(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(SOURCE)
}
fn encode(magic: &[u8; 4], words: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + 8 * words.len());
    bytes.extend_from_slice(&((8 + 8 * words.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(magic);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}
fn decode<const N: usize>(bytes: &[u8], magic: &[u8; 4]) -> Option<[u64; N]> {
    if bytes.len() != 8 + N * 8
        || bytes[4..8] != magic[..]
        || u32::from_le_bytes(bytes[..4].try_into().ok()?) as usize != bytes.len()
    {
        return None;
    }
    Some(std::array::from_fn(|i| {
        u64::from_le_bytes(
            bytes[8 + i * 8..16 + i * 8]
                .try_into()
                .expect("fixed words"),
        )
    }))
}
