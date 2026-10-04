//! Bounded, immutable damage facts. Recording is a commit operation; constructing
//! a candidate view does not advance or evict the authoritative history.
use super::{OutputDamageCause, OutputFrameSurfaceState};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, Rect, Region, Size, SurfaceId, SurfaceRasterTransform,
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

pub const SURFACE_DAMAGE_TRANSITIONS: usize = 16;
pub const SURFACE_DAMAGE_RECTS: usize = 32;
pub const SURFACE_DAMAGE_BYTES: usize = 8 * 1024 * 1024;

/// Opaque identity of one preparation, preserved by clones and by its commit.
/// Equal generation numbers or client buffer handles do not prove equal pixels.
#[derive(Clone, Debug, Default)]
pub struct SurfaceDamageIdentity(Arc<DamagePreparation>);
#[derive(Clone, Debug, Default)]
struct DamagePreparation {
    rebased_surface: Option<SurfaceId>,
}
impl SurfaceDamageIdentity {
    pub(crate) fn mark_rebased(&mut self, surface: SurfaceId) {
        // The just-created preparation is uniquely owned. Preserve that
        // allocation; copy-on-write also keeps any prior clones immutable.
        Arc::make_mut(&mut self.0).rebased_surface = Some(surface);
    }
}
impl PartialEq for SurfaceDamageIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for SurfaceDamageIdentity {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceDamageTransition {
    surface: SurfaceId,
    before_identity: Option<SurfaceDamageIdentity>,
    after_identity: SurfaceDamageIdentity,
    predecessor: u64,
    successor: u64,
    before: BufferSource,
    after: BufferSource,
    size: Size,
    // None is an explicit full-damage marker, not an empty update.
    rects: Option<Vec<Rect>>,
    full_reason: Option<OutputDamageCause>,
}
impl SurfaceDamageTransition {
    fn between(
        before: &CommittedSurfaceState,
        after: &CommittedSurfaceState,
        before_identity: Option<SurfaceDamageIdentity>,
        after_identity: SurfaceDamageIdentity,
    ) -> Self {
        let old = before.content.canonical_variant();
        let new = after.content.canonical_variant();
        let full_reason = if before.surface != after.surface
            || before.committed_generation.checked_add(1) != Some(after.committed_generation)
        {
            Some(OutputDamageCause::Generation)
        } else if before.geometry != after.geometry {
            Some(OutputDamageCause::Geometry)
        } else if old.variant != new.variant
            || old.pixel_size != new.pixel_size
            || old.transform != SurfaceRasterTransform::Normal
            || new.transform != SurfaceRasterTransform::Normal
            || old.density_millis != new.density_millis
        {
            Some(OutputDamageCause::Sampling)
        } else if new.damage.rects.len() > SURFACE_DAMAGE_RECTS {
            Some(OutputDamageCause::RectLimit)
        } else {
            None
        };
        Self {
            before_identity,
            after_identity,
            surface: after.surface,
            predecessor: before.committed_generation,
            successor: after.committed_generation,
            before: old.source,
            after: new.source,
            size: new.pixel_size,
            rects: full_reason.is_none().then(|| new.damage.rects.clone()),
            full_reason,
        }
    }
    fn origin(after: &CommittedSurfaceState, identity: SurfaceDamageIdentity) -> Self {
        let mut before = after.clone();
        before.committed_generation = 0;
        let mut edge = Self::between(&before, after, None, identity);
        edge.rects = None;
        edge.full_reason = Some(OutputDamageCause::Origin);
        edge
    }
    fn bytes(&self) -> usize {
        // Include the allocation header, queue slot and journal sequencing.
        std::mem::size_of::<Self>()
            + 256
            + self
                .rects
                .as_ref()
                .map_or(0, |r| r.capacity() * std::mem::size_of::<Rect>())
    }
}

#[derive(Clone, Debug, Default)]
pub struct SurfaceDamageHistory {
    surfaces: BTreeMap<SurfaceId, VecDeque<(u64, Arc<SurfaceDamageTransition>)>>,
    bytes: usize,
    sequence: u64,
}
impl SurfaceDamageHistory {
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn record_committed(
        &mut self,
        before: &[CommittedSurfaceState],
        after: &[CommittedSurfaceState],
        identity: &SurfaceDamageIdentity,
    ) {
        let removed: Vec<_> = self
            .surfaces
            .keys()
            .copied()
            .filter(|id| !after.iter().any(|state| state.surface == *id))
            .collect();
        for id in removed {
            if let Some(entries) = self.surfaces.remove(&id) {
                for (_, entry) in entries {
                    self.bytes -= entry.bytes();
                }
            }
        }
        for next in after {
            let mut origin;
            let previous = match before.iter().find(|state| state.surface == next.surface) {
                Some(previous) => previous,
                None => {
                    origin = next.clone();
                    origin.committed_generation = 0;
                    &origin
                }
            };
            if previous == next {
                continue;
            }
            let before_identity = self.surfaces.get(&previous.surface).and_then(|entries| {
                entries
                    .back()
                    .filter(|(_, edge)| {
                        edge.successor == previous.committed_generation
                            && edge.after == previous.buffer()
                    })
                    .map(|(_, edge)| edge.after_identity.clone())
            });
            let transition = Arc::new(
                if before.iter().any(|state| state.surface == next.surface) {
                    SurfaceDamageTransition::between(
                        previous,
                        next,
                        before_identity,
                        identity.clone(),
                    )
                } else {
                    SurfaceDamageTransition::origin(next, identity.clone())
                },
            );
            let entries = self.surfaces.entry(next.surface).or_default();
            if previous.committed_generation >= next.committed_generation {
                for (_, entry) in entries.drain(..) {
                    self.bytes -= entry.bytes();
                }
                continue;
            }
            if entries.len() == SURFACE_DAMAGE_TRANSITIONS {
                self.bytes -= entries.pop_front().expect("bounded queue").1.bytes();
            }
            self.sequence = self.sequence.saturating_add(1);
            self.bytes += transition.bytes();
            entries.push_back((self.sequence, transition));
        }
        self.surfaces.retain(|_, entries| !entries.is_empty());
        while self.bytes > SURFACE_DAMAGE_BYTES
            || self.surfaces.len() > super::MAX_OUTPUT_FRAME_SURFACES
        {
            let oldest = self
                .surfaces
                .iter()
                .filter_map(|(id, entries)| entries.front().map(|(seq, _)| (*seq, *id)))
                .min();
            let Some((_, id)) = oldest else {
                break;
            };
            let entries = self.surfaces.get_mut(&id).expect("selected entry");
            self.bytes -= entries.pop_front().expect("selected front").1.bytes();
            if entries.is_empty() {
                self.surfaces.remove(&id);
            }
        }
    }
    /// The extra transition remains local to this candidate, including when it
    /// is rejected or superseded. A future candidate cannot inherit it.
    pub fn for_candidate(
        &self,
        committed: &[CommittedSurfaceState],
        candidate: &[CommittedSurfaceState],
        identity: Option<&SurfaceDamageIdentity>,
    ) -> Result<Arc<[Arc<SurfaceDamageTransition>]>, super::OutputFrameDamageError> {
        if candidate.len() > super::MAX_OUTPUT_FRAME_SURFACES {
            return Err(super::OutputFrameDamageError::SurfaceCapacityExceeded);
        }
        // Unassociated views must not alias one another. Only a caller with
        // the actual prepared commit can carry this identity into a commit.
        let identity = identity.cloned().unwrap_or_default();
        let mut result = Vec::new();
        let mut pending = Vec::new();
        for next in candidate {
            let previous = committed.iter().find(|state| state.surface == next.surface);
            let provisional = previous != Some(next);
            if let Some(entries) = self.surfaces.get(&next.surface) {
                let skip = usize::from(provisional && entries.len() == SURFACE_DAMAGE_TRANSITIONS);
                result.extend(
                    entries
                        .iter()
                        .skip(skip)
                        .map(|(_, entry)| Arc::clone(entry)),
                );
            }
            if previous.is_none()
                || previous.is_some_and(|state| {
                    state != next && state.committed_generation >= next.committed_generation
                })
            {
                pending.push(Arc::new(SurfaceDamageTransition::origin(
                    next,
                    identity.clone(),
                )));
            }
            if let Some(previous) = committed.iter().find(|state| state.surface == next.surface)
                && previous != next
                && previous.committed_generation < next.committed_generation
            {
                let before_identity = self.surfaces.get(&previous.surface).and_then(|entries| {
                    entries
                        .back()
                        .filter(|(_, edge)| {
                            edge.successor == previous.committed_generation
                                && edge.after == previous.buffer()
                        })
                        .map(|(_, edge)| edge.after_identity.clone())
                });
                pending.push(Arc::new(SurfaceDamageTransition::between(
                    previous,
                    next,
                    before_identity,
                    identity.clone(),
                )));
            }
        }
        // A pending endpoint is the only identity of pixels that may already
        // have reached a slot without committing. Never evict it to make room
        // for older transitions. Each validated frame has at most 1024 sources,
        // so these endpoints fit within the budget even at 32 rectangles each.
        let mut bytes: usize = result.iter().chain(&pending).map(|edge| edge.bytes()).sum();
        let skip = result
            .iter()
            .take_while(|edge| {
                if bytes <= SURFACE_DAMAGE_BYTES {
                    return false;
                }
                bytes -= edge.bytes();
                true
            })
            .count();
        result.drain(..skip);
        result.extend(pending);
        Ok(result.into())
    }
}

/// Preserve pixel identities for every source, but make unproved mappings full.
pub fn restrict_surface_damage_precision(
    history: Arc<[Arc<SurfaceDamageTransition>]>,
    permitted: &[SurfaceId],
) -> Arc<[Arc<SurfaceDamageTransition>]> {
    history
        .iter()
        .map(|edge| {
            if edge.rects.is_none() || permitted.contains(&edge.surface) {
                Arc::clone(edge)
            } else {
                let mut full = (**edge).clone();
                full.rects = None;
                full.full_reason = Some(OutputDamageCause::PrecisionRestricted);
                Arc::new(full)
            }
        })
        .collect()
}

pub(super) fn instance_damage_identity(
    surface: SurfaceId,
    generation: u64,
    history: &[Arc<SurfaceDamageTransition>],
) -> Option<&SurfaceDamageIdentity> {
    history
        .iter()
        .rev()
        .find(|edge| edge.surface == surface && edge.successor == generation)
        .map(|edge| &edge.after_identity)
}

/// A preparation identifies the whole content set, including alternate source
/// variants. Matching its canonical buffer here would lose the identity of a
/// frame rendered from another variant. Precise chaining below still requires
/// matching source buffers and an independently proved sampling footprint.
pub(super) fn surface_damage_identity<'a>(
    state: &OutputFrameSurfaceState,
    history: &'a [Arc<SurfaceDamageTransition>],
) -> Option<&'a SurfaceDamageIdentity> {
    history
        .iter()
        .rev()
        .find(|edge| edge.surface == state.surface && edge.successor == state.committed_generation)
        .map(|edge| &edge.after_identity)
}

