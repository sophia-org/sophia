// Production phase of one authority owner-loop cycle.
//
// Split from `authority.rs` so batch selection and per-batch observation stay
// separable from committing and composing. Included as a block fragment, so it
// shares the enclosing loop's locals exactly as it did inline.
{
                if runtime.is_none() {
                    *runtime = Some(
                        LiveProductionVisualRuntime::new(&outputs, native_scanout.as_mut())?
                            .with_m4_proof_controls(
                                config.m4_first_acquire_delay,
                                config.m4_reject_first_present,
                                config.m4_diagnose_first_mixed_export,
                            )
                            .with_surface_chrome_style(
                                wm_session
                                    .as_ref()
                                    .and_then(|wm| wm.surface_chrome_style())
                                    .unwrap_or(config.surface_chrome_style),
                            ),
                    );
                }
                let runtime = runtime
                    .as_mut()
                    .expect("persistent backend runtime was initialized above");
                runtime.set_transitions_enabled(window_transitions_enabled);
                if let Some(update) = wm_update.as_ref() {
                    match update.commit.outcome {
                        TransactionOutcome::Committed => {
                            let staged = runtime.commit_layout_epoch(update.commit.transaction);
                            if staged != 0 {
                                crate::session_println!(
                                    "sophia_live_resize_epoch schema=3 status=queue_committed epoch={} staged_presents={staged}",
                                    update.commit.transaction.raw(),
                                );
                            }
                        }
                        TransactionOutcome::TimedOut => {
                            let report = runtime.abort_layout_epoch(update.commit.transaction);
                            crate::session_println!(
                                "sophia_live_resize_epoch schema=3 status=queue_aborted epoch={} rejected_presents={} recovery_extents={}",
                                update.commit.transaction.raw(),
                                report.rejected.len(),
                                layout.recovery_extent_count(),
                            );
                        }
                        _ => {}
                    }
                }
                // Layout progress can promote staged WM chrome after the
                // earlier WM phase. Synchronize again at the production
                // boundary so the committed clearance and rendered chrome
                // always belong to the same transaction.
                let surface_chrome_style = wm_session
                    .as_ref()
                    .and_then(|wm| wm.surface_chrome_style())
                    .unwrap_or(config.surface_chrome_style);
                synchronize_runtime_surface_chrome_style(runtime, surface_chrome_style);
                if layout.pending.is_none() {
                    runtime.release_layout_deferred_presentations();
                }
                let raised_surface = layout
                    .top_client_positioned_surface()
                    .or_else(|| focus.focused_surface(seat));
                let focused_surface = focus.focused_surface(seat);
                let cursor_presentation = if native_scanout.is_some() {
                    LiveProductionCursorPresentation::HardwarePlane
                } else {
                    LiveProductionCursorPresentation::Software(pointer.position())
                };
                // One scene spans every output, so a layer belongs in it when
                // any output shows it. Asking only the primary output erased
                // whatever the policy had placed on the others.
                let mut presentation_layout = Vec::with_capacity(layout.layers.len());
                for layer in layout.layers.values() {
                    let visible = if layout.is_client_positioned(layer.surface) {
                        layout.client_positioned_visible(layer.surface, |owner| {
                            match wm_session.as_ref() {
                                Some(wm) => wm.surface_visible_on_any_output(owner, &outputs),
                                None => Ok(layout.mapped_surfaces.contains(&owner)),
                            }
                        })?
                    } else {
                        layout.managed_scene_visible(layer.surface, |surface| {
                            match wm_session.as_ref() {
                                Some(wm) => wm.surface_visible_on_any_output(surface, &outputs),
                                None => Ok(true),
                            }
                        })?
                    };
                    if visible {
                        presentation_layout.push(layer.clone());
                    }
                }
                presentation_layout.sort_by_key(|layer| layer.stack_rank);
                // A surface reaches an output by geometry when no policy assigned
                // it an owner: every client-positioned surface, and -- in a
                // session with no window manager -- every surface, since nothing
                // there assigns an owner and the Engine owns placement. Filtering
                // to client-positioned alone left a no-WM policy-managed window
                // routed to no output, so it never presented.
                let geometry_routed_surfaces = presentation_layout.iter()
                    .filter(|layer| layout.surface_is_geometry_routed(layer.surface))
                    .map(|layer| layer.surface).collect::<Vec<_>>();
                let chrome_surfaces = presentation_layout
                    .iter()
                    .filter(|layer| !layout.is_client_positioned(layer.surface))
                    .map(|layer| layer.surface)
                    .collect::<Vec<_>>();
                let native_owner_policy = production_cycle_native_owner_policy(
                    native_scanout.is_some(),
                    defer_cpu_frame,
                );
                let indicator_publication = wm_session
                    .as_ref()
                    .and_then(LiveWmSession::indicator_publication);
                visual_progress.observe_intake(&production_batch);
                if let Some(wm) = wm_session.as_mut() {
                    let application_capture_active = client_keys.pending_len() != 0
                        || application_route_leases.leases().next().is_some()
                        || pointer_focus_handoff.target().is_some()
                        || keyboard_focus_handoff.target().is_some();
                    let heads = policy_presentation_heads(
                        wm.public.as_ref().and_then(|public| public.reducer.presentation_publication().map(|(_, p)| p)),
                        native_scanout.as_ref(),
                    );
                    wm.install_committed_policy_presentation(runtime, scene, native_scanout.is_some(), application_capture_active, None, &heads)?;
                }
                let (_tick, report, committed_surfaces, composed, compose_elapsed, cpu_progress) =
                    if !production_batch.has_dma_buf_present_submissions()
                        && !runtime.released_surface_content_requires_gpu()
                    {
                        let (submission, committed_surfaces, cpu_progress) =
                            runtime.run_cpu_production_cycle(LiveProductionCycleRequest {
                                batch: &production_batch,
                                scene,
                                raised_surface,
                                focused_surface,
                                cursor_presentation,
                                defer_frame: defer_cpu_frame,
                                output_descriptors: &outputs,
                                native_scanout: native_scanout.as_mut().filter(|_| {
                                    native_owner_policy
                                        == ProductionCycleNativeOwnerPolicy::Available
                                }),
                                wm_update,
                                presentation_layout: &presentation_layout,
                                geometry_routed_surfaces: &geometry_routed_surfaces,
                                chrome_surfaces: &chrome_surfaces,
                                indicator_publication: indicator_publication.clone(),
                                staged_cpu_buffer_handles: &staged_cpu_buffer_handles,
                            })?;
                        (
                            submission.tick,
                            submission.composition,
                            committed_surfaces,
                            submission.composed,
                            submission.compose_elapsed,
                            cpu_progress,
                        )
                    } else {
                        let (submission, committed_surfaces, cpu_progress) =
                            runtime.run_gpu_production_cycle(LiveProductionCycleRequest {
                                batch: &production_batch,
                                scene,
                                raised_surface,
                                focused_surface,
                                cursor_presentation,
                                defer_frame: defer_cpu_frame,
                                output_descriptors: &outputs,
                                native_scanout: native_scanout.as_mut().filter(|_| {
                                    native_owner_policy
                                        == ProductionCycleNativeOwnerPolicy::Available
                                }),
                                wm_update,
                                presentation_layout: &presentation_layout,
                                geometry_routed_surfaces: &geometry_routed_surfaces,
                                chrome_surfaces: &chrome_surfaces,
                                indicator_publication,
                                staged_cpu_buffer_handles: &staged_cpu_buffer_handles,
                            })?;
                        (
                            submission.tick,
                            submission.composition,
                            committed_surfaces,
                            submission.composed,
                            submission.compose_elapsed,
                            cpu_progress,
                        )
                    };
                visual_progress.observe_committed(&committed_surfaces);
                if let Some(native) = native_scanout.as_ref() {
                    visual_progress.observe_native(native);
                }
                cpu_visual_progress.observe_production(&cpu_progress, Instant::now())?;
                if let Some(native_scanout) = native_scanout.as_ref() {
                    cpu_visual_progress.observe_native_scanout(native_scanout, Instant::now());
                }
                if let Some(ring) = runtime.take_focus_ring_observation() {
                    crate::session_println!(
                        "sophia_live_compositor_chrome schema=2 status=focus_ring_composed surface={} generation={} primitives={}",
                        ring.surface.index(),
                        ring.generation,
                        ring.primitives,
                    );
                }
                if let Some(chrome) = runtime.take_chrome_set_observation() {
                    crate::session_println!(
                        "sophia_live_compositor_chrome_set schema=1 status=composed generation={} eligible_surfaces={} frames={} focused_frames={} unfocused_frames={} focus_rings={} primitives={} clearance={}",
                        chrome.generation,
                        chrome.eligible_surfaces,
                        chrome.frames,
                        chrome.focused_frames,
                        chrome.unfocused_frames,
                        chrome.focus_rings,
                        chrome.primitives,
                        chrome.clearance,
                    );
                }
                for frame in runtime.take_chrome_frame_observations() {
                    crate::session_println!(
                        "sophia_live_compositor_chrome_frame schema=1 generation={} source={} in_flight={} surface={} x={} y={} width={} height={} focused={}",
                        frame.generation,
                        frame.source.as_str(),
                        frame.in_flight,
                        frame.surface.index(),
                        frame.geometry.x,
                        frame.geometry.y,
                        frame.geometry.width,
                        frame.geometry.height,
                        frame.focused,
                    );
                }
                // GPU preservation may defer a CPU turn the pacer admitted.
                // Its retained damage still needs a later cadence repaint.
                if cpu_cadence_eligible {
                    primary_frame_pacer.observe_production(Instant::now(), composed);
                }
                if composed {
                    metrics.max_compose = metrics.max_compose.max(compose_elapsed);
                    metrics.cpu_compositions = metrics.cpu_compositions.saturating_add(1);
                } else {
                    metrics.coalesced_batches = metrics.coalesced_batches.saturating_add(1);
                }
                if let (Some(surface), Some(before_surface)) =
                    (input_surface, input_surface_generation)
                    && scene
                        .surface_buffer_generation(&committed_surfaces, surface)
                        .is_some_and(|generation| generation != before_surface)
                {
                    input_surface_pixel_change = true;
                }
                if let Some(before_frame) = injection_checksum
                    && report.checksum != before_frame
                    && (config.expect_physical_text.is_none()
                        || physical_sequence_completed_at.is_some())
                {
                    input_pixel_change = true;
                }
                if let Some(before_frame) = pointer_checksum
                    && report.checksum != before_frame
                    && metrics.physical_pointer_routed > 0
                {
                    pointer_pixel_change = true;
                }
                metrics.backend_ticks = metrics.backend_ticks.saturating_add(1);
                // Counts every transaction this cycle committed, which is the
                // whole merged run rather than one batch's share.
                metrics.runtime_committed =
                    record_runtime_commits(metrics.runtime_committed, committed_transactions);
                metrics.record_runtime_surfaces(runtime.committed_surfaces().len());
                if let Some(native_scanout) = native_scanout.as_ref() {
                    for requirements in
                        runtime.reconcile_surface_raster_requirements(native_scanout)?
                    {
                        pending_surface_raster_requirements
                            .insert(requirements.surface, requirements);
                    }
                }
                while let Some((surface, requirements)) =
                    pending_surface_raster_requirements.pop_first()
                {
                    match raster_sender.try_route(requirements) {
                        Ok(()) => {}
                        Err(std::sync::mpsc::TrySendError::Full(requirements)) => {
                            pending_surface_raster_requirements.insert(surface, requirements);
                            break;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            return Err("X Authority raster route disconnected".into());
                        }
                    }
                }
                for surface in removed_surfaces {
                    // Proof records a surface going missing. Only a destroy is
                    // missing: a hidden surface can still come back, so this
                    // accounting stays here and is not shared with the hide
                    // path below.
                    if config.application_proof_requested()
                        && metrics.physical_pointer_buttons_routed == 0
                        && Some(surface) == input_surface
                    {
                        application_surface_missing_since.get_or_insert_with(Instant::now);
                    }
                    if config.application_proof_requested() && Some(surface) == input_surface {
                        application_surface_gone_at.get_or_insert_with(Instant::now);
                    }
                    release_surface_input_standing!(surface, "surface_removed");
                }
                if let Some(surface) = input_surface
                    && runtime
                        .committed_surfaces()
                        .iter()
                        .any(|committed| committed.surface == surface)
                {
                    application_surface_missing_since = None;
                    application_surface_gone_at = None;
                }
                reconcile_initial_session_focus(InitialSessionFocusContext {
                    runtime,
                    focus: &mut focus,
                    seat,
                    wm_session_present: wm_session.is_some(),
                    layout: &layout,
                    session_controls: &mut session_controls,
                    next_focus_control_transaction: &mut next_focus_control_transaction,
                })?;
                reconcile_pending_wm_focus!(runtime);
                if let Some(surface) = focus.focused_surface(seat) {
                    let cpu_visual_detail =
                        scene.surface_has_visual_detail(runtime.committed_surfaces(), surface);
                    if !startup_content_ready && cpu_visual_detail {
                        startup_content_ready = true;
                        let _ = reduce_session_startup(
                            &mut startup_readiness,
                            SessionStartupEvent::VisualDetail(surface),
                        );
                        let focused_geometry = runtime
                            .committed_surfaces()
                            .iter()
                            .find(|committed| committed.surface == surface)
                            .map(|committed| committed.geometry);
                        let output_bounds = wm_output_bounds(&outputs);
                        startup_required_submissions = native_scanout.as_ref().map(|native| {
                            native
                                .heads
                                .iter()
                                .map(|head| {
                                    let intersects = output_bounds
                                        .iter()
                                        .find(|(output, _)| *output == head.output.id)
                                        .is_some_and(|(_, bounds)| {
                                            focused_geometry.is_some_and(|geometry| {
                                                rects_intersect(geometry, *bounds)
                                            })
                                        });
                                    (
                                        head.head,
                                        StartupHeadRequirement {
                                            submission: startup_submission_requirement(
                                                head.submissions,
                                                head.presented_submissions,
                                                intersects,
                                            ),
                                            // The newest composition anywhere in
                                            // this head's pipeline right now was
                                            // planned before the focused surface
                                            // had content; presentation must
                                            // exceed it.
                                            content_frame: newest_head_composition_frame(
                                                [
                                                    head.pending_content,
                                                    head.rendering_content,
                                                    head.submitted_content,
                                                    head.presented_content,
                                                ]
                                                .map(|content| {
                                                    content
                                                        .map(|content| content.frame().raw())
                                                }),
                                            ),
                                        },
                                    )
                                })
                                .collect()
                        });
                        crate::session_println!(
                            "sophia_live_session_startup schema=1 status=content_ready source=cpu_visual_detail"
                        );
                        std::io::stdout().flush()?;
                    }
                }
}
