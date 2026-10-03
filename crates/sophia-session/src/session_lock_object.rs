//! The lock object a lock provider reads (t294), derived from Session's lock
//! phase and the published output topology. It is republished on every
//! phase, lock epoch and topology change, and grants allocations only while
//! the cover is drawn.
//!
//! One allocation covers one logical output. Its pixel size is the largest
//! current mode among the output's heads; Engine scales the image to every
//! mirror head, so a smaller mirror shows it reduced rather than cropped. The
//! scale is the nearest ratio the contract allows to pixels per logical unit.
use sophia_protocol::lock_files::{
    LOCK_FILE_MAX_OUTPUTS, LOCK_FILE_MAX_PIXELS_PER_SIDE, LockAllocation, LockObject, LockPhase,
};
use sophia_protocol::{OutputAuthoritySnapshot, OutputLogicalGroupState, Size};

use crate::session_lock::SessionLockPhase;

/// The lock object for `phase` over `topology`. Without a topology no
/// allocation can be granted, and the cover alone is shown.
pub fn session_lock_object(
    phase: SessionLockPhase,
    topology: Option<&OutputAuthoritySnapshot>,
) -> LockObject {
    let (lock_epoch, phase, covers) = match phase {
        SessionLockPhase::Unlocked => (0, LockPhase::Unlocked, false),
        SessionLockPhase::Locking { epoch, .. } => (epoch.raw(), LockPhase::Locking, true),
        SessionLockPhase::Locked { epoch, .. } => (epoch.raw(), LockPhase::Locked, true),
        SessionLockPhase::Unlocking { epoch } => (epoch.raw(), LockPhase::Unlocking, false),
    };
    let allocations = match topology {
        Some(topology) if covers => topology
            .groups
            .iter()
            .filter_map(|group| allocation(topology, group))
            .take(LOCK_FILE_MAX_OUTPUTS)
            .collect(),
        _ => Vec::new(),
    };
    LockObject {
        lock_epoch,
        // A zero topology epoch is not a generation the contract accepts.
        topology_generation: topology.map_or(1, |topology| topology.topology_epoch.max(1)),
        phase,
        allocations,
    }
}

fn current_size(
    topology: &OutputAuthoritySnapshot,
    group: &OutputLogicalGroupState,
) -> Option<Size> {
    group
        .members
        .iter()
        .filter_map(|member| topology.heads.iter().find(|head| head.head == member.head))
        .filter(|head| head.enabled)
        .filter_map(|head| {
            let mode = head.current_mode?;
            head.modes
                .iter()
                .find(|candidate| candidate.mode == mode)
                .map(|candidate| candidate.pixel_size)
        })
        .max_by_key(|size| (i64::from(size.width) * i64::from(size.height), size.width))
}

/// The scale numerator and denominator (1..=32 over 1..=4) nearest to
/// `pixels` per `logical` unit.
fn scale(pixels: i32, logical: i32) -> (u32, u32) {
    let (pixels, logical) = (f64::from(pixels.max(1)), f64::from(logical.max(1)));
    let ratio = pixels / logical;
    (1..=4u32)
        .map(|denominator| {
            let numerator = (ratio * f64::from(denominator)).round().clamp(1.0, 32.0) as u32;
            let error = (f64::from(numerator) / f64::from(denominator) - ratio).abs();
            (numerator, denominator, error)
        })
        .min_by(|a, b| a.2.total_cmp(&b.2).then(a.1.cmp(&b.1)))
        .map_or((1, 1), |(numerator, denominator, _)| {
            (numerator, denominator)
        })
}

fn allocation(
    topology: &OutputAuthoritySnapshot,
    group: &OutputLogicalGroupState,
) -> Option<LockAllocation> {
    let size = current_size(topology, group)?;
    let side = |value: i32| {
        u32::try_from(value)
            .ok()
            .filter(|v| (1..=LOCK_FILE_MAX_PIXELS_PER_SIDE).contains(v))
    };
    let (pixel_width, pixel_height) = (side(size.width)?, side(size.height)?);
    let (scale_numerator, scale_denominator) = scale(size.width, group.logical.width);
    let output_id = group.output.raw();
    let generation = group.generation.max(1);
    (output_id != 0).then_some(LockAllocation {
        output_id,
        output_generation: generation,
        // One allocation per output: its identity is the output's, and its
        // generation moves with the output's, so a topology change makes
        // every older candidate stale.
        allocation_id: output_id,
        allocation_generation: generation,
        pixel_width,
        pixel_height,
        scale_numerator,
        scale_denominator,
    })
}