pub(super) fn accumulated_surface_damage(
    before: &OutputFrameSurfaceState,
    after: &OutputFrameSurfaceState,
    before_history: &[Arc<SurfaceDamageTransition>],
    history: &[Arc<SurfaceDamageTransition>],
    causes: &mut super::OutputDamageCauses,
) -> Result<Region, OutputDamageCause> {
    use OutputDamageCause as C;
    if before.surface != after.surface
        || before.geometry != after.geometry
        || before.logical_geometry != after.logical_geometry
    {
        return Err(C::Geometry);
    }
    if before.source_size != after.source_size
        || after.geometry.width != after.source_size.width
        || after.geometry.height != after.source_size.height
    {
        return Err(C::Sampling);
    }
    if before.committed_generation >= after.committed_generation {
        return Err(C::Generation);
    }
    let mut generation = before.committed_generation;
    let mut source = before.buffer;
    let mut identity = surface_damage_identity(before, before_history).ok_or(C::MissingIdentity)?;
    let mut damage = Region::empty();
    for _ in 0..=SURFACE_DAMAGE_TRANSITIONS {
        let edge = history
            .iter()
            .find(|edge| {
                edge.surface == after.surface
                    && edge.predecessor == generation
                    && edge.before == source
                    && edge.before_identity.as_ref() == Some(identity)
            })
            .ok_or(C::NoMatchingTransition)?;
        if edge.after_identity.0.rebased_surface == Some(after.surface) {
            causes.insert(C::Rebased);
        }
        if edge.size != after.source_size || edge.successor <= generation {
            return Err(C::InvalidTransition);
        }
        for rect in edge
            .rects
            .as_ref()
            .ok_or(edge.full_reason.unwrap_or(C::InvalidTransition))?
        {
            damage.push(Rect {
                x: after
                    .geometry
                    .x
                    .checked_add(rect.x)
                    .ok_or(C::CoordinateOverflow)?,
                y: after
                    .geometry
                    .y
                    .checked_add(rect.y)
                    .ok_or(C::CoordinateOverflow)?,
                ..*rect
            });
        }
        generation = edge.successor;
        source = edge.after;
        identity = &edge.after_identity;
        if generation == after.committed_generation && source == after.buffer {
            // Equal public generation/source do not prove the prepared pixels.
            if Some(identity) == surface_damage_identity(after, history) {
                return Ok(damage);
            }
            return Err(C::TerminalIdentity);
        }
        if generation >= after.committed_generation {
            return Err(C::TerminalIdentity);
        }
    }
    Err(C::HistoryLimit)
}
