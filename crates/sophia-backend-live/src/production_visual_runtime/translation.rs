use super::*;

impl LiveProductionVisualRuntime {
    /// Every deadline the owner loop must wake for, soonest first.
    ///
    /// Two sources, deliberately answered together. An animating translation
    /// owes a frame at its deadline, and a Present parked for pacing owes its
    /// client a completion at the head's next refresh. The owner has one wait
    /// to bound and one preemption to decide, so it asks one question; keeping
    /// them apart is what would let a session with no animation sleep through
    /// a parked candidate's tick.
    fn frame_deadlines(&self) -> impl Iterator<Item = Instant> + '_ {
        self.translation_deadlines
            .values()
            .copied()
            .chain(self.present_scheduler.frame_tick_deadline())
    }

    pub fn frame_deadlines_pending(&self) -> bool {
        self.frame_deadlines().next().is_some()
    }

    pub fn frame_deadline_due(&self) -> bool {
        let now = Instant::now();
        self.frame_deadlines().any(|deadline| deadline <= now)
    }

    pub fn frame_deadline_cap_wait(&self, now: Instant, maximum: Duration) -> Duration {
        self.frame_deadlines()
            .map(|deadline| {
                deadline
                    .saturating_duration_since(now)
                    .max(Duration::from_millis(1))
            })
            .min()
            .map_or(maximum, |wait| wait.min(maximum))
    }

    pub fn set_transitions_enabled(&mut self, enabled: bool) {
        self.translations.set_enabled(enabled);
    }

    pub(super) fn translation_time(&self) -> f64 {
        self.translation_origin.elapsed().as_secs_f64()
    }

    pub(super) fn service_translation_frames(
        &mut self,
        native: &mut LiveProductionNativeScanout,
        scene: &LiveProductionCpuScene,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.native_suspended || !native.output_topology_allows_frame_service() {
            return Ok(());
        }
        let now = Instant::now();
        native.set_translation_motion_active(self.translations.active(self.translation_time()));
        let time = self.translation_time();
        // Presentation admission retains buffer ownership. Let its own candidate
        // supply the next frame before issuing an optional retained repaint.
        if self.native_publication_blocked() || self.present_scheduler.has_eligible() {
            return Ok(());
        }
        let due = self
            .translation_deadlines
            .iter()
            .filter_map(|(output, deadline)| {
                (*deadline <= now
                    && native.frame_queue_ready(*output)
                    && !native.pending_frame(*output))
                .then_some(*output)
            })
            .collect::<Vec<_>>();
        if due.is_empty() {
            return Ok(());
        }
        let sources = self.retained_composition_source_set(scene, None)?;
        let mut frames = Vec::new();
        for output in due {
            let Some(viewport) = self.outputs.logical_viewport(output) else {
                self.translation_deadlines.remove(&output);
                continue;
            };
            let list = self.display_list_for_output(
                output,
                viewport,
                &sources.committed,
                &sources.presentation_order,
            )?;
            let heads = self.compose_native_head_frames_from_sources(
                native,
                output,
                &sources.committed,
                list,
                sources.scene_generation,
                &sources.sources,
            )?;
            frames.push((output, heads));
            if self.translations.active_on(output, time) {
                let refresh = native
                    .head_render_targets(output)
                    .first()
                    .map_or(60_000, |h| h.refresh_millihz)
                    .max(1);
                self.translation_deadlines.insert(
                    output,
                    now + Duration::from_nanos(1_000_000_000_000 / u64::from(refresh)),
                );
            } else {
                self.translation_deadlines.remove(&output);
                tracing::debug!(
                    event = "translation_settled",
                    output = output.raw(),
                    "queued final translation frame"
                );
            }
        }
        native.queue_retained_output_head_composition_frames(frames)?;
        Ok(())
    }
}
