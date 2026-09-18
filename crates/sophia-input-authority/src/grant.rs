//! Grants, and the two handles that keep issuing separate from submitting.
//!
//! Session holds the issuer. Adapters hold a submit handle and capabilities.
//! An adapter therefore has no expression for "register a physical device" or
//! "issue myself a grant", which is stronger than a policy saying it must not.

use crate::identity::{AuthorityUid, SeatBinding};

/// Identifies one grant within one authority.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GrantId {
    pub(crate) authority: AuthorityUid,
    pub(crate) slot: usize,
    pub(crate) generation: GrantGeneration,
}

/// Reissued whenever a grant is revoked and its slot reused.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GrantGeneration(pub(crate) u64);

/// Session's handle: issues grants, registers physical sources, revokes,
/// publishes transitions.
///
/// Carries the authority's own identity rather than its public seat binding.
/// A caller can rebuild a `SeatBinding` from public values and construct a
/// second authority with it; it cannot mint the identity below, so a handle
/// from that second authority is refused by the first.
pub struct IssuerHandle {
    authority: AuthorityUid,
    binding: SeatBinding,
}

/// An adapter's handle: submits work, issues nothing.
#[derive(Clone, Copy, Debug)]
pub struct SubmitHandle {
    authority: AuthorityUid,
    binding: SeatBinding,
}

impl IssuerHandle {
    pub(crate) fn new(authority: AuthorityUid, binding: SeatBinding) -> Self {
        Self { authority, binding }
    }

    pub(crate) fn authority(&self) -> AuthorityUid {
        self.authority
    }

    /// Which seat and instance this issuer speaks for.
    pub fn binding(&self) -> SeatBinding {
        self.binding
    }
}

impl SubmitHandle {
    pub(crate) fn new(authority: AuthorityUid, binding: SeatBinding) -> Self {
        Self { authority, binding }
    }

    pub(crate) fn authority(&self) -> AuthorityUid {
        self.authority
    }

    /// Which seat and instance this handle speaks for.
    ///
    /// Exposed so an adapter can refuse a mismatched request before it reaches
    /// the authority, not so it can choose one.
    pub fn binding(&self) -> SeatBinding {
        self.binding
    }
}
