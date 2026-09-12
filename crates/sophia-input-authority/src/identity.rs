//! Identities that cannot be forged by naming them.
//!
//! Every type here is opaque on purpose. `DeviceId` exists in the protocol and
//! anyone can build one, so it is a lookup key and never evidence. The
//! capability below is the evidence, and only an issuer mints one.

use sophia_protocol::{DeviceId, SeatId};
use std::sync::atomic::{AtomicU64, Ordering};

/// Distinguishes one live authority from another with the same seat binding.
///
/// Allocated from a process counter, never from a caller-supplied value. Two
/// authorities constructed for the same instance and seat still differ here, so
/// a handle from one cannot be used on the other. Without this, a caller could
/// build a second authority with the same public binding and use its issuer.
static NEXT_AUTHORITY: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityUid(u64);

impl AuthorityUid {
    pub(crate) fn allocate() -> Self {
        Self(NEXT_AUTHORITY.fetch_add(1, Ordering::Relaxed))
    }
}

/// Which seat, in which Sophia instance, an authority speaks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeatBinding {
    instance: InstanceId,
    seat: SeatId,
}

impl SeatBinding {
    pub fn new(instance: InstanceId, seat: SeatId) -> Self {
        Self { instance, seat }
    }

    pub fn instance(self) -> InstanceId {
        self.instance
    }

    pub fn seat(self) -> SeatId {
        self.seat
    }
}

/// Which private instance this authority belongs to.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InstanceId(u64);

impl InstanceId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }
}

/// Where a source's events come from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    Physical,
    Synthetic,
}

/// A registered source.
///
/// Physical and synthetic sources are counted in separate tables, so an
/// injector cannot consume a slot a device needs and the two can never collide
/// in the holder set.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceId {
    pub(crate) synthetic: bool,
    pub(crate) index: u16,
}

impl SourceId {
    /// Position in the holder bitset. Synthetic sources occupy the low half,
    /// physical the high half, with no arithmetic that can wrap one onto the
    /// other.
    pub(crate) fn holder_bit(self, synthetic_capacity: usize) -> usize {
        if self.synthetic {
            usize::from(self.index)
        } else {
            synthetic_capacity + usize::from(self.index)
        }
    }
}

/// Permission to emit from one registered device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceCapability {
    pub(crate) source: SourceId,
    pub(crate) binding: SeatBinding,
    pub(crate) authority: AuthorityUid,
    pub(crate) grant: crate::GrantId,
    pub(crate) generation: crate::GrantGeneration,
    pub(crate) device: DeviceId,
}

impl DeviceCapability {
    /// The packet key for this device. Not authority.
    pub fn device_id(self) -> DeviceId {
        self.device
    }

    /// The source this capability speaks for. Names a source, grants nothing.
    pub fn source(self) -> SourceId {
        self.source
    }
}

/// A key or a button, validated into the advertised domain.
///
/// Constructed only through the checked builders, because the raw numbers
/// overlap: X keycodes run 8..=255 and buttons 1..=9, so a key and a button
/// both land near 249 if either is used unnormalized.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Input {
    slot: u16,
}

/// Why an input was outside the advertised domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputError {
    /// X keycodes below 8 are not keys.
    KeycodeBelowMinimum(u8),
    /// Button outside the advertised pointer domain.
    ButtonOutsideDomain { button: u8, domain: u8 },
}

impl Input {
    /// The lowest X keycode. Below this is not a key at all.
    pub const MIN_KEYCODE: u8 = 8;

    /// A key, normalized so keys and buttons cannot share a slot.
    pub fn key(keycode: u8) -> Result<Self, InputError> {
        if keycode < Self::MIN_KEYCODE {
            return Err(InputError::KeycodeBelowMinimum(keycode));
        }
        Ok(Self {
            slot: u16::from(keycode - Self::MIN_KEYCODE),
        })
    }

    /// A button, checked against the advertised domain and placed above keys.
    pub fn button(button: u8, domain: u8) -> Result<Self, InputError> {
        if button == 0 || button > domain {
            return Err(InputError::ButtonOutsideDomain { button, domain });
        }
        let keys = u16::from(u8::MAX - Self::MIN_KEYCODE) + 1;
        Ok(Self {
            slot: keys + u16::from(button - 1),
        })
    }

    pub(crate) fn slot(self) -> usize {
        usize::from(self.slot)
    }
}

/// Which hold a release belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HoldIncarnation {
    pub recipient: u64,
    pub connection_generation: u64,
    pub input: Input,
    pub hold: u64,
}
