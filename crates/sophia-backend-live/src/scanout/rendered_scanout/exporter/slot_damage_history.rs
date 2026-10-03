use std::collections::VecDeque;

use super::frame_slots::{LIVE_RENDERER_FRAME_SLOT_CAPACITY, LiveRendererFrameSlotId};

/// How many rendered snapshots each slot retains. A slot's frame surface hands
/// back a buffer some number of swaps old, so the history has to reach at least
/// as far as the surface's own buffer set; beyond that depth a repaint falls
/// back to painting everything rather than guessing.
pub const LIVE_RENDERER_SLOT_DAMAGE_HISTORY_DEPTH: usize = 4;

/// The age of the buffer a slot's surface just handed back, in renders into
/// that same slot. `1` means the buffer holds the content of the previous
/// render; `0` means the driver would not say, which is not the same as fresh.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveRendererSlotBufferAge(u32);

impl LiveRendererSlotBufferAge {
    pub const UNKNOWN: Self = Self(0);

    pub const fn new(age: u32) -> Self {
        Self(age)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn is_known(self) -> bool {
        self.0 != 0
    }
}

/// What a repaint into a slot owes. `Full` carries no rectangles because the
/// caller already knows the target extent; carrying a synthesized full-output
/// rectangle here would let a caller treat the two outcomes alike and lose the
/// distinction the fallback exists to make.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveRendererSlotRepaint {
    Full {
        reason: LiveRendererSlotFullRepaintReason,
    },
    Partial {
        damage: Vec<sophia_protocol::Rect>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRendererSlotFullRepaintReason {
    /// The driver reported no usable age for the acquired buffer.
    UnknownBufferAge,
    /// The slot has never been written, or its history was invalidated.
    NoHistory,
    /// The buffer is older than the retained history reaches.
    BeyondHistoryDepth,
    /// Engine could not reduce the two snapshots to a bounded damage region.
    DamageUnavailable,
    /// The reduced damage covered enough of the output that a full repaint is
    /// the cheaper honest answer.
    PlanChoseFull,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveRendererSlotDamageMetrics {
    pub partial_repaints: usize,
    pub full_repaints: usize,
    pub invalidations: usize,
    pub records: usize,
}

/// Per-slot content history for buffer-age damage. The history follows the
/// buffer rather than the lease: a slot released and reacquired still holds the
/// pixels it last had, which is the whole reason buffer age is worth anything.
/// What ends a history is a rebuilt bundle or a write that did not complete.
#[derive(Debug, Default)]
pub struct LiveRendererSlotDamageHistory {
    slots: [VecDeque<sophia_engine::OutputFrameDamageSnapshot>; LIVE_RENDERER_FRAME_SLOT_CAPACITY],
    metrics: LiveRendererSlotDamageMetrics,
}

impl LiveRendererSlotDamageHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn metrics(&self) -> LiveRendererSlotDamageMetrics {
        self.metrics
    }

    pub fn depth(&self, slot: LiveRendererFrameSlotId) -> usize {
        self.slots
            .get(slot.index())
            .map_or(0, std::collections::VecDeque::len)
    }

    /// An opportunity cohort independent of whether client damage is usable.
    /// Compare consecutive renders, not the buffer-age-selected older image.
    /// Identity and provenance still decide the actual repaint separately.
    pub fn stable_geometry(
        &self,
        slot: LiveRendererFrameSlotId,
        current: &sophia_engine::OutputFrameDamageSnapshot,
    ) -> bool {
        self.slots
            .get(slot.index())
            .and_then(|history| history.front())
            .is_some_and(|previous| {
                previous.output == current.output
                    && previous.compositor_display_list == current.compositor_display_list
                    && previous.software_cursor == current.software_cursor
                    && !current.surfaces.is_empty()
                    && previous.surfaces.len() == current.surfaces.len()
                    && previous
                        .surfaces
                        .iter()
                        .zip(&current.surfaces)
                        .all(|(a, b)| {
                            a.surface == b.surface
                                && a.logical_geometry == b.logical_geometry
                                && a.geometry == b.geometry
                                && a.source_size == b.source_size
                                && b.source_size.width == b.geometry.width
                                && b.source_size.height == b.geometry.height
                        })
            })
    }

    /// Decide what a repaint into `slot` owes, given the age its surface
    /// reported and the scene it is being brought up to.
    pub fn plan(
        &mut self,
        slot: LiveRendererFrameSlotId,
        age: LiveRendererSlotBufferAge,
        current: Option<&sophia_engine::OutputFrameDamageSnapshot>,
        size: sophia_protocol::Size,
    ) -> LiveRendererSlotRepaint {
        let plan = self.evaluate(slot, age, current, size);
        match &plan {
            LiveRendererSlotRepaint::Partial { .. } => self.metrics.partial_repaints += 1,
            LiveRendererSlotRepaint::Full { .. } => self.metrics.full_repaints += 1,
        }
        plan
    }

    fn evaluate(
        &self,
        slot: LiveRendererFrameSlotId,
        age: LiveRendererSlotBufferAge,
        current: Option<&sophia_engine::OutputFrameDamageSnapshot>,
        size: sophia_protocol::Size,
    ) -> LiveRendererSlotRepaint {
        let full = |reason| LiveRendererSlotRepaint::Full { reason };
        if !age.is_known() {
            return full(LiveRendererSlotFullRepaintReason::UnknownBufferAge);
        }
        let Some(history) = self.slots.get(slot.index()) else {
            return full(LiveRendererSlotFullRepaintReason::NoHistory);
        };
        if history.is_empty() {
            return full(LiveRendererSlotFullRepaintReason::NoHistory);
        }
        // Age n means the buffer holds what was rendered n renders ago into this
        // slot, so the work owed is every change since that render: exactly the
        // damage between that snapshot and the current one.
        let index = (age.get() as usize) - 1;
        let Some(previous) = history.get(index) else {
            return full(LiveRendererSlotFullRepaintReason::BeyondHistoryDepth);
        };
        let Some(current) = current else {
            return full(LiveRendererSlotFullRepaintReason::DamageUnavailable);
        };
        let Ok(damage) = sophia_engine::output_frame_damage(Some(previous), current) else {
            return full(LiveRendererSlotFullRepaintReason::DamageUnavailable);
        };
        match sophia_engine::plan_output_repaint(
            size,
            &damage,
            sophia_engine::OutputRepaintPolicy::default(),
        ) {
            Ok(sophia_engine::OutputRepaintPlan::Partial { damage, .. }) => {
                LiveRendererSlotRepaint::Partial {
                    damage: damage.rects,
                }
            }
            // A scene the output already holds still owes nothing, which is a
            // partial repaint of no regions rather than a full one.
            Ok(sophia_engine::OutputRepaintPlan::Skip) => {
                LiveRendererSlotRepaint::Partial { damage: Vec::new() }
            }
            Ok(sophia_engine::OutputRepaintPlan::Full { .. }) => {
                full(LiveRendererSlotFullRepaintReason::PlanChoseFull)
            }
            Err(_) => full(LiveRendererSlotFullRepaintReason::DamageUnavailable),
        }
    }

    /// Record a completed write. Only a complete write may be recorded: a slot
    /// that holds neither its old content nor its new one has no age worth
    /// naming, and recording one would let the next repaint paint too little.
    pub fn record(
        &mut self,
        slot: LiveRendererFrameSlotId,
        snapshot: sophia_engine::OutputFrameDamageSnapshot,
    ) {
        let Some(history) = self.slots.get_mut(slot.index()) else {
            return;
        };
        history.push_front(snapshot);
        history.truncate(LIVE_RENDERER_SLOT_DAMAGE_HISTORY_DEPTH);
        self.metrics.records += 1;
    }

    /// Drop a slot's history. Its bundle was rebuilt or its write did not
    /// complete, so nothing is known about what its buffers now hold.
    pub fn invalidate(&mut self, slot: LiveRendererFrameSlotId) {
        let Some(history) = self.slots.get_mut(slot.index()) else {
            return;
        };
        if history.is_empty() {
            return;
        }
        history.clear();
        self.metrics.invalidations += 1;
    }

    /// Drop every slot's history, for a size change or a context reset that
    /// rebuilds all of them at once.
    pub fn invalidate_all(&mut self) {
        for slot in 0..LIVE_RENDERER_FRAME_SLOT_CAPACITY {
            if let Some(slot) = LiveRendererFrameSlotId::from_index(slot) {
                self.invalidate(slot);
            }
        }
    }
}
