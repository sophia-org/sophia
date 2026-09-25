impl LiveProductionNativeScanout {
        pub fn take_presentation_feedback(
            &mut self,
            output: OutputId,
        ) -> Option<LiveProductionNativeFrameRetirement> {
            let (retirement, native) = self.production_page_flips.take_native_retirement(output)?;
            let native = native?;
            let content = native.content;
            Some(LiveProductionNativeFrameRetirement {
                output,
                frame: content.frame(),
                submission: retirement.cycle,
                content,
                direct: native.direct,
                layout_witness: native.layout_witness,
                ust: retirement.retirement.ust,
                msc: retirement.retirement.msc,
            })
        }

        pub fn pending_kernel_page_flip_timestamps(&self) -> usize {
            self.kernel_page_flip_ust.len()
        }

        pub fn discard_presentation_feedback(&mut self, output: Option<OutputId>) {
            self.production_page_flips.discard_retirements(output);
        }

        /// Whether any head of this output has KMS work in flight.
        ///
        /// A mirror group submits into per-head slots, so the runtime's single
        /// per-output submission slot stays empty for one and reports Idle. The
        /// frame service reads that phase to decide whether to poll for
        /// retirement, so a grouped output was never polled, its retirements
        /// never consumed, and Present completions never routed -- a silent
        /// freeze on the first content change rather than an error.
        pub fn output_in_flight(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_custody.submitted().is_some())
        }

        /// Whether any head of this output still owes resource cleanup.
        pub fn output_cleanup_pending(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_custody.cleanup_pending())
                || self.output_topology_cleanup.iter().any(|(head, _)| {
                    self.head_index_for_head(*head)
                        .is_some_and(|index| self.heads[index].output.id == output)
                })
        }

        pub fn pending_frame(&self, output: OutputId) -> bool {
            let mirror = self.head_indices(output).len() > 1;
            self.deferred_mirror_generations.pending(output)
                || self.head_indices(output).into_iter().any(|index| {
                    self.exporters[index].pending_frame()
                        || self.heads[index].prepared_scanout.is_some()
                        || self.heads[index].pending_content.is_some()
                        || self.heads[index].rendering_content.is_some()
                        || (!mirror && self.heads[index].scanout_custody.submitted().is_some())
                })
        }

        pub fn is_mirror_output(&self, output: OutputId) -> bool {
            self.head_indices(output).len() > 1
        }

        pub fn primary_scanout_in_flight(&self, output: OutputId) -> bool {
            self.primary_head_index(output)
                .is_some_and(|index| self.heads[index].scanout_custody.submitted().is_some())
        }

        pub fn primary_cleanup_pending(&self, output: OutputId) -> bool {
            self.primary_head_index(output)
                .is_some_and(|index| self.heads[index].scanout_custody.cleanup_pending())
        }

        pub fn frame_queue_ready(&self, output: OutputId) -> bool {
            if self.is_mirror_output(output) {
                return !self.mirror_generation_failed(output);
            }
            !self.pending_frame(output)
                && !self.output_in_flight(output)
                && !self.output_cleanup_pending(output)
        }

        pub fn scanout_in_flight(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_custody.submitted().is_some())
        }

        pub fn scanout_cleanup_pending(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_custody.cleanup_pending())
        }

        pub fn mirror_generation_failed(&self, output: OutputId) -> bool {
            self.output_lifecycles
                .get(&output)
                .is_some_and(LiveProductionMirrorGroupLifecycle::failed)
        }

        fn mirror_poison_drained(&self, output: OutputId) -> bool {
            self.mirror_generation_failed(output)
                && !self.scanout_in_flight(output)
                && !self.scanout_cleanup_pending(output)
        }

        pub fn any_head_scanout_in_flight(&self) -> bool {
            self.heads
                .iter()
                .any(|head| head.scanout_custody.submitted().is_some())
        }

        /// Heads holding a KMS submission the kernel has not retired.
        ///
        /// Keyed on the submitted sequence rather than on the retained mirror
        /// owner. `scanout_submission` exists only on the mirror path, where a
        /// group parks each head's owner until the cohort joins; an ordinary
        /// extended-desktop head never sets it, so counting it reported zero
        /// for every session that was not mirroring -- including the one this
        /// counter exists to describe. The submitted sequence is set at submit
        /// and taken at retirement on both paths, which is exactly the window
        /// "in flight" names.
        pub fn head_scanout_in_flight_count(&self) -> usize {
            self.heads
                .iter()
                .filter(|head| head.submitted_sequence.is_some())
                .count()
        }

        pub fn any_head_cleanup_pending(&self) -> bool {
            !self.output_topology_cleanup.is_empty()
                || self.layout_probe_cleanup_pending()
                || self
                    .heads
                    .iter()
                    .any(|head| head.scanout_custody.cleanup_pending())
        }

        pub fn submitted_content(&self, output: OutputId) -> Option<LiveProductionScanoutContent> {
            let frame = self.submitted_frame(output)?;
            self.head_indices(output).into_iter().find_map(|index| {
                self.heads[index]
                    .submitted_content
                    .filter(|content| content.frame() == frame)
            })
        }

        /// Returns the logical submitted generation independently of which
        /// physical head still owns `submitted_content`.
        ///
        /// During an asymmetric mirror flip the primary may already have moved
        /// its content to `presented_content` while a sibling remains in flight.
        pub fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
            if self.head_indices(output).len() > 1 {
                return self
                    .output_lifecycles
                    .get(&output)
                    .and_then(LiveProductionMirrorGroupLifecycle::logically_submitted_frame);
            }
            self.heads[self.primary_head_index(output)?]
                .submitted_content
                .map(LiveProductionScanoutContent::frame)
        }

        /// Returns the immutable scene snapshot retired by the latest accepted
        /// page flip for this output. Pending, rendering, and submitted work is
        /// intentionally invisible here.
        pub fn presented_output_frame(
            &self,
            output: OutputId,
        ) -> Option<&sophia_engine::OutputFrameDamageSnapshot> {
            self.heads[self.primary_head_index(output)?]
                .output_frames
                .presented()
        }

        pub fn presented_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
            self.heads[self.primary_head_index(output)?]
                .presented_content
                .map(LiveProductionScanoutContent::frame)
        }

        /// Whether an exact logical frame still has a native owner.
        ///
        /// CPU progress uses this after each production/service turn. A frame
        /// remains live while it is deferred, active in a mirror cohort, or held
        /// by any physical-head queue stage. Once it disappears from all of
        /// those cells without retiring, latest-wins supersession is proven.
        pub fn output_owns_frame(
            &self,
            output: OutputId,
            frame: LiveProductionNativeFrameId,
        ) -> bool {
            if self.deferred_mirror_generations.owns(output, frame) {
                return true;
            }
            if self
                .output_lifecycles
                .get(&output)
                .is_some_and(|lifecycle| {
                    lifecycle.active_frame() == Some(frame)
                        || lifecycle.generation_is_scanned(frame)
                })
            {
                return true;
            }
            self.head_indices(output).into_iter().any(|index| {
                let head = &self.heads[index];
                [
                    head.pending_content,
                    head.rendering_content,
                    head.submitted_content,
                    head.presented_content,
                ]
                .into_iter()
                .flatten()
                .any(|content| content.frame() == frame)
                    || head.prepared_group_frame == Some(frame)
                    || head.submitted_group_frame == Some(frame)
                    || head.displayed_group_frame == Some(frame)
            })
        }

        pub fn stable_present(&self, output: OutputId, transaction: TransactionId) -> bool {
            self.primary_head_index(output).is_some_and(|index| {
                live_production_scanout_is_stable_present(
                    self.heads[index].presented_content,
                    transaction,
                )
            })
        }

        pub fn presented_mixed_nonzero_rgb_pixels(&self, transaction: TransactionId) -> usize {
            self.outputs()
                .into_iter()
                .filter_map(|output| {
                    let index = self.primary_head_index(output.id)?;
                    match self.heads[index].presented_content {
                        Some(LiveProductionScanoutContent::MixedPresent {
                            transaction: presented,
                            nonzero_rgb_pixels,
                            ..
                        }) if presented == transaction => Some(nonzero_rgb_pixels),
                        _ => None,
                    }
                })
                .max()
                .unwrap_or(0)
        }
}
