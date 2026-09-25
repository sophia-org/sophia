use super::*;

impl LiveProductionVisualRuntime {
    pub fn drive_gpu_presentation(
        &mut self,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        let transaction = match self
            .present_scheduler
            .poll_gate(self.presentation_feedback.resources_mut(), Instant::now())?
        {
            LiveProductionPresentGate::Idle
            | LiveProductionPresentGate::SubmittedInFlight
            | LiveProductionPresentGate::WaitingAcquire => {
                return self.run_observation_tick();
            }
            LiveProductionPresentGate::Reject(transaction) => {
                self.reject_gpu_presentation(transaction);
                return self.run_observation_tick();
            }
            LiveProductionPresentGate::Ready(transaction) => transaction,
        };
        let Some(native_scanout) = native_scanout else {
            self.present_scheduler.pop_front();
            self.reject_gpu_presentation(transaction);
            return self.run_observation_tick();
        };
        native_scanout
            .set_translation_motion_active(self.translations.active(self.translation_time()));
        // The cadence an unpresentable candidate is paced to, read from the
        // head that paces everything else rather than from a constant.
        let paced_interval = crate::head_refresh_interval(
            native_scanout
                .cadence_head(self.outputs.primary_output())
                .map_or(0, |head| head.refresh_millihz),
        );
        let queued = self
            .present_scheduler
            .front()
            .ok_or("ready Present gate has no queued presentation")?;
        let queued_surface = queued.surface;
        let queued_candidate = queued.candidate.key();
        // A candidate that has already spent its first-visibility budget is
        // not parked again; it takes the ordinary rejection below, which is
        // what bounds the wait.
        let first_presentation = !self
            .production
            .committed_surfaces()
            .iter()
            .any(|state| state.surface == queued_surface)
            && !self.present_scheduler.front_first_visibility_exhausted();
        if !self.presentation_order.contains(&queued_surface) {
            if first_presentation {
                self.present_scheduler.defer_first_visibility(
                    queued_candidate,
                    crate::LiveProductionFirstVisibilityReason::OutsidePresentationOrder,
                    std::time::Instant::now(),
                );
                return self.run_observation_tick();
            }
            self.skip_unpresentable(
                transaction,
                queued_candidate,
                queued_surface,
                crate::LiveProductionFirstVisibilityReason::OutsidePresentationOrder,
                paced_interval,
            );
            return self.run_observation_tick();
        }

        tracing::trace!(
            transaction = transaction.raw(),
            candidate_surfaces = 1,
            committed_scene_surfaces = self.production.committed_surfaces().len(),
            queued_presents = self.present_scheduler.has_queued(),
            "preparing queued Present against committed scene"
        );
        let prepared = self
            .production
            .prepare_present_transaction(&queued.candidate);
        if !prepared.is_ready() {
            self.present_scheduler.pop_front();
            self.reject_gpu_presentation(transaction);
            return self.run_observation_tick();
        }
        let previous_geometry = self
            .production
            .committed_surfaces()
            .iter()
            .find(|state| state.surface == queued_surface)
            .map(|state| state.geometry);
        let candidate_geometry = prepared
            .candidate()
            .iter()
            .find(|state| state.surface == queued_surface)
            .map(|state| state.geometry)
            .ok_or("ready Present candidate lost its surface geometry")?;
        let logical_viewports = self.outputs.logical_viewports().collect::<Vec<_>>();
        let mut applicable_outputs = sophia_engine::applicable_output_retirement_set(
            &logical_viewports,
            previous_geometry,
            candidate_geometry,
        )?;
        // Only heads that can display this surface owe its retirement. A
        // scrolling column overlapping its neighbour does not belong there.
        applicable_outputs.retain(|output| {
            live_surface_routes_to_output(
                queued_surface,
                &self.surface_outputs,
                &self.geometry_routed_surfaces,
                *output,
            )
        });
        if applicable_outputs.is_empty() {
            if first_presentation {
                self.present_scheduler.defer_first_visibility(
                    queued_candidate,
                    crate::LiveProductionFirstVisibilityReason::NoApplicableOutput,
                    std::time::Instant::now(),
                );
                return self.run_observation_tick();
            }
            self.skip_unpresentable(
                transaction,
                queued_candidate,
                queued_surface,
                crate::LiveProductionFirstVisibilityReason::NoApplicableOutput,
                paced_interval,
            );
            return self.run_observation_tick();
        }
        // Present's MSC belongs to one physical CRTC clock. Bind it before
        // submission so a multi-output cohort cannot change clock domains
        // according to whichever callback happens to arrive last.
        let clock_output = applicable_outputs[0];
        for output in &applicable_outputs {
            let index = self
                .outputs
                .output_index(*output)
                .ok_or("Present cohort targets an unknown logical output")?;
            let in_flight = self
                .outputs
                .output_native_scanout_in_flight(index)
                .ok_or("Present output in-flight state was not registered")?;
            let cleanup_pending = self
                .outputs
                .output_native_cleanup_pending(index)
                .ok_or("Present output cleanup state was not registered")?;
            let pending_frame = native_scanout.pending_frame(*output);
            if in_flight || cleanup_pending || !native_scanout.frame_queue_ready(*output) {
                // The primary can no longer reach this: the reducer withholds
                // the presentation effect while it owes a native frame. A
                // cohort spanning outputs can still find a secondary busy, and
                // that one is drained by this same pass. Deferring silently is
                // what let the earlier deadlock hide, so it is said out loud --
                // at info, because the sessions that need this run at info and
                // a debug line there is the same silence by another name.
                // Coalesced on powers of two so a flood stays legible.
                self.present_output_busy_defers = self.present_output_busy_defers.saturating_add(1);
                if self.present_output_busy_defers.is_power_of_two() {
                    tracing::info!(
                        "sophia_live_present_defer schema=1 status=output_busy defers={} transaction={} output={} in_flight={in_flight} cleanup_pending={cleanup_pending} pending_frame={pending_frame}",
                        self.present_output_busy_defers,
                        transaction.raw(),
                        output.raw(),
                    );
                }
                return self.run_observation_tick();
            }
        }
        let mut mixed = self.presentation_feedback.resources().build_mixed_frame(
            transaction,
            None,
            queued.target,
            Some(queued.surface_clip),
            1.0,
        )?;
        let current_layer = mixed
            .layers
            .iter()
            .find_map(|layer| match layer {
                LiveOwnedMixedCompositionLayer::DmaBuf {
                    image_id,
                    frame,
                    placement,
                } => Some(LiveRetainedRendererImageLayer {
                    image_id: *image_id,
                    size: Size {
                        width: i32::try_from(frame.width).ok()?,
                        height: i32::try_from(frame.height).ok()?,
                    },
                    format: frame.format,
                    placement: *placement,
                }),
                LiveOwnedMixedCompositionLayer::Cpu { .. }
                | LiveOwnedMixedCompositionLayer::RendererImage { .. }
                | LiveOwnedMixedCompositionLayer::Solid { .. } => None,
            })
            .ok_or("ready Present frame did not retain its DMA-BUF")?;
        if !current_layer.has_unit_scale() {
            self.present_scheduler.pop_front();
            tracing::warn!(
                transaction = transaction.raw(),
                surface = queued_surface.index(),
                source_width = current_layer.size.width,
                source_height = current_layer.size.height,
                logical_width = current_layer
                    .placement
                    .clip
                    .unwrap_or(current_layer.placement.target)
                    .width,
                logical_height = current_layer
                    .placement
                    .clip
                    .unwrap_or(current_layer.placement.target)
                    .height,
                "rejected Present whose pixels do not match the logical X11 surface"
            );
            self.reject_gpu_presentation(transaction);
            return self.run_observation_tick();
        }
        let current_owned = mixed
            .layers
            .pop()
            .ok_or("ready Present frame lost its current DMA-BUF layer")?;
        let current_source = match current_owned {
            LiveOwnedMixedCompositionLayer::DmaBuf {
                image_id, frame, ..
            } => sophia_renderer_live::LiveOwnedHeadCompositionSource {
                surface: queued_surface,
                source: queued.candidate.target_buffer(),
                kind: sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::DmaBuf {
                    image_id,
                    frame,
                },
            },
            _ => return Err("ready Present source changed renderer kind".into()),
        };
        let output_display_lists = applicable_outputs
            .iter()
            .map(|output| {
                let logical_viewport = self
                    .outputs
                    .logical_viewport(*output)
                    .ok_or("Present targets an unknown logical output")?;
                Ok((
                    *output,
                    self.display_list_for_output(
                        *output,
                        logical_viewport,
                        prepared.candidate(),
                        &self.presentation_order,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        // Sources are read from the scene against the candidate this Present plans,
        // never from a set captured when it was enqueued. A Present held behind a
        // layout epoch plans a scene that moved on while it waited.
        let cpu_layers =
            scene.presentation_variant_layers(prepared.candidate(), &self.sampled_surface_order());
        let head_sources = live_present_head_composition_sources(
            queued_surface,
            current_source,
            prepared.candidate(),
            output_display_lists.iter().map(|(_, list)| list),
            &cpu_layers,
            |surface| {
                self.displayed_surfaces
                    .get(&surface)
                    .map(|displayed| &displayed.layer)
            },
            // A neighbouring surface still displayed by a direct flip has no
            // renderer image to compose from; its source is the client's
            // still-held planes, exactly as on the retained requeue path.
            |surface| {
                self.displayed_surfaces
                    .get(&surface)
                    .and_then(|displayed| self.displayed_direct_frame(displayed.layer.image_id))
            },
        )?;
        let mut output_head_frames = output_display_lists
            .into_iter()
            .map(|(output, output_display_list)| {
                Ok((
                    output,
                    self.compose_native_head_frames_from_sources(
                        native_scanout,
                        output,
                        prepared.candidate(),
                        output_display_list,
                        transaction.raw(),
                        &head_sources,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        // Old geometry can require repainting a head after the current pixels
        // have scrolled completely out of it. Such a frame captures no image
        // for this Present, so it cannot own its copy-completion retirement.
        // Keep the clearing repaint, but settle the invisible client as Skipped.
        if !live_present_head_frames_capture_image(&output_head_frames, current_layer.image_id) {
            if first_presentation {
                // The settled column can be onscreen while its animated
                // position is still outside the head. Keep its first buffer
                // and admission debt; other surfaces may run while it waits.
                self.present_scheduler.defer_first_visibility(
                    queued_candidate,
                    crate::LiveProductionFirstVisibilityReason::OutsideHeadFrames,
                    std::time::Instant::now(),
                );
                return self.run_observation_tick();
            }
            tracing::debug!(
                transaction = transaction.raw(),
                surface = queued_surface.index(),
                image = current_layer.image_id.raw(),
                "skipped Present absent from lowered physical head frames"
            );
            // The clearing repaint reaches the heads, so its chrome is observed
            // as the Present turn's view of the scene.
            self.record_focus_ring_observation(
                prepared.candidate(),
                LiveChromeObservationSource::Present,
                false,
            )?;
            native_scanout.queue_retained_output_head_composition_frames(output_head_frames)?;
            self.skip_unpresentable(
                transaction,
                queued_candidate,
                queued_surface,
                crate::LiveProductionFirstVisibilityReason::OutsideHeadFrames,
                paced_interval,
            );
            return self.run_observation_tick();
        }
        if self.present_scheduler.take_diagnose_first_mixed_export() {
            let (cpu_layers, dmabuf_layers) =
                head_sources
                    .iter()
                    .fold((0usize, 0usize), |(cpu, dmabuf), source| {
                        match source.kind {
                    sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::Cpu(_) => {
                        (cpu.saturating_add(1), dmabuf)
                    }
                    sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::DmaBuf { .. }
                    | sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::RendererImage {
                        ..
                    } => (cpu, dmabuf.saturating_add(1)),
                }
                    });
            let (diagnostic_output, diagnostic_frames) = output_head_frames
                .first_mut()
                .ok_or("head planner produced no diagnostic output")?;
            let diagnostic = diagnostic_frames
                .pop()
                .ok_or("head planner produced no diagnostic frame")?;
            let (status, detail) =
                native_scanout.diagnose_mixed_frame(*diagnostic_output, diagnostic.frame);
            native_scanout.rollback_renderer_image(current_layer.image_id)?;
            self.present_scheduler.pop_front();
            self.reject_gpu_presentation(transaction);
            let _ = self.presentation_feedback.disconnect();
            return Err(Box::new(crate::LiveNativeMixedDiagnosticComplete {
                status,
                detail,
                cpu_layers,
                dmabuf_layers,
                live_sources: self.presentation_feedback.resources().source_count(),
                live_fences: self.presentation_feedback.resources().fence_count(),
                live_transactions: self.presentation_feedback.resources().presentation_count(),
            }));
        }
        let output_count = self.outputs.output_count();
        // Chrome is observed only for frames that reach a head. A Present
        // deferred or rejected above composed nothing the screen will show, and
        // recording it would seed a pair of generations no scanout produced.
        self.record_focus_ring_observation(
            prepared.candidate(),
            LiveChromeObservationSource::Present,
            false,
        )?;
        let frames = native_scanout
            .queue_present_output_head_composition_frames(transaction, output_head_frames)?;
        self.present_scheduler.pop_front();
        self.present_scheduler.mark_rendering(
            LiveProductionSubmittedPresent::new(
                frames.clone(),
                clock_output,
                queued_candidate,
                transaction,
                queued_surface,
                prepared,
                current_layer,
            )
            .ok_or("Present rendering has no output cohort")?,
        );

        let production = &self.production;
        let surface_metadata = &self.surface_metadata;
        let outputs = &mut self.outputs;
        let mut adapter = crate::LiveProductionOutputRuntimeAdapter::new(
            output_count,
            |index, committed: &[CommittedSurfaceState]| -> Result<_, Box<dyn std::error::Error>> {
                let output_id = outputs
                    .output_id(index)
                    .ok_or("production output index was not registered")?;
                outputs.run_output(index, committed, |runtime| {
                    native_scanout.run_tick(
                        output_id,
                        runtime,
                        compositor_tick_input_for_committed(
                            committed,
                            surface_metadata,
                            0,
                            Vec::new(),
                            None,
                        ),
                    )
                })
            },
        );
        let reports = production.run_outputs(&mut adapter)?;
        use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status;
        let mut selected_report = None;
        for (index, report) in reports.into_iter().enumerate() {
            let output = self
                .outputs
                .output_id(index)
                .ok_or("Present tick report lost its logical output")?;
            if !frames.contains_key(&output) {
                continue;
            }
            if selected_report.is_none() {
                selected_report = Some(report.clone());
            }
            match report
                .rendered_primary_plane_scanout_submit
                .map(|submit| submit.status)
            {
                Some(Status::SubmittedWaitingForPageFlip) => {
                    native_scanout.discard_presentation_feedback(Some(output));
                    if let Some(submitted_transaction) =
                        self.present_scheduler.mark_output_submitted(output)?
                    {
                        self.presentation_feedback
                            .resources_mut()
                            .mark_submitted(submitted_transaction)?;
                    }
                    self.observe_software_present_frame_submitted(frames[&output])?;
                }
                Some(Status::ScanoutExportPending)
                | Some(Status::AlreadyInFlight | Status::CleanupPending)
                | None => {}
                Some(status) => {
                    return Err(format!(
                        "Present output cohort failed after admission: output={} status={status:?}",
                        output.raw()
                    )
                    .into());
                }
            }
        }
        selected_report.ok_or_else(|| "Present cohort produced no output tick report".into())
    }
}

impl LiveProductionVisualRuntime {
    /// Settle a Present no head can carry, paced to the head's refresh.
    ///
    /// An onscreen client is paced by retirement: its Present completes from a
    /// real page flip, so it redraws at the head's rate. A client whose window
    /// cannot reach a screen used to be settled in the same owner pass its
    /// Present arrived in, which paced it by nothing at all -- `glxgears`
    /// dragged offscreen ran at nine thousand frames a second, and every one of
    /// them cost owner-loop present handling, a protocol completion and its
    /// evidence. Parking the candidate withholds the Idle that frees its
    /// buffer, so the client blocks on its own back buffers exactly as a
    /// visible one does, and the completion is delivered one tick later.
    ///
    /// The verdict is not revisited at the tick. A surface that becomes
    /// visible again presents a newer buffer, which enters the queue runnable
    /// and is composed normally.
    ///
    /// A candidate that has already spent its first-visibility budget is not
    /// made to wait a second time; the budget exists to bound exactly this
    /// kind of wait.
    fn skip_unpresentable(
        &mut self,
        transaction: TransactionId,
        candidate: sophia_protocol::SurfaceTransactionKey,
        surface: SurfaceId,
        reason: crate::LiveProductionFirstVisibilityReason,
        interval: std::time::Duration,
    ) {
        if !self.present_scheduler.front_first_visibility_exhausted()
            && self
                .present_scheduler
                .defer_to_frame_tick(candidate, Instant::now(), interval)
        {
            // Coalesced on powers of two, like the busy-output defer above: a
            // paced client produces one of these per frame, and a line each
            // would be the evidence flood this pacing exists to stop.
            let paced = self.present_scheduler.paced_skips();
            if paced.is_power_of_two() {
                tracing::info!(
                    "sophia_live_present_pace schema=1 status=parked paced={paced} transaction={} surface={} reason={reason:?} interval_usec={}",
                    transaction.raw(),
                    surface.index(),
                    interval.as_micros(),
                );
            }
            for overflowed in self.present_scheduler.bound_frame_tick_parking(surface) {
                self.reject_gpu_presentation(overflowed);
            }
            return;
        }
        self.present_scheduler.pop_front();
        self.reject_gpu_presentation(transaction);
    }

    /// Idle the client buffer a successor flip has just replaced on `output`.
    ///
    /// Does nothing unless a direct frame is recorded as displayed there. The
    /// caller is any retirement on that output, because both a direct and a
    /// composed successor take the plane away from the displayed buffer, and
    /// only after that has happened is releasing it lawful.
    ///
    /// A Present the registry no longer knows -- torn down with its client, or
    /// already released along some other path -- is not an error here: the
    /// obligation this discharges is "do not release too early", and a buffer
    /// that is already gone cannot be released too early.
    /// Release every directly displayed buffer, because the session is ending.
    ///
    /// The successor that would normally retire one is not coming. Errors are
    /// swallowed deliberately: shutdown reports its own outcome, and a Present
    /// the registry has already forgotten is not a reason to fail a teardown.
    pub(super) fn release_displayed_direct_presents(&mut self) {
        let outputs = self
            .displayed_direct_presents
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for output in outputs {
            // Shutdown has no successor and nothing left to compose from a
            // snapshot, so there is nothing to promote.
            let _ = self.idle_superseded_direct_present(output, None);
        }
    }

    pub(super) fn idle_superseded_direct_present(
        &mut self,
        output: OutputId,
        native_scanout: Option<&mut crate::LiveProductionNativeScanout>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(transaction) = self.displayed_direct_presents.remove(&output) else {
            return Ok(());
        };
        // If a composed successor sampled this buffer, its compose captured a
        // compositor-owned snapshot under this id; promoting it now, before
        // the release, is what lets every later retained frame show these
        // pixels after the client has its buffer back. A direct successor
        // captured nothing -- the surface's newest content is that successor
        // itself -- so a promotion that finds nothing is ordinary.
        if let Some(native_scanout) = native_scanout {
            let image = crate::presentation::renderer_image_for_present(transaction);
            if let Err(error) = native_scanout.promote_renderer_image(image) {
                tracing::warn!(
                    transaction = transaction.raw(),
                    ?error,
                    "a superseded direct present could not promote its captured snapshot"
                );
            }
        }
        match self.presentation_feedback.idle_displayed(transaction) {
            Ok(outcome) => self.route_present_feedback(outcome),
            // The only way this can fail is that the registry no longer knows
            // the Present -- torn down with its client, or released along some
            // other path. Not an error: the obligation being discharged is
            // "do not release too early", and a buffer already gone cannot be
            // released too early.
            Err(crate::LivePresentFeedbackError::UnknownPresentation { .. }) => {
                tracing::debug!(
                    transaction = transaction.raw(),
                    output = output.raw(),
                    "directly displayed Present was released before its successor retired it"
                );
            }
        }
        Ok(())
    }
}

/// A copy Present requires a capture of its exact image in at least one
/// physical head frame. Geometry overlap before clipping or animation is not
/// proof that a renderer will consume the candidate pixels.
pub fn live_present_head_frames_capture_image(
    frames: &[(OutputId, Vec<LiveProductionHeadCompositionFrame>)],
    image: LiveRendererImageId,
) -> bool {
    frames.iter().flat_map(|(_, heads)| heads).any(|head| {
        head.frame.layers.iter().any(|layer| {
            matches!(layer,
            LiveOwnedMixedCompositionLayer::DmaBuf { image_id, .. } if *image_id == image)
        })
    })
}
