impl LiveProductionNativeScanout {
        pub fn retire_ready(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            if self.head_indices(output).len() > 1 {
                // Retirement is physical ownership work. In particular it must
                // not invent an empty compositor input and re-enter scene
                // projection: an output can have committed surfaces while a
                // page-flip poll has no layer templates to contribute. The frame
                // service will schedule any promoted successor through `run_tick`
                // after this callback-only phase returns.
                let retirement = self.service_mirror_group_retirement(output, runtime);
                if !retirement.errors.is_empty() {
                    return Err(format!(
                        "mirror retirement failed after servicing callbacks and cleanup: {}",
                        retirement.errors.join("; ")
                    )
                    .into());
                }
                self.publish_mirror_group_page_flip(output, runtime, retirement.completed_serial);
                if let Some(completed_serial) = retirement.completed_serial {
                    self.finish_mirror_presentation_cohort(
                        output,
                        LiveProductionNativeFrameId::from_raw(completed_serial),
                    )?;
                }
                if self.mirror_poison_drained(output) {
                    return Err("mirror generation failed after physical ownership drained".into());
                }
                return Ok(());
            }
            let index = self.primary_head(output)?;
            let group = self.heads[index].group;
            self.arm_singleton_retirement(index, output, runtime);
            let mut callbacks = crate::LivePageFlipCallbackQueueReport::with_accepted_capacity(1);
            let completion = self.heads[index]
                .pending_callback
                .take()
                .map(|callback| (callback, LiveProductionKmsCompletionSource::PageFlipEvent));
            let completion = if completion.is_some() {
                completion
            } else {
                match runtime.rendered_primary_plane_completion_fence_status_for(output) {
                    Ok(status) => {
                        self.heads[index].completion_fence_status = status;
                        if status == crate::LibdrmNativeCompletionFenceStatus::Signaled {
                            Some((
                                self.synthesize_out_fence_callback(index),
                                LiveProductionKmsCompletionSource::OutFence,
                            ))
                        } else {
                            None
                        }
                    }
                    Err(error) => {
                        self.heads[index].completion_fence_errors =
                            self.heads[index].completion_fence_errors.saturating_add(1);
                        return Err(format!("native completion fence poll failed: {error}").into());
                    }
                }
            };
            let mut completion_source = LiveProductionKmsCompletionSource::PageFlipEvent;
            let retire = completion.and_then(|(callback, source)| {
                completion_source = source;
                let observation = runtime.observe_page_flip_callback(callback);
                callbacks.record_observation(callback, observation);
                (observation.decision == crate::LivePageFlipCallbackDecision::Accepted).then(|| {
                    runtime.retire_tracked_rendered_primary_plane_scanout_after_page_flip(
                        self.groups[group].session.card(),
                        &observation,
                    )
                })
            });
            if let Some(retire) = retire {
                self.observe_retire(index, retire);
            }
            self.observe_callbacks_with_source(index, callbacks, completion_source);
            Ok(())
        }

        pub(crate) fn retire_ready_for_drain(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.invalidate_layout_probes();
            self.service_layout_probe_cleanup();
            if self.head_indices(output).len() > 1 {
                let retirement = self.service_mirror_group_retirement(output, runtime);
                if !retirement.errors.is_empty() {
                    return Err(format!(
                        "mirror drain failed after servicing callbacks and cleanup: {}",
                        retirement.errors.join("; ")
                    )
                    .into());
                }
                self.publish_mirror_group_page_flip(output, runtime, retirement.completed_serial);
                return Ok(());
            }
            self.retire_ready(output, runtime)
        }

        pub fn retire_ready_and_retry_cleanup(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.service_layout_probe_cleanup();
            let index = self.primary_head(output)?;
            self.retire_ready(output, runtime)?;
            if runtime.rendered_primary_plane_scanout_cleanup_pending() {
                let cleanup =
                    runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(self.card(index));
                if !cleanup.cleanup_pending {
                    self.retire_failures = self.retire_failures.saturating_sub(1);
                }
            }
            Ok(())
        }

        fn cancel_prepared_head_owner(
            &mut self,
            head_index: usize,
            prepared: crate::LivePreparedRenderedPrimaryPlaneScanout<
                crate::NativeGbmRenderedScanoutOwner,
            >,
        ) -> bool {
            let group = self.heads[head_index].group;
            match self.heads[head_index]
                .scanout_custody
                .cancel_prepared(self.groups[group].session.card(), prepared)
            {
                Ok(result) => {
                    if !result.released {
                        self.retire_failures = self.retire_failures.saturating_add(1);
                    }
                    self.heads[head_index].prepared_group_frame = None;
                    self.heads[head_index].prepared_worker_was_in_flight = false;
                    true
                }
                Err(prepared) => {
                    self.heads[head_index].prepared_scanout = Some(prepared);
                    false
                }
            }
        }

