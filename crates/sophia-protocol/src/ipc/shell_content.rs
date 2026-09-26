//! Revision-5 CPU content vocabulary. Codec availability is not admission.
//!
//! The session must separately intersect implementation, operator permission and
//! peer requirements. These records never grant device access.

mod codec;
pub(super) mod fields;
mod limits;
mod records;
pub(super) mod validation;

pub use codec::{decode_shell_content_frame, encode_shell_content_frame};
pub(crate) use codec::{decode_shell_content_payload, encode_shell_content_payload};
pub use limits::*;
pub use records::*;
pub use validation::ContentResourceLayout;

pub const SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE: u64 = 1 << 7;
pub const SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT: u64 = 1 << 8;
pub const SOPHIA_SHELL_CONTENT_REVISION: u16 = 5;

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
