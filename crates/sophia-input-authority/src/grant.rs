//! Grants, and the two handles that keep issuing separate from submitting.
//!
//! Session holds the issuer. Adapters hold a submit handle and capabilities.
//! An adapter therefore has no expression for "register a physical device" or
//! "issue myself a grant", which is a stronger statement than a policy that
//! says it must not.

use crate::identity::SeatBinding;

/// Identifies one grant within one authority.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GrantId(pub(crate) u32);

/// Reissued whenever a grant is revoked and a new one takes its slot.
///
/// Carried on every capability and every queued request, and compared at
/// execution. A reconnecting client gets a new generation, so nothing pending
/// from the old one can execute: replay is impossible rather than prevented.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GrantGeneration(pub(crate) u64);

/// Session's handle. Issues grants, registers physical sources, revokes.
///
/// Deliberately not `Clone`: one issuer per authority, held by the component
/// that decides authorization, so an adapter cannot come to hold one by
/// copying it out of a structure it was given.
pub struct IssuerHandle {
    pub(crate) binding: SeatBinding,
}

/// An adapter's handle. Submits work; issues nothing.
#[derive(Clone, Copy, Debug)]
pub struct SubmitHandle {
    pub(crate) binding: SeatBinding,
}

impl SubmitHandle {
    /// Which seat and instance this handle speaks for.
    ///
    /// Exposed so an adapter can refuse a request that names a different one
    /// before it reaches the authority, not so it can choose.
    pub fn binding(&self) -> SeatBinding {
        self.binding
    }
}
