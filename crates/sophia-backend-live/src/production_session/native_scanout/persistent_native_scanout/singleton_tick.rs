impl LiveProductionNativeScanout {
        pub fn run_tick(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            input: CompositorBackendTickInput,
        ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
            self.retry_output_topology_cleanup();
            if !self.output_topology_allows_frame_service() {
                return Err(
                    "ordinary native frame scheduling is quarantined during topology preparation"
                        .into(),
                );
            }
            self.activate_deferred_mirror_generation(output)?;
            if self.head_indices(output).len() > 1 {
                let report = self.run_mirror_group_scene_tick(output, runtime, input)?;
                if self.mirror_poison_drained(output) {
                    return Err("mirror generation failed after physical ownership drained".into());
                }
                // Sampled here too: a mirror output is where concurrent depth
                // is expected to exceed one, so omitting it would leave the
                // measurement blind to the case it exists to describe.
                self.observe_in_flight_depth();
                return Ok(report);
            }
            let index = self.primary_head(output)?;
            if !self.exporter_mut(output)?.pending_frame() {
                self.retire_ready_and_retry_cleanup(output, runtime)?;
                return Ok(runtime.run_tick(input)?);
            }
            self.arm_singleton_retirement(index, output, runtime);
            self.prepare_layout_probe_turn(index);
            let group = self.heads[index].group;
            // Arm the cursor to ride this frame's commit, when one is
            // pending. The request is being built anyway, so the ride costs
            // nothing -- and it is the only way a cursor moves while frames
            // are flowing, since the CRTC is then never free for a
            // cursor-only commit. Settled below only when the submit was
            // accepted; a deferred or failed submission leaves the position
            // pending, because a cursor must never be lost to a frame that
            // did not happen.
            let cursor_ride = self.arm_cursor_ride(index);
            runtime.set_cursor_ride_request(output, cursor_ride.map(|(cursor, _)| cursor));
            let (report, exported_nonzero, worker_was_in_flight, submitted_direct) = {
                let groups = &mut self.groups;
                let head = &mut self.heads[index];
                let exporter = self
                    .exporters
                    .get_mut(index)
                    .ok_or_else(|| format!("native output {} has no exporter", output.raw()))?;
                let worker_was_in_flight = exporter.worker_in_flight();
                let export_attempts_before = exporter.cpu_frame_export_attempts();
                let direct_flips_before = exporter.direct_scanout_flips();
                let report = runtime
                    .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_with(
                        input,
                        groups[group].session.card(),
                        exporter,
                    )?;
                let exported_nonzero = exporter.cpu_frame_export_attempts()
                    > export_attempts_before
                    && head.pending_nonzero_pixel_bytes > 0;
                // Whether the submission this tick produced -- if it produced
                // one -- put the client's own buffer on the plane. Read as a
                // difference rather than a flag because the exporter is
                // several calls away from the submit that consumed its export,
                // and a flag would have to be cleared by whichever of those
                // calls ran last.
                let submitted_direct = exporter.direct_scanout_flips() > direct_flips_before;
                if !exporter.pending_cpu_frame() {
                    head.pending_nonzero_pixel_bytes = 0;
                }
                (
                    report,
                    exported_nonzero,
                    worker_was_in_flight,
                    submitted_direct,
                )
            };
            if exported_nonzero {
                self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                self.heads[index].nonzero_exports =
                    self.heads[index].nonzero_exports.saturating_add(1);
            }
            if let Some(retire) = report.rendered_primary_plane_scanout_retire {
                self.observe_retire(index, retire);
            }
            self.observe_callbacks(index, report.page_flip_callbacks.clone());
            let worker_is_in_flight = self.exporter(output).is_some_and(
                crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::worker_in_flight,
            );
            let renderer_started = {
                let head = &mut self.heads[index];
                advance_live_production_renderer_content(
                    worker_was_in_flight,
                    worker_is_in_flight,
                    &mut head.pending_content,
                    &mut head.rendering_content,
                )?
            };
            if renderer_started && self.heads[index].output_frames.pending().is_some() {
                self.heads[index]
                    .output_frames
                    .mark_rendering()
                    .map_err(|error| {
                        format!("compositor display-list render transition failed: {error}")
                    })?;
            }
            if let Some(submit) = report.rendered_primary_plane_scanout_submit {
                self.heads[index].last_submit_report = Some(submit);
                use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status;
                match submit.status {
                    Status::SubmittedWaitingForPageFlip => {
                        let identity = runtime
                            .submitted_rendered_frame_correlation(output)
                            .and_then(|correlation| correlation.native)
                            .ok_or("accepted native submission has no captured native identity")?;
                        let native_frame = LiveProductionNativeFrameId::from_raw(identity.frame());
                        if identity != self.native_frame_identity(index, output, native_frame) {
                            return Err(
                                "accepted native submission belongs to another owner/head/target"
                                    .into(),
                            );
                        }
                        let head = &mut self.heads[index];
                        let selected_rendering = head
                            .rendering_content
                            .is_some_and(|content| content.frame() == native_frame);
                        let selected_pending = head
                            .pending_content
                            .is_some_and(|content| content.frame() == native_frame);
                        if selected_rendering == selected_pending {
                            return Err("accepted native submission must name exactly one retained content owner".into());
                        }
                        let content = if selected_rendering {
                            head.rendering_content.take()
                        } else {
                            head.pending_content.take()
                        };
                        // This is the head's pixel proof, not a measurement of
                        // this frame: a readback costs a whole framebuffer, so
                        // a renderer context takes a bounded number of them and
                        // then keeps the last. It answers "this head has put
                        // light on a screen", which is what startup readiness
                        // asks of it.
                        let content = content.map(|content| {
                            content.with_nonzero_rgb_pixels(
                                self.exporter(output).map_or(0, |exporter| {
                                    exporter.composition_nonzero_rgb_pixels()
                                }),
                            )
                        });
                        if selected_rendering
                            && self.heads[index].output_frames.rendering().is_some()
                        {
                            self.heads[index]
                                .output_frames
                                .promote_rendering_to_submitted()
                                .map_err(|error| {
                                    format!(
                                        "compositor display-list worker promotion failed: {error}"
                                    )
                                })?;
                        } else if !selected_rendering
                            && self.heads[index].output_frames.pending().is_some()
                        {
                            self.heads[index]
                                .output_frames
                                .mark_submitted()
                                .map_err(|error| {
                                    format!(
                                        "compositor display-list submit transition failed: {error}"
                                    )
                                })?;
                        }
                        trace_live_native_lifecycle("kms_submit_accepted");
                        self.submissions = self.submissions.saturating_add(1);
                        self.heads[index].submissions =
                            self.heads[index].submissions.saturating_add(1);
                        self.heads[index].submitted_at = Some(Instant::now());
                        self.heads[index].submitted_ust_usec = Some(Self::monotonic_ust_usec());
                        self.heads[index].submitted_checksum =
                            Some(self.heads[index].last_checksum);
                        self.heads[index].submitted_sequence = Some(self.heads[index].submissions);
                        self.heads[index].submitted_content = content;
                        self.heads[index].submitted_direct = submitted_direct;
                        self.observe_layout_witness_submit(index, submit);
                        // Settled only if the cursor actually rode: a
                        // combined commit the driver refused retries with the
                        // primary alone, and settling then would record a
                        // cursor the plane is not showing. The position stays
                        // pending instead, for a later commit.
                        if let Some((_, placement)) = cursor_ride {
                            if submit.cursor_dropped {
                                self.cursor_combined_drops =
                                    self.cursor_combined_drops.saturating_add(1);
                            } else {
                                self.settle_atomic_cursor(index, placement, true);
                            }
                        }
                        if matches!(
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
                        ) {
                            self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                            self.heads[index].nonzero_exports =
                                self.heads[index].nonzero_exports.saturating_add(1);
                        }
                        let output = self.heads[index].output.id;
                        let cycle =
                            u64::try_from(self.heads[index].submissions).unwrap_or(u64::MAX);
                        let frame = content.map_or(0, |content| content.frame().raw());
                        tracing::trace!(
                            "sophia_live_native_page_flip schema=1 status=submitted output={} submission={} content={:?} frame={}",
                            output.raw(),
                            cycle,
                            content,
                            frame,
                        );
                        tracing::trace!(
                            "sophia_live_native_head_page_flip schema=2 status=submitted output={} head={} submission={} content={:?} frame={}",
                            output.raw(),
                            self.heads[index].head.raw(),
                            cycle,
                            content,
                            frame,
                        );
                        if let Err(error) = self.production_page_flips.submit(output, cycle) {
                            self.vsync_overlap_rejections =
                                self.vsync_overlap_rejections.saturating_add(1);
                            tracing::error!(
                                "sophia_live_native_pacing schema=1 status=submit_rejected output={} submission={} error={error:?}",
                                output.raw(),
                                cycle,
                            );
                        }
                    }
                    Status::ScanoutExportPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                    }
                    Status::AlreadyInFlight | Status::CleanupPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                    }
                    status => {
                        let failed_content = if worker_was_in_flight {
                            self.heads[index].rendering_content.take()
                        } else {
                            self.heads[index].pending_content.take()
                        };
                        if worker_was_in_flight {
                            self.heads[index].output_frames.discard_rendering();
                        } else {
                            self.heads[index].output_frames.discard_pending();
                        }
                        self.submit_failures = self.submit_failures.saturating_add(1);
                        tracing::warn!(
                            "sophia_live_native_submit schema=1 status=failed output={} reason={status:?} content={failed_content:?} export={:?} scanout_buffer={:?} resources={:?} framebuffer={:?} submit={:?} commit={:?}",
                            self.heads[index].output.id.raw(),
                            submit.export,
                            submit.scanout_buffer,
                            submit.resources,
                            submit.framebuffer,
                            submit.submit,
                            submit.commit_submit,
                        );
                    }
                }
            }
            self.max_in_flight_ticks = self
                .max_in_flight_ticks
                .max(report.rendered_primary_plane_scanout_in_flight_ticks);
            self.observe_in_flight_depth();
            Ok(report)
        }
}
