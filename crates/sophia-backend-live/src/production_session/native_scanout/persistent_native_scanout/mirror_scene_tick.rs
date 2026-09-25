impl LiveProductionNativeScanout {
        fn run_mirror_group_scene_tick(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            input: CompositorBackendTickInput,
        ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
            let indices = self.head_indices(output);
            if indices.is_empty() {
                return Err("mirror group has no head".into());
            }
            let retirement = self.service_mirror_group_retirement(output, runtime);
            if !retirement.errors.is_empty() {
                return Err(format!(
                    "mirror retirement failed after servicing callbacks and cleanup: {}",
                    retirement.errors.join("; ")
                )
                .into());
            }
            // Check stalls only after consuming callbacks that may have arrived
            // at the deadline. Drain-only retirement deliberately omits this
            // scheduler watchdog and relies on its outer bounded timeout.
            self.ensure_page_flip_progress()?;
            // Publish a completed join before the fallible Engine tick. A tick
            // failure must not erase the already-retired callback evidence.
            // Waiting is published later because this tick may start a new group.
            let completed_page_flip = retirement.completed_serial.and_then(|serial| {
                self.publish_mirror_group_page_flip(output, runtime, Some(serial))
            });

            // Advance the logical Engine exactly once. Physical callback owners
            // were retired first, so an Engine failure cannot consume and lose
            // their callback evidence.
            let mut tick = runtime.run_tick(input)?;
            tick.page_flip_callbacks = retirement.page_flip_callbacks;
            if let Some(event) = completed_page_flip {
                tick.page_flip = event;
            }
            let completed_retire = retirement.completed_retire;
            let completed_serial = retirement.completed_serial;
            if let Some(completed_serial) = completed_serial {
                self.finish_mirror_presentation_cohort(
                    output,
                    LiveProductionNativeFrameId::from_raw(completed_serial),
                )?;
            }

            for head_index in indices.iter().copied() {
                if self
                    .output_lifecycles
                    .get(&output)
                    .is_some_and(LiveProductionMirrorGroupLifecycle::failed)
                {
                    // Poison forbids new commits, not ownership cleanup. Visit
                    // every head so a failed later commit cannot strand the
                    // earlier head's retired framebuffer.
                    continue;
                }
                let head_id = self.heads[head_index].head;
                if self.heads[head_index].scanout_custody.submitted().is_some() {
                    self.heads[head_index].scanout_in_flight_ticks = self.heads[head_index]
                        .scanout_in_flight_ticks
                        .saturating_add(1);
                }
                if self.heads[head_index].scanout_custody.cleanup_pending() {
                    continue;
                }
                let already_prepared = self.heads[head_index].prepared_scanout.is_some();
                if !already_prepared && !self.exporters[head_index].pending_frame() {
                    continue;
                }
                let worker_was_in_flight = if already_prepared {
                    self.heads[head_index].prepared_worker_was_in_flight
                } else {
                    self.exporters[head_index].worker_in_flight()
                };
                let work_frame = if already_prepared {
                    self.heads[head_index].prepared_group_frame
                } else {
                    live_production_mirror_head_work_frame(
                        worker_was_in_flight,
                        self.heads[head_index].rendering_content,
                        self.heads[head_index].pending_content,
                    )
                }
                .ok_or("mirror head has renderer work without frame identity")?;
                let newest_frame = self
                    .output_lifecycles
                    .get(&output)
                    .and_then(LiveProductionMirrorGroupLifecycle::active_frame)
                    .ok_or("mirror renderer work has no ready generation")?;
                let logical_frame = work_frame;
                let selection = self.heads[head_index].selection;
                let size = self.heads[head_index].output.size;
                let head_group = self.heads[head_index].group;
                let expected_identity =
                    self.native_frame_identity(head_index, output, logical_frame);
                if let Some(prepared) = self.heads[head_index].prepared_scanout.as_ref()
                    && prepared.correlation().and_then(|value| value.native)
                        != Some(expected_identity)
                {
                    return Err(
                        "prepared mirror owner does not match its current head/generation".into(),
                    );
                }
                let submit = if let Some(prepared) = self.heads[head_index].prepared_scanout.take()
                {
                    if logical_frame != newest_frame {
                        let worker_owned = self.heads[head_index].prepared_worker_was_in_flight;
                        if !self.cancel_prepared_head_owner(head_index, prepared) {
                            continue;
                        }
                        if worker_owned {
                            self.heads[head_index].rendering_content = None;
                            self.heads[head_index].output_frames.discard_rendering();
                        }
                        if let Some(cohort) = self.output_cohorts.get_mut(&(output, logical_frame))
                        {
                            let _ = cohort.mark_skipped(head_id);
                        }
                        continue;
                    }
                    if self.heads[head_index].scanout_custody.submitted().is_some()
                        || self.heads[head_index].scanout_custody.cleanup_pending()
                    {
                        self.heads[head_index].prepared_scanout = Some(prepared);
                        continue;
                    }
                    if !self
                        .output_cohorts
                        .get(&(output, logical_frame))
                        .is_some_and(sophia_engine::OutputPresentationCohort::all_prepared)
                    {
                        self.heads[head_index].prepared_scanout = Some(prepared);
                        continue;
                    }
                    let mut result = crate::submit_prepared_rendered_primary_plane_scanout(
                        self.groups[head_group].session.card(),
                        prepared,
                    );
                    if let Some(submission) = result.submission.take() {
                        let callback_baseline = self.heads[head_index].last_callback_serial;
                        self.heads[head_index]
                            .scanout_custody
                            .accept_submission(
                                submission
                                    .with_submitted_after_page_flip_serial(callback_baseline)
                                    .map_scanout_buffer(|owner| {
                                        Box::new(owner) as Box<dyn std::any::Any>
                                    }),
                            )
                            .expect("mirror submission capacity checked before device call");
                    }
                    if let Some(cleanup) = result.cleanup.take() {
                        self.heads[head_index]
                            .scanout_custody
                            .accept_cleanup(cleanup.map_scanout_buffer(|owner| {
                                Box::new(owner) as Box<dyn std::any::Any>
                            }))
                            .expect("mirror cleanup capacity checked before device call");
                    }
                    mirror_tracked_submit_report(&result, size)
                } else {
                    if !self.output_cohorts.contains_key(&(output, logical_frame)) {
                        let primary = self
                            .output_lifecycles
                            .get(&output)
                            .map(LiveProductionMirrorGroupLifecycle::primary_head)
                            .ok_or("mirror generation has no configured primary head")?;
                        let cohort = sophia_engine::OutputPresentationCohort::new(
                            output,
                            logical_frame.raw(),
                            primary,
                            indices.iter().map(|index| self.heads[*index].head),
                        )
                        .ok_or("mirror generation could not create a preparation cohort")?;
                        self.output_cohorts.insert((output, logical_frame), cohort);
                    }
                    // Each mirror head carries its own cursor contribution:
                    // the pointer projects differently per head, and a head
                    // it is not on hides in this same commit.
                    let cursor_ride = self.arm_cursor_ride(head_index);
                    if let Some((_, placement)) = cursor_ride {
                        self.heads[head_index].prepared_cursor_ride = Some(placement);
                    }
                    let mut prepare = {
                        let device = self.groups[head_group].session.card();
                        let exporter = &mut self.exporters[head_index];
                        crate::prepare_rendered_primary_plane_scanout_from_target_and_selection_with_cursor(
                            crate::LiveKmsScanoutTargetStatus::Ready,
                            Some(crate::LiveGbmEglFrameTargetRecord::new(size)),
                            crate::LibdrmNativePrimaryPlaneSelectionResult {
                                status: crate::LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                                selection: Some(selection),
                            },
                            None,
                            cursor_ride.map(|(cursor, _)| cursor),
                            device,
                            exporter,
                        )
                    };
                    let report = mirror_tracked_prepare_report(&prepare, size);
                    if let Some(cleanup) = prepare.cleanup.take() {
                        self.heads[head_index]
                            .scanout_custody
                            .accept_cleanup(cleanup.map_scanout_buffer(|owner| {
                                Box::new(owner) as Box<dyn std::any::Any>
                            }))
                            .expect("mirror cleanup capacity checked before device call");
                    }
                    if let Some(prepared) = prepare.prepared.take() {
                        if prepared.correlation().and_then(|value| value.native)
                            != Some(expected_identity)
                        {
                            self.cancel_prepared_head_owner(head_index, prepared);
                            return Err(
                                "renderer prepared a different mirror frame identity".into()
                            );
                        }
                        if logical_frame != newest_frame {
                            if !self.cancel_prepared_head_owner(head_index, prepared) {
                                continue;
                            }
                            if worker_was_in_flight {
                                self.heads[head_index].rendering_content = None;
                                self.heads[head_index].output_frames.discard_rendering();
                            }
                            if let Some(cohort) =
                                self.output_cohorts.get_mut(&(output, logical_frame))
                            {
                                let _ = cohort.mark_skipped(head_id);
                            }
                            tracing::info!(
                                "sophia_live_mirror_pacing schema=1 status=coalesced output={} head={} skipped={} newest={}",
                                output.raw(),
                                head_id.raw(),
                                logical_frame.raw(),
                                newest_frame.raw(),
                            );
                            continue;
                        }
                        let content = if worker_was_in_flight {
                            self.heads[head_index].rendering_content
                        } else {
                            self.heads[head_index].pending_content
                        };
                        let Some(content) = content else {
                            self.cancel_prepared_head_owner(head_index, prepared);
                            return Err("prepared mirror head lost its content identity".into());
                        };
                        let logical_content_checksum = content
                            .cpu_checksum()
                            .unwrap_or(self.heads[head_index].last_checksum);
                        let candidate = sophia_engine::HeadFrameCandidate {
                            candidate: self.allocate_head_candidate_id(),
                            output,
                            scene_generation: logical_frame.raw(),
                            head: head_id,
                            target_generation: self.heads[head_index].target_generation,
                            logical_content_checksum,
                        };
                        let transition = self
                            .output_cohorts
                            .get_mut(&(output, logical_frame))
                            .expect("mirror preparation cohort exists")
                            .mark_prepared(candidate);
                        if !matches!(
                            transition,
                            sophia_engine::OutputPresentationTransition::Accepted
                                | sophia_engine::OutputPresentationTransition::PhaseReady
                        ) {
                            self.cancel_prepared_head_owner(head_index, prepared);
                            return Err(format!(
                                "mirror head {} entered invalid prepared transition {transition:?}",
                                head_id.raw(),
                            )
                            .into());
                        }
                        self.heads[head_index].prepared_scanout = Some(prepared);
                        self.heads[head_index].prepared_group_frame = Some(logical_frame);
                        self.heads[head_index].prepared_worker_was_in_flight = worker_was_in_flight;
                        self.heads[head_index].last_submit_report = Some(report);
                        self.output_lifecycles
                            .get_mut(&output)
                            .expect("mirror output has a lifecycle")
                            .observe_physical_progress(logical_frame);
                        tracing::trace!(
                            "sophia_live_native_head_page_flip schema=2 status=prepared output={} head={} frame={} all_prepared={}",
                            output.raw(),
                            head_id.raw(),
                            logical_frame.raw(),
                            self.output_cohorts
                                .get(&(output, logical_frame))
                                .is_some_and(sophia_engine::OutputPresentationCohort::all_prepared),
                        );
                        if tick.rendered_primary_plane_scanout_submit.is_none() {
                            tick.rendered_primary_plane_scanout_submit = Some(report);
                        }
                        continue;
                    }
                    report
                };
                self.heads[head_index].last_submit_report = Some(submit);
                use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status;
                let worker_is_in_flight = self.exporters[head_index].worker_in_flight();
                let renderer_started = {
                    let head = &mut self.heads[head_index];
                    advance_live_production_renderer_content(
                        worker_was_in_flight,
                        worker_is_in_flight,
                        &mut head.pending_content,
                        &mut head.rendering_content,
                    )?
                };
                if renderer_started && self.heads[head_index].output_frames.pending().is_some() {
                    self.heads[head_index]
                        .output_frames
                        .mark_rendering()
                        .map_err(|error| {
                            format!("mirror display-list render transition failed: {error}")
                        })?;
                    let progressed = self
                        .output_lifecycles
                        .get_mut(&output)
                        .expect("mirror output has a lifecycle")
                        .observe_physical_progress(logical_frame);
                    debug_assert!(progressed);
                }
                match submit.status {
                    Status::SubmittedWaitingForPageFlip => {
                        self.heads[head_index].prepared_group_frame = None;
                        self.heads[head_index].prepared_worker_was_in_flight = false;
                        let pending_before = self.heads[head_index].pending_content;
                        let rendering_before = self.heads[head_index].rendering_content;
                        let content = if worker_was_in_flight {
                            self.heads[head_index].rendering_content.take()
                        } else {
                            self.heads[head_index].pending_content.take()
                        };
                        if content.is_none() {
                            return Err(format!(
                                "accepted mirror submission lost its content identity: \
output={} head={} selected_slot={} worker_was_in_flight={} worker_is_in_flight={} \
pending_before={pending_before:?} rendering_before={rendering_before:?} exporter_pending={}",
                                output.raw(),
                                head_id.raw(),
                                if worker_was_in_flight {
                                    "rendering"
                                } else {
                                    "pending"
                                },
                                worker_was_in_flight,
                                worker_is_in_flight,
                                self.exporters[head_index].pending_frame(),
                            )
                            .into());
                        }
                        let content = content.map(|content| {
                            content.with_nonzero_rgb_pixels(
                                self.exporters[head_index].composition_nonzero_rgb_pixels(),
                            )
                        });
                        if worker_was_in_flight
                            && self.heads[head_index].output_frames.rendering().is_some()
                        {
                            self.heads[head_index]
                                .output_frames
                                .promote_rendering_to_submitted()
                                .map_err(|error| {
                                    format!("mirror display-list worker promotion failed: {error}")
                                })?;
                        } else if !worker_was_in_flight
                            && self.heads[head_index].output_frames.pending().is_some()
                        {
                            self.heads[head_index]
                                .output_frames
                                .mark_submitted()
                                .map_err(|error| {
                                    format!("mirror display-list submit failed: {error}")
                                })?;
                        }
                        self.heads[head_index].submitted_content = content;
                        self.heads[head_index].submitted_group_frame = Some(logical_frame);
                        self.heads[head_index].submissions =
                            self.heads[head_index].submissions.saturating_add(1);
                        self.heads[head_index].submitted_sequence =
                            Some(self.heads[head_index].submissions);
                        self.heads[head_index].submitted_checksum = Some(
                            content
                                .and_then(LiveProductionScanoutContent::cpu_checksum)
                                .unwrap_or(self.heads[head_index].last_checksum),
                        );
                        self.heads[head_index].submitted_at = Some(Instant::now());
                        if let Some(placement) = self.heads[head_index].prepared_cursor_ride.take()
                        {
                            if submit.cursor_dropped {
                                self.cursor_combined_drops =
                                    self.cursor_combined_drops.saturating_add(1);
                            } else {
                                self.settle_atomic_cursor(head_index, placement, true);
                            }
                        }
                        self.heads[head_index].submitted_ust_usec =
                            Some(Self::monotonic_ust_usec());
                        self.submissions = self.submissions.saturating_add(1);
                        let exported_nonzero =
                            matches!(content, Some(LiveProductionScanoutContent::Cpu { .. }))
                                && self.heads[head_index].pending_nonzero_pixel_bytes > 0
                                || matches!(
                                    content,
                                    Some(
                                        LiveProductionScanoutContent::MixedPresent {
                                            nonzero_rgb_pixels: 1..,
                                            ..
                                        } | LiveProductionScanoutContent::RetainedMixed {
                                            nonzero_rgb_pixels: 1..,
                                            ..
                                        } | LiveProductionScanoutContent::HeadComposition {
                                            nonzero_rgb_pixels: 1..,
                                            ..
                                        }
                                    )
                                );
                        if exported_nonzero {
                            self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                            self.heads[head_index].nonzero_exports =
                                self.heads[head_index].nonzero_exports.saturating_add(1);
                        }
                        if matches!(content, Some(LiveProductionScanoutContent::Cpu { .. })) {
                            self.heads[head_index].pending_nonzero_pixel_bytes = 0;
                        }
                        tracing::trace!(
                            "sophia_live_native_head_page_flip schema=2 status=submitted output={} head={} submission={} content={:?} frame={}",
                            output.raw(),
                            head_id.raw(),
                            self.heads[head_index].submissions,
                            content,
                            logical_frame.raw(),
                        );
                        let cohort_transition = self
                            .output_cohorts
                            .get_mut(&(output, logical_frame))
                            .ok_or("submitted mirror generation has no preparation cohort")?
                            .mark_submitted(head_id);
                        if !matches!(
                            cohort_transition,
                            sophia_engine::OutputPresentationTransition::Accepted
                                | sophia_engine::OutputPresentationTransition::PhaseReady
                        ) {
                            return Err(format!(
                                "mirror-head {} entered invalid cohort submit transition {cohort_transition:?}",
                                head_id.raw(),
                            )
                            .into());
                        }
                        let transition = self
                            .output_lifecycles
                            .get_mut(&output)
                            .expect("mirror output has a lifecycle")
                            .mark_submitted(head_id, logical_frame);
                        match transition {
                            LiveProductionMirrorHeadTransition::GroupReady => {
                                let cycle = logical_frame.raw();
                                if let Err(error) = self.production_page_flips.submit(output, cycle)
                                {
                                    self.vsync_overlap_rejections =
                                        self.vsync_overlap_rejections.saturating_add(1);
                                    return Err(format!(
                                        "mirror logical page-flip submission was rejected: {error:?}"
                                    )
                                    .into());
                                }
                                tick.rendered_primary_plane_scanout_submit = Some(submit);
                            }
                            LiveProductionMirrorHeadTransition::Accepted => {}
                            invalid => {
                                return Err(format!(
                                    "mirror-head {} entered invalid submitted transition {invalid:?}",
                                    head_id.raw(),
                                )
                                .into());
                            }
                        }
                    }
                    Status::ScanoutExportPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                        // The logical Present owns this generation as soon as any
                        // physical exporter starts. Returning `None` leaves the
                        // Present queued and lets the next Ready pass replace the
                        // frame whose worker is still running.
                        if tick.rendered_primary_plane_scanout_submit.is_none() {
                            tick.rendered_primary_plane_scanout_submit = Some(submit);
                        }
                    }
                    Status::AlreadyInFlight | Status::CleanupPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                    }
                    _ => {
                        if worker_was_in_flight {
                            self.heads[head_index].output_frames.discard_rendering();
                            self.heads[head_index].rendering_content = None;
                        } else {
                            self.heads[head_index].output_frames.discard_pending();
                            self.heads[head_index].pending_content = None;
                        }
                        self.submit_failures = self.submit_failures.saturating_add(1);
                        let cohort_failure = if self
                            .output_cohorts
                            .get(&(output, logical_frame))
                            .is_some_and(sophia_engine::OutputPresentationCohort::all_prepared)
                        {
                            sophia_engine::OutputPresentationFailure::Submission
                        } else {
                            sophia_engine::OutputPresentationFailure::Preparation
                        };
                        if let Some(cohort) = self.output_cohorts.get_mut(&(output, logical_frame))
                        {
                            cohort.fail(cohort_failure);
                        }
                        for prepared_index in indices.iter().copied() {
                            if let Some(prepared) =
                                self.heads[prepared_index].prepared_scanout.take()
                            {
                                self.cancel_prepared_head_owner(prepared_index, prepared);
                            }
                        }
                        tracing::error!(
                            "sophia_live_native_head_page_flip schema=2 status=submit_failed output={} head={} submit_status={:?} action=terminate_session",
                            output.raw(),
                            head_id.raw(),
                            submit.status,
                        );
                        let aborted = self
                            .output_lifecycles
                            .get_mut(&output)
                            .expect("mirror output has a lifecycle")
                            .abort(logical_frame);
                        if !aborted {
                            return Err(
                                "mirror submit failure could not poison its generation".into()
                            );
                        }
                        break;
                    }
                }
            }
            if self
                .output_lifecycles
                .get(&output)
                .is_some_and(|lifecycle| {
                    lifecycle.active_generation_hard_stalled(LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL)
                })
            {
                let blockers = indices
                    .iter()
                    .map(|index| {
                        let head = &self.heads[*index];
                        format!(
                            "head={} kms={} cleanup={} worker={} pending={:?} rendering={:?} newest={:?}",
                            head.head.raw(),
                            head.scanout_custody.submitted().is_some(),
                            head.scanout_custody.cleanup_pending(),
                            self.exporters[*index].worker_in_flight(),
                            head.pending_content.map(LiveProductionScanoutContent::frame),
                            head.rendering_content.map(LiveProductionScanoutContent::frame),
                            self.output_lifecycles
                                .get(&output)
                                .and_then(LiveProductionMirrorGroupLifecycle::active_frame),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(format!(
                    "mirror group generation made no physical progress within {:?}: output={} active={:?} blockers=[{}]",
                    LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL,
                    output.raw(),
                    self.output_lifecycles
                        .get(&output)
                        .and_then(LiveProductionMirrorGroupLifecycle::active_frame),
                    blockers,
                )
                .into());
            }
            tick.rendered_primary_plane_scanout_retire = completed_retire;
            if completed_page_flip.is_none()
                && let Some(logical_page_flip) =
                    self.publish_mirror_group_page_flip(output, runtime, completed_serial)
            {
                tick.page_flip = logical_page_flip;
            }
            tick.rendered_primary_plane_scanout_cleanup_pending = indices
                .iter()
                .any(|index| self.heads[*index].scanout_custody.cleanup_pending());
            tick.rendered_primary_plane_scanout_in_flight_ticks = self
                .heads
                .iter()
                .enumerate()
                .filter(|(index, _)| indices.contains(index))
                .map(|(_, head)| head.scanout_in_flight_ticks)
                .max()
                .unwrap_or_default();
            Ok(tick)
        }
}
