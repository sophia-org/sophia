impl LiveProductionNativeScanout {
        /// How many renderer threads this session runs.
        ///
        /// One per card group when sharing is on, one per enabled head when
        /// it is off. The count is evidence: it is the difference the row
        /// exists to make, and a session claiming to share while running a
        /// thread per head would look identical everywhere else.
        /// What the direct scanout path did this session, summed over heads.
        ///
        /// Summed at report time rather than accumulated per tick because the
        /// exporters own the counts and a head that comes and goes takes its
        /// history with it; a session-level mirror would have to be kept
        /// correct across every topology change to say the same thing.
        /// The output whose head has flipped a client buffer directly, if any.
        ///
        /// Named so a development control can put an overlay on the screen
        /// that is actually scanning one, rather than on whichever output
        /// happens to be first.
        pub fn direct_scanout_output(&self) -> Option<OutputId> {
            std::iter::zip(&self.heads, &self.exporters)
                .find(|(_, exporter)| exporter.direct_scanout_flips() != 0)
                .map(|(head, _)| head.output.id)
        }

        /// What frames cost, both halves and both populations.
        ///
        /// The submit-to-flip half is recorded here, on the head that
        /// flipped; the offer-to-submit half lives in each exporter, which
        /// is the only place that knows how long its own composition pass
        /// took. They are merged rather than kept apart because the question
        /// -- does a direct frame cost less than a composed one -- is about
        /// the whole path, not either end of it.
        pub fn direct_scanout_cost(&self) -> crate::DirectScanoutCost {
            let mut cost = self.cost.clone();
            for exporter in &self.exporters {
                cost.merge(exporter.cost());
            }
            cost
        }

        pub fn direct_scanout_totals(&self) -> LiveProductionDirectScanoutTotals {
            self.exporters.iter().fold(
                LiveProductionDirectScanoutTotals::default(),
                |totals, exporter| LiveProductionDirectScanoutTotals {
                    attempts: totals
                        .attempts
                        .saturating_add(exporter.direct_scanout_attempts()),
                    flips: totals.flips.saturating_add(exporter.direct_scanout_flips()),
                    tests: totals.tests.saturating_add(exporter.direct_scanout_tests()),
                    test_rejections: totals
                        .test_rejections
                        .saturating_add(exporter.direct_scanout_test_rejections()),
                    refusals: totals
                        .refusals
                        .saturating_add(exporter.direct_scanout_refusals()),
                    unsupported: totals
                        .unsupported
                        .saturating_add(exporter.direct_scanout_unsupported()),
                    fallbacks: totals
                        .fallbacks
                        .saturating_add(exporter.direct_scanout_fallbacks()),
                },
            )
        }

        /// How many lowered frames carried each direct-scanout verdict, per
        /// head, indexed as `DirectScanoutVerdict::VERDICTS`.
        ///
        /// Per head rather than summed, because a session's heads answer
        /// differently and the sum hides it: a head with no client contributes
        /// its blank frames to the same column as a head whose client is one
        /// layer short, and reading that total sends someone to the wrong
        /// screen.
        pub fn direct_scanout_head_verdicts(
            &self,
        ) -> Vec<(
            OutputId,
            sophia_engine::RenderHeadId,
            [usize; sophia_engine::DirectScanoutVerdict::COUNT],
        )> {
            self.exporters
                .iter()
                .enumerate()
                .filter_map(|(index, exporter)| {
                    let head = self.heads.get(index)?;
                    Some((
                        head.output.id,
                        head.head,
                        exporter.direct_scanout_verdicts(),
                    ))
                })
                .collect()
        }

        /// The same, summed over heads.
        pub fn direct_scanout_verdicts(
            &self,
        ) -> [usize; sophia_engine::DirectScanoutVerdict::COUNT] {
            self.exporters.iter().fold(
                [0usize; sophia_engine::DirectScanoutVerdict::COUNT],
                |mut totals, exporter| {
                    for (total, count) in
                        std::iter::zip(&mut totals, exporter.direct_scanout_verdicts())
                    {
                        *total = total.saturating_add(count);
                    }
                    totals
                },
            )
        }

        /// Let heads take the direct path, now that startup readiness has
        /// proven a picture reached glass.
        ///
        /// Before that the barrier has no evidence to read: it measures
        /// composed pixels, and a direct frame is never composed. A session
        /// that flipped immediately could put a client on screen and still
        /// time out claiming nothing was presented, which is exactly what one
        /// did.
        pub fn admit_direct_scanout(&mut self) {
            self.direct_scanout_admitted = true;
            if !self.direct_scanout_admissible {
                return;
            }
            for index in 0..self.exporters.len() {
                let mirrored = self.head_indices(self.heads[index].output.id).len() > 1;
                self.exporters[index]
                    .set_direct_scanout_enabled(!mirrored && !self.translation_motion_active);
            }
        }

        pub fn set_translation_motion_active(&mut self, active: bool) {
            if self.translation_motion_active == active {
                return;
            }
            self.translation_motion_active = active;
            self.invalidate_layout_probes();
            for index in 0..self.exporters.len() {
                let mirrored = self.head_indices(self.heads[index].output.id).len() > 1;
                self.exporters[index].set_direct_scanout_enabled(
                    !active
                        && self.direct_scanout_admitted
                        && self.direct_scanout_admissible
                        && !mirrored,
                );
            }
        }

        pub fn renderer_worker_count(&self) -> usize {
            if Self::shared_renderer_worker_enabled() {
                self.groups
                    .iter()
                    .filter(|group| group.renderer_core.is_some())
                    .count()
            } else {
                self.exporters
                    .iter()
                    .enumerate()
                    .filter(|(index, exporter)| {
                        self.heads[*index].enabled && exporter.worker_enabled()
                    })
                    .count()
            }
        }

        pub fn enabled_head_count(&self) -> usize {
            self.heads.iter().filter(|head| head.enabled).count()
        }
}
