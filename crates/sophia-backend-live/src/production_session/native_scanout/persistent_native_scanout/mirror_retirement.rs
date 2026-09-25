impl LiveProductionNativeScanout {
        fn service_mirror_group_retirement(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> LiveProductionMirrorRetirementReport {
            let indices = self.head_indices(output);
            let mut errors = Vec::new();
            // The card pump has already routed at most one completion into
            // each head's ledger. Admit those physical callbacks without
            // publishing a per-head logical flip; the group join below is the
            // only publisher of the output-level event.
            let mut page_flip_callbacks =
                crate::LivePageFlipCallbackQueueReport::with_accepted_capacity(indices.len());
            let mut completion_sources = Vec::with_capacity(indices.len());
            for head_index in indices.iter().copied() {
                let completion = self.heads[head_index]
                    .pending_callback
                    .take()
                    .map(|callback| (callback, LiveProductionKmsCompletionSource::PageFlipEvent));
                let completion = if completion.is_some() {
                    completion
                } else {
                    match self.heads[head_index]
                        .scanout_custody
                        .submitted()
                        .map(crate::LiveRenderedPrimaryPlaneScanoutSubmission::completion_fence_status)
                        .transpose()
                    {
                        Ok(status) => {
                            let status = status
                                .unwrap_or(crate::LibdrmNativeCompletionFenceStatus::Unsupported);
                            self.heads[head_index].completion_fence_status = status;
                            if status == crate::LibdrmNativeCompletionFenceStatus::Signaled {
                                Some((
                                    self.synthesize_out_fence_callback(head_index),
                                    LiveProductionKmsCompletionSource::OutFence,
                                ))
                            } else {
                                None
                            }
                        }
                        Err(error) => {
                            self.heads[head_index].completion_fence_errors = self.heads[head_index]
                                .completion_fence_errors
                                .saturating_add(1);
                            errors.push(format!(
                                "mirror head {} completion fence poll failed: {error}",
                                self.heads[head_index].head.raw(),
                            ));
                            None
                        }
                    }
                };
                let Some((callback, source)) = completion else {
                    continue;
                };
                let observation = runtime.observe_mirror_page_flip_callback(callback);
                if observation.decision == crate::LivePageFlipCallbackDecision::Accepted {
                    completion_sources.push(source);
                }
                page_flip_callbacks.record_observation(callback, observation);
            }
            self.callback_rejected = self.callback_rejected.saturating_add(
                page_flip_callbacks.rejected_unexpected_output
                    + page_flip_callbacks.rejected_stale_frame_serial,
            );

            let mut completed_retire = None;
            let mut completed_serial = None;
            for (callback, completion_source) in page_flip_callbacks
                .accepted_callbacks
                .iter()
                .copied()
                .zip(completion_sources)
            {
                let Some(head_index) = self.head_index_for_output_head(output, callback.head)
                else {
                    self.callback_rejected = self.callback_rejected.saturating_add(1);
                    errors.push(format!(
                        "mirror callback referenced unknown head {}",
                        callback.head.raw()
                    ));
                    continue;
                };
                let Some(frame) = self.heads[head_index].submitted_group_frame else {
                    errors.push(format!(
                        "mirror head {} callback has no logical generation",
                        callback.head.raw()
                    ));
                    continue;
                };
                let Some(submission) = self.heads[head_index].scanout_custody.submitted() else {
                    errors.push(format!(
                        "mirror head {} callback has no physical submission",
                        callback.head.raw()
                    ));
                    continue;
                };
                if self.heads[head_index]
                    .last_callback_serial
                    .is_some_and(|serial| callback.frame_serial <= serial)
                {
                    self.callback_rejected = self.callback_rejected.saturating_add(1);
                    continue;
                }
                let expected = self.native_frame_identity(head_index, output, frame);
                if submission.correlation().and_then(|value| value.native) != Some(expected) {
                    errors.push(format!(
                        "mirror head {} submission identity does not match its current generation",
                        callback.head.raw()
                    ));
                    continue;
                }
                let group = self.heads[head_index].group;
                let callback_timestamp = self.completion_timestamp(
                    output,
                    callback.head,
                    callback.frame_serial,
                    completion_source,
                );
                let callback_ust = callback_timestamp.ust_usec;
                let last_callback_serial = self.heads[head_index].last_callback_serial;
                let completion = mirror_completion::complete_mirror_head(
                    self.groups[group].session.card(),
                    &mut self.heads[head_index].scanout_custody,
                    self.output_lifecycles
                        .get_mut(&output)
                        .expect("mirror lifecycle"),
                    self.output_cohorts.get_mut(&(output, frame)),
                    mirror_completion::MirrorCompletionWitness {
                        expected,
                        callback,
                        last_callback_serial,
                        ust_usec: callback_ust,
                    },
                );
                let presented = completion.physical;
                let crate::PersistentFlipOutcome::Presented {
                    correlation,
                    previous_cleanup,
                    ..
                } = presented
                else {
                    errors.push(format!(
                        "mirror head {} retained unprocessed completion: {presented:?}",
                        callback.head.raw()
                    ));
                    continue;
                };
                debug_assert_eq!(correlation.and_then(|value| value.native), Some(expected));
                self.heads[head_index].last_callback_serial = Some(callback.frame_serial);
                let submitted_ust_usec = self.heads[head_index].submitted_ust_usec.take();
                let submit_to_page_flip = submitted_ust_usec
                    .and_then(|submitted| callback_ust.checked_sub(submitted))
                    .map(Duration::from_micros)
                    .or_else(|| {
                        self.heads[head_index]
                            .submitted_at
                            .map(|submitted| submitted.elapsed())
                    })
                    .unwrap_or_default();
                self.max_submit_to_page_flip =
                    self.max_submit_to_page_flip.max(submit_to_page_flip);
                self.heads[head_index].presented_submission_ust_usec =
                    submitted_ust_usec.unwrap_or_default();
                self.heads[head_index].presented_page_flip_ust_usec = callback_ust;
                self.heads[head_index].presented_completion_timestamp = Some(callback_timestamp);
                self.heads[head_index].presented_submit_to_page_flip = submit_to_page_flip;
                // A mirror head composes by construction -- eligibility
                // requires a single-head plan shape -- so this is recorded
                // as composed rather than asked.
                self.cost.record_submit_to_flip(false, submit_to_page_flip);
                self.heads[head_index].submitted_group_frame = None;
                self.heads[head_index].displayed_group_frame = Some(frame);
                if let Some(cleanup) = previous_cleanup
                    && cleanup.released
                    && let Some(identity) = cleanup.correlation.and_then(|value| value.native)
                    && let Some(cohort) = self.output_cohorts.get_mut(&(
                        identity.output(),
                        LiveProductionNativeFrameId::from_raw(identity.frame()),
                    ))
                {
                    let _ = cohort.mark_cleanup_complete(identity.head());
                }
                self.heads[head_index].submitted_at = None;
                self.heads[head_index].scanout_in_flight_ticks = 0;
                self.heads[head_index].retirements =
                    self.heads[head_index].retirements.saturating_add(1);
                self.heads[head_index].callback_accepted =
                    self.heads[head_index].callback_accepted.saturating_add(1);
                self.retirements = self.retirements.saturating_add(1);
                self.callback_accepted = self.callback_accepted.saturating_add(1);
                self.heads[head_index].presented_content =
                    self.heads[head_index].submitted_content.take();
                self.heads[head_index].presented_logical_checksum = self.heads[head_index]
                    .submitted_checksum
                    .take()
                    .unwrap_or_default();
                if let Some(submission) = self.heads[head_index].submitted_sequence.take() {
                    self.heads[head_index].presented_submissions = submission;
                }
                if self.heads[head_index].output_frames.submitted().is_some() {
                    match self.heads[head_index].output_frames.mark_presented() {
                        Ok(presented) => {
                            trace_presented_output_damage(
                                "presented",
                                self.heads[head_index].output.id,
                                &presented,
                            );
                            trace_presented_mirror_head_damage(
                                output,
                                callback.head,
                                frame,
                                &presented,
                            );
                        }
                        Err(error) => errors.push(format!(
                            "mirror display-list presentation transition failed: {error}"
                        )),
                    }
                }
                if let Some(transition) = completion.cohort
                    && !matches!(
                        transition,
                        sophia_engine::OutputPresentationTransition::Accepted
                            | sophia_engine::OutputPresentationTransition::PhaseReady
                    )
                {
                    errors.push(format!(
                        "mirror head {} entered invalid cohort flip transition {transition:?}",
                        callback.head.raw(),
                    ));
                }
                tracing::info!(
                    "sophia_live_native_head_completion schema=1 status=accepted output={} head={} callbacks=1 completion_source={} completion_serial={} frame={}",
                    output.raw(),
                    callback.head.raw(),
                    completion_source.label(),
                    callback.frame_serial,
                    frame.raw(),
                );
                trace_native_head_retirement(
                    output.raw(),
                    callback.head.raw(),
                    self.heads[head_index].presented_submissions,
                    frame.raw(),
                );
                if self
                    .output_lifecycles
                    .get(&output)
                    .is_some_and(LiveProductionMirrorGroupLifecycle::failed)
                {
                    continue;
                }
                if !completion.timing_valid {
                    errors.push(format!(
                        "mirror head {} callback timing named the wrong generation",
                        callback.head.raw()
                    ));
                    continue;
                }
                self.trace_shell_native_completion(output, frame);
                let Some(transition) = completion.logical else {
                    continue;
                };
                match transition {
                    LiveProductionMirrorHeadTransition::GroupReady => {
                        let Some((logical_serial, logical_ust)) = self
                            .output_lifecycles
                            .get(&output)
                            .and_then(LiveProductionMirrorGroupLifecycle::flip_timing)
                        else {
                            errors.push(
                                "completed mirror generation has no timing evidence".to_owned(),
                            );
                            continue;
                        };
                        let mut presented = true;
                        let native_content =
                            self.heads[head_index].presented_content.map(|content| {
                                LiveProductionNativeRetirementContent {
                                    content,
                                    submission: u64::try_from(
                                        self.heads[head_index].presented_submissions,
                                    )
                                    .unwrap_or(u64::MAX),
                                    direct: false,
                                    layout_witness: None,
                                }
                            });
                        if let Err(error) = self.production_page_flips.observe_native_page_flip(
                            output,
                            logical_serial,
                            logical_ust,
                            native_content,
                        ) {
                            self.page_flip_phase_rejections =
                                self.page_flip_phase_rejections.saturating_add(1);
                            errors.push(format!(
                                "mirror logical page-flip retirement was rejected: {error:?}"
                            ));
                            presented = false;
                        }
                        let presented_content = self.heads[head_index].presented_content;
                        let presented_logical_checksum =
                            self.heads[head_index].presented_logical_checksum;
                        if presented_content.is_none_or(|content| content.frame() != frame) {
                            errors.push(
                                "mirror primary presented the wrong content identity".to_owned(),
                            );
                            presented = false;
                        }
                        if let Some(content) = presented_content
                            && presented
                        {
                            tracing::info!(
                                "sophia_live_mirror_pacing schema=1 status=primary_presented output={} primary={} frame={}",
                                output.raw(),
                                callback.head.raw(),
                                frame.raw(),
                            );
                            tracing::info!(
                                "sophia_live_mirror_generation schema=2 status=presented output={} frame={} source={} logical_content_checksum={}",
                                output.raw(),
                                frame.raw(),
                                content.source_label(),
                                presented_logical_checksum,
                            );
                        }
                        if presented {
                            completed_retire = Some(crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport {
                                status: crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip,
                                layout_witness: None,
                                destroy: None,
                                runtime_scanout_state: Some(crate::RuntimeScanoutState::Retired),
                                in_flight: false,
                                in_flight_ticks: 0,
                                cleanup_pending: self.heads[head_index].scanout_custody.cleanup_pending(),
                            });
                            completed_serial = Some(logical_serial);
                        }
                    }
                    LiveProductionMirrorHeadTransition::Accepted => {}
                    invalid => errors.push(format!(
                        "mirror-head {} entered invalid flipped transition {invalid:?}",
                        callback.head.raw(),
                    )),
                }
            }

            // Cleanup is ownership work, not scheduling. Always visit every
            // head, even when callback processing above found an error.
            for head_index in indices.iter().copied() {
                let group = self.heads[head_index].group;
                if let Some(result) = self.heads[head_index]
                    .scanout_custody
                    .retry_cleanup(self.groups[group].session.card())
                {
                    if !result.released {
                        self.retire_failures = self.retire_failures.saturating_add(1);
                    } else if let Some(identity) = result.correlation.and_then(|value| value.native)
                        && let Some(cohort) = self.output_cohorts.get_mut(&(
                            identity.output(),
                            LiveProductionNativeFrameId::from_raw(identity.frame()),
                        ))
                    {
                        let transition = cohort.mark_cleanup_complete(identity.head());
                        if !matches!(
                            transition,
                            sophia_engine::OutputPresentationTransition::Accepted
                                | sophia_engine::OutputPresentationTransition::PhaseReady
                        ) {
                            errors.push(format!("mirror head {} entered invalid retried cleanup transition {transition:?}", identity.head().raw()));
                        }
                    }
                }
            }

            self.output_cohorts
                .retain(|(cohort_output, frame), cohort| {
                    let releasable = *cohort_output == output && cohort.generation_releasable();
                    if releasable {
                        tracing::info!(
                            "sophia_live_mirror_pacing schema=1 status=released output={} frame={}",
                            output.raw(),
                            frame.raw(),
                        );
                    }
                    !releasable
                });

            LiveProductionMirrorRetirementReport {
                page_flip_callbacks,
                completed_retire,
                completed_serial,
                errors,
            }
        }

        fn publish_mirror_group_page_flip(
            &self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            completed_serial: Option<u64>,
        ) -> Option<crate::LivePageFlipEvent> {
            if completed_serial.is_none()
                && self
                    .output_lifecycles
                    .get(&output)
                    .and_then(LiveProductionMirrorGroupLifecycle::logically_submitted_frame)
                    .is_none()
            {
                return None;
            }
            let event = crate::LivePageFlipEvent {
                status: if completed_serial.is_some() {
                    crate::LivePageFlipEventStatus::Presented
                } else {
                    crate::LivePageFlipEventStatus::WaitingForOutput
                },
                frame_serial: completed_serial,
            };
            runtime.set_page_flip_observation(event);
            Some(event)
        }

        fn finish_mirror_presentation_cohort(
            &mut self,
            output: OutputId,
            frame: LiveProductionNativeFrameId,
        ) -> Result<(), Box<dyn std::error::Error>> {
            if !self
                .output_cohorts
                .get(&(output, frame))
                .is_some_and(|cohort| {
                    matches!(
                        cohort.terminal(),
                        Some(sophia_engine::OutputPresentationTerminal::Presented { .. })
                    )
                })
            {
                return Err("mirror generation joined before its Engine cohort presented".into());
            }
            Ok(())
        }
}
