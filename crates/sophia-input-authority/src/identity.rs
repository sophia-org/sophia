//! Identities that cannot be forged by naming them.
//!
//! Every type here is opaque on purpose. `DeviceId` exists in the protocol and
//! is constructible by anyone, so it is a lookup key for building a packet and
//! never evidence that the caller may emit from that device. The capability
//! below is that evidence, and it is only ever handed out by the issuer.

use sophia_protocol::{DeviceId, SeatId};

/// Which seat, in which Sophia instance, an authority speaks for.
///
/// Bound once at construction. A caller cannot select another authority by
/// naming a different seat in a request, because requests carry no seat: they
/// carry a capability, and a capability belongs to exactly one binding.
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
///
/// Not a flag alongside an event: a property of the registered source, so that
/// asking whether an event is synthetic is asking which device emitted it. The
/// emergency recognizer consumes `Physical` sources only, which is a predicate
/// over this rather than a filter someone can forget to apply per event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    Physical,
    Synthetic,
}

/// Internal source identity. Never leaves the crate as a constructible value.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceId(u32);

impl SourceId {
    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> u32 {
        self.0
    }
}

/// Permission to emit from one registered device.
///
/// Handed to an adapter by the issuer. Carries its binding and its owning
/// grant generation, so a capability from one instance, seat or grant cannot
/// be replayed into another: validation compares what the capability claims
/// against what the authority is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceCapability {
    pub(crate) source: SourceId,
    pub(crate) binding: SeatBinding,
    pub(crate) grant: crate::GrantId,
    pub(crate) generation: crate::GrantGeneration,
    pub(crate) device: DeviceId,
}

impl DeviceCapability {
    /// The packet key for this device. Not authority: building a packet is not
    /// permission to deliver one.
    pub fn device_id(self) -> DeviceId {
        self.device
    }

    /// The registered source this capability speaks for.
    ///
    /// Safe to hand out: a `SourceId` names a source but grants nothing, and
    /// every mutating call either takes the capability itself or the issuer.
    pub fn source(self) -> SourceId {
        self.source
    }
}

/// A key or a button, in one flat space so the ledger has one key type.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Input {
    Key(u8),
    Button(u8),
}

/// Which hold a release belongs to.
///
/// Recipient and connection generation are part of the identity because a
/// release owed to one recipient must not be suppressed by a newer hold at
/// another, and must not clear a newer hold at the same one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HoldIncarnation {
    pub recipient: u64,
    pub connection_generation: u64,
    pub input: Input,
    pub hold: u64,
}
