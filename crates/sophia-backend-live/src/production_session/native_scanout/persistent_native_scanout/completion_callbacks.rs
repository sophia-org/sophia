impl LiveProductionNativeScanout {
        pub fn observe_callbacks(
            &mut self,
            index: usize,
            report: crate::LivePageFlipCallbackQueueReport,
        ) {
            self.observe_callbacks_with_source(
                index,
                report,
                LiveProductionKmsCompletionSource::PageFlipEvent,
            );
        }

        fn observe_callbacks_with_source(
            &mut self,
            index: usize,
            report: crate::LivePageFlipCallbackQueueReport,
            completion_source: LiveProductionKmsCompletionSource,
        ) {
            self.callback_accepted = self.callback_accepted.saturating_add(report.accepted);
            self.heads[index].callback_accepted = self.heads[index]
                .callback_accepted
                .saturating_add(report.accepted);
            if report.accepted > 0 {
                trace_live_native_lifecycle("page_flip_callback_accepted");
                if completion_source == LiveProductionKmsCompletionSource::PageFlipEvent {
                    tracing::trace!(
                        "sophia_live_native_page_flip schema=1 status=callback_accepted output={} callbacks={} kernel_sequence={}",
                        self.heads[index].output.id.raw(),
                        report.accepted,
                        report
                            .last_accepted
                            .and_then(|accepted| accepted.event.frame_serial)
                            .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                    );
                    tracing::trace!(
                        "sophia_live_native_head_page_flip schema=2 status=callback_accepted output={} head={} callbacks={} kernel_sequence={}",
                        self.heads[index].output.id.raw(),
                        self.heads[index].head.raw(),
                        report.accepted,
                        report
                            .last_accepted
                            .and_then(|accepted| accepted.event.frame_serial)
                            .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                    );
                } else {
                    tracing::info!(
                        "sophia_live_native_completion schema=1 status=accepted output={} head={} callbacks={} completion_source={} completion_serial={}",
                        self.heads[index].output.id.raw(),
                        self.heads[index].head.raw(),
                        report.accepted,
                        completion_source.label(),
                        report
                            .last_accepted
                            .and_then(|accepted| accepted.event.frame_serial)
                            .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                    );
                }
                self.heads[index].last_callback_serial = report
                    .last_accepted
                    .and_then(|accepted| accepted.event.frame_serial);
                if let Some(checksum) = self.heads[index].submitted_checksum.take() {
                    self.heads[index].presented_logical_checksum = checksum;
                }
                if let Some(submission) = self.heads[index].submitted_sequence.take() {
                    self.heads[index].presented_submissions = submission;
                }
                self.heads[index].presented_content = self.heads[index].submitted_content.take();
                self.heads[index].presented_direct =
                    std::mem::take(&mut self.heads[index].submitted_direct);
                if self.heads[index].output_frames.submitted().is_some() {
                    let presented = self.heads[index]
                        .output_frames
                        .mark_presented()
                        .expect("submitted display-list state checked above");
                    trace_presented_output_damage(
                        "presented",
                        self.heads[index].output.id,
                        &presented,
                    );
                }
                let output = self.heads[index].output.id;
                if let Some(kernel_sequence) = report
                    .last_accepted
                    .and_then(|accepted| accepted.event.frame_serial)
                {
                    let timestamp = self.completion_timestamp(
                        output,
                        self.heads[index].head,
                        kernel_sequence,
                        completion_source,
                    );
                    let ust = timestamp.ust_usec;
                    let submitted_ust_usec = self.heads[index].submitted_ust_usec.take();
                    let submit_to_page_flip = submitted_ust_usec
                        .and_then(|submitted| ust.checked_sub(submitted))
                        .map(Duration::from_micros)
                        .or_else(|| {
                            self.heads[index]
                                .submitted_at
                                .map(|submitted| submitted.elapsed())
                        })
                        .unwrap_or_default();
                    self.heads[index].submitted_at = None;
                    self.max_submit_to_page_flip =
                        self.max_submit_to_page_flip.max(submit_to_page_flip);
                    self.heads[index].presented_submission_ust_usec =
                        submitted_ust_usec.unwrap_or_default();
                    self.heads[index].presented_page_flip_ust_usec = ust;
                    self.heads[index].presented_completion_timestamp = Some(timestamp);
                    self.heads[index].presented_submit_to_page_flip = submit_to_page_flip;
                    // What the display engine did with the buffer, filed
                    // under how the buffer got there. This half should not
                    // differ by population, and is measured to find out
                    // rather than to assume.
                    self.cost.record_submit_to_flip(
                        self.heads[index].presented_direct,
                        submit_to_page_flip,
                    );
                    let native_content = self.completed_native_content(index);
                    if let Err(error) = self.production_page_flips.observe_native_page_flip(
                        output,
                        kernel_sequence,
                        ust,
                        native_content,
                    ) {
                        self.page_flip_phase_rejections =
                            self.page_flip_phase_rejections.saturating_add(1);
                        tracing::error!(
                            "sophia_live_native_pacing schema=1 status=completion_rejected output={} kernel_sequence={} completion_source={} ust_usec={} error={error:?}",
                            output.raw(),
                            kernel_sequence,
                            completion_source.label(),
                            ust,
                        );
                    } else if let Some(content) = self.heads[index].presented_content {
                        self.trace_shell_native_completion(output, content.frame());
                    }
                }
            }
            self.callback_rejected = self.callback_rejected.saturating_add(
                report.rejected_unexpected_output + report.rejected_stale_frame_serial,
            );
            self.callback_queue_saturated = self
                .callback_queue_saturated
                .saturating_add(usize::from(report.max_reached));
        }

        fn completion_timestamp(
            &mut self,
            output: OutputId,
            head: sophia_engine::RenderHeadId,
            sequence: u64,
            source: LiveProductionKmsCompletionSource,
        ) -> LiveProductionCompletionTimestamp {
            // Always consume a matching timestamp record. An out-fence is
            // authoritative once selected, so a late kernel event must not
            // leave timing evidence resident after its physical owner retires.
            let kernel_ust = self.kernel_page_flip_ust.remove(&(output, head, sequence));
            let needs_fallback =
                source == LiveProductionKmsCompletionSource::OutFence || kernel_ust.is_none();
            let timestamp = reduce_live_production_completion_timestamp(
                source,
                kernel_ust,
                needs_fallback
                    .then(Self::monotonic_ust_usec)
                    .unwrap_or_default(),
            );
            if timestamp.used_kernel_timestamp {
                self.kernel_page_flip_timestamps =
                    self.kernel_page_flip_timestamps.saturating_add(1);
            }
            if timestamp.missing_kernel_timestamp {
                self.kernel_page_flip_timestamp_missing =
                    self.kernel_page_flip_timestamp_missing.saturating_add(1);
            }
            // Kernel page-flip UST and every fallback share the
            // CLOCK_MONOTONIC epoch. Session-relative elapsed time would jump
            // backward when a head changes to authoritative out-fence
            // completion and would strand the logical presentation owner.
            timestamp
        }

        fn monotonic_ust_usec() -> u64 {
            let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
            let seconds = u64::try_from(now.tv_sec).unwrap_or_default();
            let nanos = u64::try_from(now.tv_nsec).unwrap_or_default();
            seconds
                .saturating_mul(1_000_000)
                .saturating_add(nanos / 1_000)
        }
}
