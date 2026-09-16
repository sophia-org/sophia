//! Presentation replacement and button lifetime are different identities.
use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{PresentedContentBinding, PresentedContentTarget};

/// Server-local lifetime of one continuously presented, equivalent target.
/// Never sent to a client and never owns pixels or renderer resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentTargetContinuity(NonZeroU64);

impl ContentTargetContinuity {
    /// Mint only at an authoritative presentation boundary. Exhaustion disables
    /// continuity rather than wrapping an identity held by an old press.
    pub fn mint() -> Option<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .ok()
            .and_then(NonZeroU64::new)
            .map(Self)
    }
}

/// All authority and geometry, excluding the replaceable raster's identity.
pub fn same_content_target_authority(
    a: &PresentedContentTarget,
    b: &PresentedContentTarget,
) -> bool {
    a.grant == b.grant
        && a.output == b.output
        && a.interaction_generation == b.interaction_generation
        && a.allocation == b.allocation
        && a.scale_generation == b.scale_generation
        && a.allocation_logical == b.allocation_logical
        && a.allocation_pixel == b.allocation_pixel
        && a.target_id == b.target_id
        && a.target_generation == b.target_generation
        && a.action_id == b.action_id
        && a.bounds_px == b.bounds_px
}

pub fn content_target_continues(a: &PresentedContentTarget, b: &PresentedContentTarget) -> bool {
    a.continuity.is_some() && a.continuity == b.continuity && same_content_target_authority(a, b)
}

/// Reconcile each binding against the immediately preceding projection.
/// The publisher must store absent/empty projections too, discarding old tokens.
/// Looking only at press and release would miss an intervening removal followed
/// by an identical reappearance. No historical target registry is needed.
pub fn reconcile_content_continuity(
    previous: Option<&PresentedContentBinding>,
    next: &mut PresentedContentBinding,
) {
    let previous = previous.filter(|p| {
        p.authority_current
            && next.authority_current
            && p.output == next.output
            && p.transform == next.transform
    });
    for target in &mut next.targets {
        target.continuity = if next.authority_current {
            previous
                .and_then(|p| {
                    let mut matches = p
                        .targets
                        .iter()
                        .filter(|old| same_content_target_authority(old, target));
                    let first = matches.next()?;
                    matches
                        .next()
                        .is_none()
                        .then_some(first.continuity)
                        .flatten()
                })
                .or_else(ContentTargetContinuity::mint)
        } else {
            None
        };
    }
}
