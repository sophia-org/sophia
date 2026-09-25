impl LiveProductionNativeScanout {
        /// Record how many submissions this output holds right now.
        ///
        /// Sampled per tick rather than incremented at submit: the property is
        /// concurrent depth, and only a reading taken while submissions are
        /// live can observe it.
        fn observe_in_flight_depth(&mut self) {
            let depth = self.head_scanout_in_flight_count();
            self.max_in_flight_per_output = self.max_in_flight_per_output.max(depth);
            let supersessions: usize = self
                .exporters
                .iter()
                .map(|exporter| exporter.pending_frame_supersessions())
                .sum();
            self.pending_frame_supersessions = self.pending_frame_supersessions.max(supersessions);
            self.observe_service_skew();
        }

        /// How far one output's wait ran behind its siblings' service.
        ///
        /// While a head has a request outstanding, every render another head
        /// completes is that head being passed over. Taking the shared queue
        /// in order bounds this at one per sibling, which is the property the
        /// model states and the reason no scheduler is needed; measuring it
        /// is what turns that from an argument into evidence.
        ///
        /// Sampled on the tick rather than hooked at submit and completion,
        /// because the worker cannot see its own queue: a render already
        /// dequeued gives no way to know who was waiting behind it. Two
        /// completions inside one tick therefore read as one, so this is a
        /// lower bound on true skew -- it can miss a peak, never invent one.
        /// The structural guarantee remains FIFO service; this is the check
        /// that the implementation kept it.
        fn observe_service_skew(&mut self) {
            for index in 0..self.exporters.len() {
                if !self.heads[index].enabled {
                    continue;
                }
                if !self.exporters[index].worker_in_flight() {
                    self.heads[index].service_skew_baseline = None;
                    continue;
                }
                let siblings: usize = self
                    .exporters
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .filter_map(|(_, exporter)| exporter.worker_metrics())
                    .map(|metrics| metrics.completions)
                    .sum();
                match self.heads[index].service_skew_baseline {
                    None => self.heads[index].service_skew_baseline = Some(siblings),
                    Some(baseline) => {
                        self.max_service_skew =
                            self.max_service_skew.max(siblings.saturating_sub(baseline));
                    }
                }
            }
        }

        fn synthesize_out_fence_callback(&mut self, index: usize) -> crate::LivePageFlipCallback {
            let serial = self.heads[index]
                .last_callback_serial
                .unwrap_or_default()
                .saturating_add(1);
            self.heads[index].completion_mode =
                LiveProductionKmsCompletionMode::OutFenceAuthoritative;
            self.heads[index].out_fence_retirements =
                self.heads[index].out_fence_retirements.saturating_add(1);
            crate::LivePageFlipCallback {
                output: self.heads[index].output.id,
                head: self.heads[index].head,
                frame_serial: serial,
            }
        }
}
