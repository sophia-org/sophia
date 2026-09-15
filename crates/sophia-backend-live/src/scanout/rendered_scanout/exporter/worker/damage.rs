fn reusable_cpu_buffer_damage(
    previous_checksum: u64,
    previous: Option<&sophia_engine::OutputFrameDamageSnapshot>,
    checksum: u64,
    current: Option<&sophia_engine::OutputFrameDamageSnapshot>,
    size: sophia_protocol::Size,
) -> Vec<sophia_protocol::Rect> {
    if previous_checksum == checksum {
        return Vec::new();
    }
    let damage = previous
        .zip(current)
        .and_then(|(previous, current)| {
            sophia_engine::output_frame_damage(Some(previous), current).ok()
        })
        .and_then(|damage| {
            sophia_engine::plan_output_repaint(
                size,
                &damage,
                sophia_engine::OutputRepaintPolicy::default(),
            )
            .ok()
        })
        .map(|repaint| match repaint {
            sophia_engine::OutputRepaintPlan::Skip => Vec::new(),
            sophia_engine::OutputRepaintPlan::Partial { damage, .. }
            | sophia_engine::OutputRepaintPlan::Full { damage, .. } => damage.rects,
        });
    damage.unwrap_or_else(|| {
        vec![sophia_protocol::Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        }]
    })
}

/// A worker's view of what its slots' buffers already hold.
///
/// One per worker, which is one per physical head: a head's slots are its own,
/// and nothing here crosses threads. It owns the history, the bundle identity
/// each slot last rendered against, and whether damage-limited repaint is
/// switched on at all.
pub struct WorkerSlotDamage {
    history: LiveRendererSlotDamageHistory,
    target_generations: [Option<u64>; LIVE_RENDERER_FRAME_SLOT_CAPACITY],
    enabled: bool,
}

impl WorkerSlotDamage {
    fn new() -> Self {
        // On by default. The precondition this was opt-in for is met:
        // `tools/check_buffer_age_equivalence.sh` renders a twelve-frame
        // sequence twice on this host's GPU, damage-limited and full, and
        // requires the results byte-identical -- and requires a lying damage
        // table to be caught by the same comparison. Signed native archive
        // 0002 then promoted the path on hardware.
        //
        // The failure mode that justified opt-in -- a frame presentable,
        // self-consistent, and stale in one region -- is structurally
        // unreachable rather than merely untriggered: every path that cannot
        // prove a buffer age falls back to a full repaint under a named
        // reason, and a partial write records no history at all.
        //
        // SOPHIA_ENABLE_BUFFER_AGE_DAMAGE=0 is the opt-out, kept so a session
        // suspecting a stale region can rule this path out without a rebuild.
        Self::with_enabled(
            !std::env::var("SOPHIA_ENABLE_BUFFER_AGE_DAMAGE").is_ok_and(|value| value == "0"),
        )
    }

    /// Deterministic constructor: the environment decides only in production.
    pub fn with_enabled(enabled: bool) -> Self {
        Self {
            history: LiveRendererSlotDamageHistory::new(),
            target_generations: [None; LIVE_RENDERER_FRAME_SLOT_CAPACITY],
            enabled,
        }
    }

    pub fn repaint_table(
        &mut self,
        slot: LiveRendererFrameSlotId,
        snapshot: Option<&sophia_engine::OutputFrameDamageSnapshot>,
        size: sophia_protocol::Size,
    ) -> Option<sophia_renderer_live::NativeCompositionRepaintTable> {
        if !self.enabled {
            return None;
        }
        slot_repaint_table(&mut self.history, slot, snapshot, size)
    }

    /// Settle what a completed export means for the slot's retained content.
    pub fn settle(
        &mut self,
        slot: LiveRendererFrameSlotId,
        exported: bool,
        target_generation: Option<u64>,
        snapshot: Option<sophia_engine::OutputFrameDamageSnapshot>,
    ) {
        observe_slot_target_generation(
            &mut self.history,
            &mut self.target_generations,
            slot,
            target_generation,
        );
        // Only a complete export may be recorded. A slot whose write did not
        // finish holds neither its old content nor its new one, and an age
        // named against it would let the next repaint paint too little.
        match snapshot.filter(|_| exported) {
            Some(snapshot) => self.history.record(slot, snapshot),
            None => self.history.invalidate(slot),
        }
    }

    pub fn invalidate(&mut self, slot: LiveRendererFrameSlotId) {
        self.history.invalidate(slot);
    }

    /// The history's own counters, for assertions and telemetry.
    pub fn metrics(&self) -> LiveRendererSlotDamageMetrics {
        self.history.metrics()
    }
}

/// Build the per-age repaint table for one slot.
///
/// The renderer discovers the buffer's age; Engine's reducer lives here. So
/// every age the history could answer is reduced up front and the renderer
/// selects, rather than either side calling into the other. `None` means the
/// whole table is full repaints and there is nothing worth sending.
fn slot_repaint_table(
    history: &mut LiveRendererSlotDamageHistory,
    slot: LiveRendererFrameSlotId,
    snapshot: Option<&sophia_engine::OutputFrameDamageSnapshot>,
    size: sophia_protocol::Size,
) -> Option<sophia_renderer_live::NativeCompositionRepaintTable> {
    let depth = history.depth(slot);
    if depth == 0 || snapshot.is_none() {
        return None;
    }
    let mut by_age = Vec::with_capacity(depth);
    let mut any_partial = false;
    for age in 1..=depth {
        let plan = history.plan(
            slot,
            LiveRendererSlotBufferAge::new(u32::try_from(age).unwrap_or(u32::MAX)),
            snapshot,
            size,
        );
        match plan {
            LiveRendererSlotRepaint::Partial { damage } => {
                any_partial = true;
                by_age.push(Some(
                    damage
                        .into_iter()
                        .map(|rect| sophia_renderer_live::NativeCompositionDamageRect {
                            x: rect.x,
                            y: rect.y,
                            width: rect.width,
                            height: rect.height,
                        })
                        .collect(),
                ));
            }
            LiveRendererSlotRepaint::Full { .. } => by_age.push(None),
        }
    }
    any_partial.then(|| {
        sophia_renderer_live::NativeCompositionRepaintTable::from_ages(by_age)
    })
}

/// Notice a rebuilt target bundle and drop what the slot remembered.
///
/// A rebuild takes fresh buffers, so every age the history could name refers to
/// pixels that no longer exist. Comparing the reported generation catches every
/// rebuild path at once, including the ones that happen inside a single export.
fn observe_slot_target_generation(
    history: &mut LiveRendererSlotDamageHistory,
    generations: &mut [Option<u64>; LIVE_RENDERER_FRAME_SLOT_CAPACITY],
    slot: LiveRendererFrameSlotId,
    reported: Option<u64>,
) {
    let Some(slot_generation) = generations.get_mut(slot.index()) else {
        return;
    };
    match (reported, *slot_generation) {
        (Some(reported), Some(previous)) if reported == previous => {}
        (Some(reported), _) => {
            history.invalidate(slot);
            *slot_generation = Some(reported);
        }
        // A render that named no bundle tells us nothing about the buffers, so
        // the safe reading is that what we remembered is gone.
        (None, _) => {
            history.invalidate(slot);
            *slot_generation = None;
        }
    }
}
