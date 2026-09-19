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

/// One refresh period, as a duration.
///
/// The single place a millihertz refresh becomes a frame interval. A zero
/// refresh takes sixty hertz, the same fallback `head_refresh_millihz` applies
/// and for the same reason: a head reporting zero is not asserting that it
/// does not scan out, and dividing by it downstream is not an option. The
/// result is never zero, because callers use it as a wait.
pub fn head_refresh_interval(refresh_millihz: u32) -> std::time::Duration {
    let refresh_millihz = if refresh_millihz == 0 {
        60_000
    } else {
        refresh_millihz
    };
    std::time::Duration::from_micros((1_000_000_000_u64 / u64::from(refresh_millihz)).max(1))
}

/// The head a cadence falls back to when no desktop primary is published yet.
///
/// Lowest *enabled*, not lowest. A head that has been disabled keeps whatever
/// refresh it last carried, so choosing one would pace the whole desktop from
/// a display that is no longer scanning out. The previous selection took
/// `heads.first()` and filtered nothing.
pub(super) fn fallback_cadence_index(enabled: &[bool]) -> Option<usize> {
    enabled.iter().position(|enabled| *enabled)
}

mod tests;
