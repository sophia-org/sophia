//! Which refresh a head is paced at, and where that number is allowed to
//! come from.
//!
//! Sysfs discovery reads `/sys/class/drm/<card>-<connector>/modes`, which
//! carries only `WIDTHxHEIGHT`. Everything else in the record it produces is a
//! default, and the refresh default is sixty hertz -- for every head, on every
//! card, whatever the connector actually runs at.
//!
//! That fabricated number reached the owner loop's frame pacer, so a hundred
//! and twenty hertz head was paced at 16.6ms until an output topology
//! transaction applied and overwrote it. Whether that transaction landed
//! decided, session to session, whether a client saw a thirty or a sixty hertz
//! cadence, with no configuration or code change between them.
//!
//! The kernel's own mode is available wherever a selection was discovered.
//! This prefers it and keeps the record only as the fallback it always was.

/// Resolve a head's refresh in millihertz.
///
/// `mode_vrefresh` is the KMS mode's whole-hertz refresh when discovery
/// attached a mode; `fallback_millihz` is the sysfs record's value, which is a
/// default rather than an observation unless something later corrected it.
///
/// A zero or absent mode refresh falls back: the kernel reporting zero is not
/// an assertion that the head does not scan out, and pacing on it would divide
/// by zero downstream.
pub(super) fn head_refresh_millihz(mode_vrefresh: Option<u32>, fallback_millihz: u32) -> u32 {
    match mode_vrefresh {
        Some(vrefresh) if vrefresh > 0 => vrefresh.saturating_mul(1_000),
        _ => fallback_millihz,
    }
}

mod tests;
