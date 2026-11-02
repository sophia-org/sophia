//! Wire-neutral typed model for the revision-5 CPU content vocabulary.
//!
//! Codec availability is not admission. The session must separately
//! intersect implementation, operator permission and peer requirements.
//! These records never grant device access.

mod limits;
mod records;
pub(crate) mod validation;

pub use limits::*;
pub use records::*;
pub use validation::ContentResourceLayout;

/// Semantic validation of one content record, as every codec that carries
/// it applies after its own structural and framing checks.
pub fn validate_shell_content_record(
    record: &ShellContentRecord,
) -> Result<(), crate::InvalidRecord> {
    validation::validate(record)
}

pub const SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE: u64 = 1 << 7;
pub const SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT: u64 = 1 << 8;
pub const SOPHIA_SHELL_CONTENT_REVISION: u16 = 5;
/// Shell revision whose CPU image source is the owned ring
/// (`docs/sophia-owned-ring.md`), negotiated with capability bit 15.
pub const SOPHIA_SHELL_OWNED_RING_REVISION: u16 = 10;
/// Shell r12 / capability bit 17: a whole candidate may name one predecessor
/// to retire once it is presented. Only a connection that selected this
/// revision and was granted the capability may send the intent.
pub const SOPHIA_SHELL_RETIRE_ON_PRESENT_REVISION: u16 = 12;
pub const SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT: u64 = 1 << 17;

/// Whether a negotiated connection may carry a candidate's retire intent.
pub const fn retire_on_present_enabled(selected_revision: u16, capabilities: u64) -> bool {
    selected_revision == SOPHIA_SHELL_RETIRE_ON_PRESENT_REVISION
        && capabilities & SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT != 0
}

/// The image mapping for allocated shell revisions 10, 11 and 12 when bit 15
/// is granted. Draw and retirement are independent capabilities; no mapping
/// is established for an unallocated later shell revision.
pub const fn owned_ring_image_revision(selected_revision: u16, capabilities: u64) -> Option<u16> {
    const OWNED_RING: u64 = 1 << 15;
    match selected_revision {
        10..=12 if capabilities & OWNED_RING != 0 => Some(SOPHIA_SHELL_OWNED_RING_REVISION),
        _ => None,
    }
}

/// The W2 intent a whole candidate carries: retire `resource` on its own
/// grant, at this exact generation, under `retire_transaction`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentCandidateRetire {
    pub retire_transaction: crate::TransactionId,
    pub resource: ContentResourceId,
}

/// Connection identity and the independently issued content permission epoch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct ContentGrant {
    pub connection_epoch: u64,
    pub content_grant_epoch: u64,
}

/// Resource generations are scoped to a grant, never to a process or address.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct ContentResourceId {
    pub id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct ContentAllocationId {
    pub id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct ContentOutputId {
    pub id: u64,
    pub generation: u64,
}

/// Logical proposals and physical allocations intentionally have distinct types.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentLogicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentPixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentMargins {
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
    pub left: i16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ContentReason {
    None = 0,
    Stale = 1,
    Budget = 2,
    Malformed = 3,
    Unauthorized = 4,
    Incomplete = 5,
    Timeout = 6,
    OutputLost = 7,
    AllocationLost = 8,
    RendererFailed = 9,
    Superseded = 10,
    Cancelled = 11,
    Revoked = 12,
}