        pub fn release_displayed_output(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let index = self.primary_head(output)?;
            self.invalidate_layout_probes();
            self.service_layout_probe_cleanup();
            trace_live_native_lifecycle("displayed_scanout_retire_started");
            let retired = runtime.retire_displayed_rendered_primary_plane_scanout(self.card(index));
            let mut runtime_cleanup_pending = retired.cleanup_pending;
            if retired.cleanup_pending {
                trace_live_native_lifecycle("displayed_scanout_cleanup_retry_started");
                let cleanup =
                    runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(self.card(index));
                runtime_cleanup_pending = cleanup.cleanup_pending;
            }
            let mut mirror_cleanup_pending = false;
            for head_index in self.head_indices(output) {
                if let Some(prepared) = self.heads[head_index].prepared_scanout.take() {
                    self.cancel_prepared_head_owner(head_index, prepared);
                }
                let group = self.heads[head_index].group;
                let custody = &mut self.heads[head_index].scanout_custody;
                let blocked = custody
                    .retire_displayed(self.groups[group].session.card())
                    .is_err();
                custody.retry_cleanup(self.groups[group].session.card());
                if blocked
                    || custody.cleanup_pending()
                    || self.heads[head_index].prepared_scanout.is_some()
                {
                    mirror_cleanup_pending = true;
                }
            }
            if runtime_cleanup_pending
                || mirror_cleanup_pending
                || self.layout_probe_cleanup_pending()
            {
                return Err(format!(
                    "persistent displayed scanout cleanup remained pending: runtime={} mirror_heads={}",
                    runtime_cleanup_pending, mirror_cleanup_pending,
                )
                .into());
            }
            trace_live_native_lifecycle("displayed_scanout_owner_released");
            self.deferred_mirror_generations.revoke_output(output);
            for ((cohort_output, frame), _) in self
                .output_cohorts
                .iter()
                .filter(|((cohort_output, _), _)| *cohort_output == output)
            {
                tracing::info!(
                    "sophia_live_mirror_pacing schema=1 status=released output={} frame={}",
                    cohort_output.raw(),
                    frame.raw(),
                );
            }
            self.output_cohorts
                .retain(|(cohort_output, _), _| *cohort_output != output);
            for head_index in self.head_indices(output) {
                self.heads[head_index].displayed_group_frame = None;
            }
            Ok(())
        }

        pub fn cancel_prepared_output(&mut self, output: OutputId) -> usize {
            let mut cancelled = 0usize;
            for head_index in self.head_indices(output) {
                let Some(prepared) = self.heads[head_index].prepared_scanout.take() else {
                    continue;
                };
                if self.cancel_prepared_head_owner(head_index, prepared) {
                    cancelled = cancelled.saturating_add(1);
                }
            }
            if cancelled > 0 {
                for cohort in
                    self.output_cohorts
                        .iter_mut()
                        .filter_map(|((cohort_output, _), cohort)| {
                            (*cohort_output == output).then_some(cohort)
                        })
                {
                    cohort.fail(sophia_engine::OutputPresentationFailure::StaleTopology);
                }
                tracing::info!(
                    "sophia_live_mirror_generation schema=2 status=preparation_cancelled output={} heads={cancelled}",
                    output.raw(),
                );
            }
            cancelled
        }

        pub fn observe_retire(
            &mut self,
            index: usize,
            retire: crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport,
        ) {
            if retire.status == crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::HeadLost
            {
                self.invalidate_layout_probes();
            }
            self.observe_layout_witness_retire(index, retire);
            use crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus as Status;
            match retire.status {
                Status::RetiredAfterPageFlip => {
                    trace_live_native_lifecycle("kms_buffer_retired");
                    let frame = self.heads[index]
                        .submitted_content
                        .or(self.heads[index].presented_content)
                        .map_or(0, |content| content.frame().raw());
                    tracing::trace!(
                        "sophia_live_native_page_flip schema=1 status=retired output={} submission={} frame={}",
                        self.heads[index].output.id.raw(),
                        self.heads[index]
                            .submitted_sequence
                            .unwrap_or(self.heads[index].submissions),
                        frame,
                    );
                    trace_native_head_retirement(
                        self.heads[index].output.id.raw(),
                        self.heads[index].head.raw(),
                        self.heads[index]
                            .submitted_sequence
                            .unwrap_or(self.heads[index].submissions),
                        frame,
                    );
                    self.retirements = self.retirements.saturating_add(1);
                    self.heads[index].retirements = self.heads[index].retirements.saturating_add(1);
                }
                Status::HeadLost => {
                    trace_live_native_lifecycle("kms_buffer_released_after_head_loss");
                    tracing::warn!(
                        "sophia_live_native_page_flip schema=1 status=head_lost output={}",
                        self.heads[index].output.id.raw(),
                    );
                }
                Status::NoSubmission | Status::WaitingForAcceptedPageFlip => {}
                Status::ResourceRetireFailed => {
                    self.retire_failures = self.retire_failures.saturating_add(1);
                }
            }
        }
}
