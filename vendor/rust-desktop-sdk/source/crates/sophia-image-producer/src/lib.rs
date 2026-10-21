//! Clockless ISB (Immutable Shared Buffers) image producer for one Sophia image channel.
//!
//! The producer serves one role connection: it consumes one discovery
//! set, owns the SEQPACKET side socket after attach, sends `Hello`,
//! waits for `ChannelReady`, begins up to four offers, and sends an
//! admitted offer's sealed source exactly once. It never uses 9P: the
//! role client reads the discovery objects, submits the `ImageBegin`
//! body this crate returns under its own header, and forwards image
//! records and `ResourceStatus` outcomes back in.
//!
//! All time is caller-supplied monotonic milliseconds; a `now_ms`
//! smaller than a previous call is an argument error. The producer
//! never replays a send and never invents a release: only
//! `SourceReleased`, a definitive refusal before admission, or journal
//! loss ends an offer's custody. After admission, only release or journal loss
//! settles custody. DMA-BUF sources are refused
//! at `begin` even when granted; this crate is the sealed CPU plane.

mod io;
mod isb;
mod offers;
mod producer;
mod ring;
mod role;

pub use io::connect;
pub use isb::{IsbSource, create_isb, create_mapped_isb, finish_isb};
pub use producer::ImageProducer;
pub use ring::{
    RingOfferInfo, RingOfferState, RingProducer, RingProducerConfig, RingSlotFill, RingUpload,
};
pub use sophia_isb::{IsbError, MappedIsb};

/// The role this producer speaks on its channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRole {
    Shell,
    Lock,
}

/// Call outcomes, mirroring the capability results of the C producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProducerError {
    /// Not now: retry after the next dispatch or pass.
    Again,
    /// Every offer slot is live, or the role offer bound is reached.
    Busy,
    /// The channel is not usable; see `cause`.
    Closed,
    /// Offer IDs are exhausted: a new 9P connection is needed.
    Exhausted,
    /// A value, source or state fails a check; nothing changed.
    Refused,
    /// An argument is invalid, including a backwards clock.
    Argument,
}

/// Channel barrier: discovery, attach, `Hello`, `ChannelReady`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelState {
    Idle,
    Discovered,
    HelloQueued,
    HelloSent,
    Ready,
    Closed,
}

/// Why the channel closed. Every closure ends image admission for this epoch;
/// recovery needs a new authenticated connection, never a byte fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseCause {
    None,
    Absent,
    Unavailable,
    Exhausted,
    Discovery,
    Timeout,
    Fault,
    JournalLost,
    Caller,
}

impl CloseCause {
    fn is_clean(self) -> bool {
        matches!(self, Self::Absent | Self::Unavailable | Self::Exhausted)
    }
}

/// One offer's custody state. No state ever sends twice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfferState {
    Begun,
    Refused,
    Admitted,
    Sent,
    Ambiguous,
    Expired,
    Settled,
    Unknown,
}

/// Which path the caller should use for its next image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImagePath {
    Descriptor,
    Wait,
    Error,
}

/// `ResourceStatus` outcomes as the role client reports them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceStatusKind {
    Accepted,
    Rejected,
    Cancelled,
}

/// Channel closure reasons carried by a `ChannelClosed` record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageClosedReason {
    PeerDisconnect,
    ProtocolFault,
    HelloTimeout,
    GrantRevoked,
}

/// Producer configuration. `granted_kinds` is the source-kind mask
/// from negotiated capabilities; discovery may withhold a granted
/// kind but never name an ungranted one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageProducerConfig {
    pub role: ImageRole,
    /// Negotiated role-file revision: exactly shell 9 or lock 2.
    pub revision: u16,
    pub connection_epoch: u64,
    pub granted_kinds: u32,
    pub bounds: ProducerRoleBounds,
    /// The role's transfer deadline; nonzero.
    pub transfer_timeout_ms: u32,
    /// Absolute monotonic deadline: discovery through `ChannelReady`.
    pub startup_deadline_ms: u64,
}

/// The role's own bounds from its negotiated limits, caller-supplied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProducerRoleBounds {
    pub max_resource_bytes: u64,
    pub max_width: u32,
    pub max_height: u32,
    pub max_admitted_outputs: u32,
    pub transfer_slots: u32,
}

/// The 96-byte image metadata, role-neutral. Fields match the shell
/// and lock metadata layouts exactly; the producer converts to the
/// role codec only to validate and encode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProducerMetadata {
    pub offer_id: u64,
    pub resource: u64,
    pub resource_generation: u64,
    pub admission_epoch: u64,
    pub output_id: u64,
    pub allocation_id: u64,
    pub topology_generation: u64,
    pub source_kind: u32,
    pub source_format: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub plane_count: u16,
    pub descriptor_count: u16,
    pub declared_bytes: u64,
    pub modifier: u64,
}

/// One image record from the role client's journal, converted by the
/// caller from its role decoding at the negotiated revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageEvent {
    ChannelReady {
        channel_generation: u64,
        granted_source_kinds: u32,
    },
    ChannelClosed {
        channel_generation: u64,
        reason: ImageClosedReason,
    },
    ImageAdmitted {
        metadata: ProducerMetadata,
    },
    SourceReleased {
        offer_id: u64,
        resource: u64,
        resource_generation: u64,
        /// Role disposition value, 1..=5.
        disposition: u32,
    },
    /// One D4E1 owned-ring journal event (shell revision 10 only).
    Ring(sophia_shell_protocol::shell_files::RingEvent),
}

/// The read outcome for one discovery object set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryRead<'a> {
    /// All three objects were read; bodies exclude record headers,
    /// whose connection epochs are supplied separately.
    Present {
        text: &'a [u8],
        limits_body: &'a [u8],
        formats_body: &'a [u8],
        limits_connection_epoch: u64,
        formats_connection_epoch: u64,
    },
    /// The image-channel object does not exist (ENOENT).
    Absent,
    /// A read failed for any other reason, or an object was denied.
    Failed,
}

/// A snapshot of one live offer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OfferInfo {
    pub offer_id: u64,
    pub resource: u64,
    pub resource_generation: u64,
    pub state: OfferState,
    /// Local expiry only; custody is unchanged by it.
    pub deadline_passed: bool,
    pub status: Option<ResourceStatusKind>,
    /// That status carried reason image_channel_unavailable.
    pub unavailable: bool,
    /// `SourceReleased` disposition; 0 until then.
    pub disposition: u32,
    /// `cancel` marked this offer; never cleared.
    pub cancel_requested: bool,
}

/// What one dispatch pass changed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DispatchChanged {
    pub channel: bool,
    pub offers: bool,
}

/// Poll readiness the caller observed for the producer's socket.
/// Values mirror poll(2): `IN` also covers hangup detection input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadyEvents(pub u16);

impl ReadyEvents {
    pub const IN: Self = Self(0x1);
    pub const OUT: Self = Self(0x4);
    pub const ERR: Self = Self(0x8);
    pub const HUP: Self = Self(0x10);
    pub const NONE: Self = Self(0);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for ReadyEvents {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

pub(crate) const SOURCE_KIND_SEALED: u32 = 1;
pub(crate) const KIND_BIT_SEALED: u32 = 1;
pub(crate) const MAX_OFFERS: usize = 4;
