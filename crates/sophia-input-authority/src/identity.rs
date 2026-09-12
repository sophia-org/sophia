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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct AuthorityUid(u64);

impl AuthorityUid {
    /// Allocate the next authority identity, or refuse.
    ///
    /// Wrapping would hand a new authority an identity a live one still uses,
    /// so every handle and capability minted against the old one would start
    /// validating against the new. Exhaustion stops construction instead.
    pub(crate) fn allocate() -> Option<Self> {
        let raw = NEXT_AUTHORITY.fetch_add(1, Ordering::Relaxed);
        if raw == u64::MAX {
            return None;
        }
        Some(Self(raw))
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
    pub(crate) authority: AuthorityUid,
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
    /// A slot no real input occupies, for initialising fixed scratch.
    pub(crate) const PLACEHOLDER: Self = Self { slot: u16::MAX };

    /// The lowest X keycode. Below this is not a key at all.
    pub const MIN_KEYCODE: u8 = 8;

    /// How many key slots the protocol's keycode range occupies.
    pub(crate) const KEY_SLOTS: u16 = (u8::MAX as u16) - (Self::MIN_KEYCODE as u16) + 1;

    /// A key, normalized so keys and buttons cannot share a slot.
    pub fn key(keycode: u8) -> Result<Self, InputError> {
        if keycode < Self::MIN_KEYCODE {
            return Err(InputError::KeycodeBelowMinimum(keycode));
        }
        Ok(Self {
            slot: u16::from(keycode - Self::MIN_KEYCODE),
        })
    }

    /// A button, checked against a domain and placed above every key.
    ///
    /// The domain belongs to the authority, which verified it against the
    /// advertised value at construction. Passing one here is for callers that
    /// already hold it; `AuthorityInstance::button` is the checked entry.
    pub fn button(button: u8, domain: u8) -> Result<Self, InputError> {
        if button == 0 || button > domain {
            return Err(InputError::ButtonOutsideDomain { button, domain });
        }
        let slot = Self::KEY_SLOTS
            .checked_add(u16::from(button - 1))
            .ok_or(InputError::ButtonOutsideDomain { button, domain })?;
        Ok(Self { slot })
    }

    pub(crate) fn slot(self) -> usize {
        usize::from(self.slot)
    }
}

/// Where a press is being delivered, as the router knows it.
///
/// This is all a caller supplies. It deliberately carries no hold identity:
/// accepting one would let a caller hand back an old, settled identity on a new
/// press, and the old completion would then clear the new debt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Recipient {
    pub recipient: u64,
    pub connection_generation: u64,
}

/// Which hold a release belongs to.
///
/// The `hold` field is minted inside the authority from a checked counter and
/// cannot be constructed outside it, so two holds are never the same identity
/// and a stale completion never matches a live one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HoldIncarnation {
    pub recipient: u64,
    pub connection_generation: u64,
    pub input: Input,
    pub(crate) hold: u64,
}

impl HoldIncarnation {
    /// The minted identity, for records that must match it back.
    pub fn hold(self) -> u64 {
        self.hold
    }
}
