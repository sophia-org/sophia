impl LiveProductionVisualRuntime {
    pub fn run_cpu_production_cycle(
        &mut self,
        request: LiveProductionCycleRequest<'_>,
    ) -> Result<CpuCycleOutcome, Box<dyn std::error::Error>> {
        let LiveProductionCycleRequest {
            batch,
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            defer_frame,
            output_descriptors,
            mut native_scanout,
            wm_update,
            presentation_layout,
            geometry_routed_surfaces,
            chrome_surfaces,
            indicator_publication,
            staged_cpu_buffer_handles,
        } = request;
        let authority_envelope = batch;
        authority_envelope.validate()?;
        self.presentation_feedback
            .observe_authority_resource_registrations(authority_envelope)?;
        let batch = self.ready_surface_content_batch(authority_envelope)?;
        let _ = self.reject_superseded_surface_content()?;
        let mut cpu_progress = authority_batch_cpu_progress(&batch);
        let mut updates = authority_batch_cpu_buffer_updates(&batch);
        record_recent_cpu_buffer_updates(&mut self.recent_cpu_buffer_updates, &updates);
        write_cpu_buffer_residency(
            &mut self.cpu_buffer_residency,
            self.production.committed_surfaces(),
            &batch,
            self.surface_content_stream
                .deferred_items()
                .chain(self.released_surface_content.iter()),
            self.present_scheduler.retained_cpu_buffer_handles(),
            staged_cpu_buffer_handles,
            &self.recent_cpu_buffer_updates,
        );
        retain_relevant_cpu_buffer_updates(scene, &mut updates, &self.cpu_buffer_residency);
        let native_enabled = native_scanout.is_some();
        let focused_surface = self.chrome_focus(focused_surface, chrome_surfaces);
        let focus_changed = self.focused_surface != focused_surface;
        self.focused_surface = focused_surface;
        let presentation_layout_changed =
            self.apply_presentation_layout(presentation_layout, geometry_routed_surfaces);
        if let Some(native) = native_scanout.as_deref_mut() {
            native.set_translation_motion_active(self.translations.active(self.translation_time()));
        }
        let chrome_surfaces_changed = self.set_chrome_surfaces(chrome_surfaces);
        let indicator_publication_changed = self.set_indicator_publication(indicator_publication);
        let visual_projection_changed = presentation_layout_changed
            || chrome_surfaces_changed
            || focus_changed
            || indicator_publication_changed;
        let committed_projection_requires_gpu =
            live_production_committed_projection_requires_gpu_scanout(
                self.production.committed_surfaces(),
                &self.presentation_order,
            );
        // A retained CPU layer is a snapshot from before this authority batch is
        // committed. Let a CPU-only scene compose the current updates instead of
        // placing new chrome around stale client pixels. GPU-owned projections
        // still need the retained mixed path to preserve their image ownership.
        let retained_projection_queued = if live_production_retained_projection_admitted(
            visual_projection_changed,
            !updates.is_empty(),
            committed_projection_requires_gpu,
        ) {
            match native_scanout.as_deref_mut() {
                Some(native_scanout) => self.queue_retained_projection(scene, native_scanout)?,
                None => false,
            }
        } else {
            false
        };
        if visual_projection_changed {
            tracing::debug!(
                "sophia_live_retained_projection schema=2 status={} focus_changed={} layout_changed={} chrome_changed={}",
                if retained_projection_queued {
                    "queued"
                } else {
                    "unavailable"
                },
                focus_changed,
                presentation_layout_changed,
                chrome_surfaces_changed,
            );
        }
        let removed_surfaces = authority_batch_removed_surfaces(&batch);
        self.release_removed_presentations(&removed_surfaces, native_scanout.as_deref_mut())?;
        let rebased_groups = batch.groups;
        self.enqueue_software_presents(&rebased_groups)?;
        let software_present_frame_required = !self.software_presents_unframed.is_empty();
        for group in &rebased_groups {
            self.observe_surface_metadata(&group.transactions, &group.removed_surfaces);
        }
        self.displayed_surfaces
            .retain(|surface, _| !removed_surfaces.contains(surface));
        let preserve_gpu_scanout = live_production_should_preserve_gpu_output(
            native_scanout.is_some(),
            self.present_scheduler.has_in_flight(),
            retained_projection_queued,
            presentation_layout_changed,
            committed_projection_requires_gpu,
        );
        let defer_frame = if software_present_frame_required {
            false
        } else {
            reduce_live_production_frame_defer(
                defer_frame,
                visual_projection_changed,
                preserve_gpu_scanout,
            )
        };
        let native_scanout = if preserve_gpu_scanout || software_present_frame_required {
            None
        } else {
            native_scanout
        };
        let intakes = rebased_groups
            .iter()
            .map(|group| {
                AuthorityTransactionIntake::new(group.transaction, group.transactions.clone())
                    .with_surface_removals(group.removed_surfaces.clone())
            })
            .collect::<Vec<_>>();
        self.observe_content_ordered_resource_releases(authority_envelope);
        let head_plan_composition = output_composition::OutputCompositionSnapshot::capture(self);
        let ordinary_repaints_pending = &mut self.ordinary_repaints_pending;
        let (production, outputs) = (&mut self.production, &mut self.outputs);
        let output_count = outputs.output_count();
        let primary_output = outputs.primary_output();
        let event_count = authority_transaction_count_for_groups(&rebased_groups);
        let surface_metadata = self.surface_metadata.clone();
        let indicator_strip_cache = &self.indicator_strip_cache;
        let text_cache = &self.text_cache;
        let mut native_scanout = native_scanout;
        let create_native_frames = native_scanout.is_some();
        // Native head frames are never composed from the committed set while a
        // Present owns the scanout: the preservation rule above withheld the
        // scanout. The chrome invariant below leans on this, so it is stated.
        debug_assert!(!create_native_frames || !self.present_scheduler.has_in_flight());
        let primary_logical_target = std::cell::Cell::new(None);
        let primary_logical_target_ref = &primary_logical_target;
        let mut adapter = LiveProductionCpuCycleAdapter::new(
            scene,
            &self.presentation_order,
            &self.chrome_surfaces,
            updates,
            raised_surface,
            focused_surface,
            self.surface_chrome_style,
            cursor_presentation.composition_position(),
            defer_frame,
            create_native_frames,
            &self.cpu_buffer_residency,
            output_descriptors,
            move |cycle: u64,
                  committed: &[CommittedSurfaceState],
                  authority_commits: &[TransactionCommit],
                  native_frames: Option<Vec<LiveProductionComposedFrame>>,
                  cpu_layers: Vec<LiveCpuPresentationLayer>| {
                // A deferred cycle may service retained native work without
                // producing a new CPU frame; only an actual frame set initializes outputs.
                let initialize_native = native_frames.is_some();
                let mut output_adapter = crate::LiveProductionOutputRuntimeAdapter::new(
                    output_count,
                    |index,
                     snapshot: &[CommittedSurfaceState]|
                     -> Result<_, Box<dyn std::error::Error>> {
                        let output_id = outputs
                            .output_id(index)
                            .ok_or("production output index was not registered")?;
                        let logical_viewport = outputs
                            .logical_viewport(output_id)
                            .ok_or("production output logical viewport was not registered")?;
                        let needs_initialization = initialize_native
                            && native_scanout.is_some()
                            && !outputs.native_initialized(output_id);
                        let mut initialized_here = false;
                        let result = outputs.run_output(index, snapshot, |runtime| {
                            let input = compositor_tick_input_for_committed(
                                snapshot,
                                &surface_metadata,
                                event_count,
                                authority_commits.to_vec(),
                                wm_update.clone(),
                            );
                            Ok(match native_scanout.as_deref_mut() {
                                Some(native_scanout) => {
                                    let display_list = head_plan_composition.display_list(output_id, snapshot)?;
                                    let scene = sophia_engine::output_scene_snapshot_from_committed_in_view(
                                        output_id,
                                        cycle.max(1),
                                        logical_viewport,
                                        snapshot,
                                        display_list,
                                        None,
                                    )?;
                                    let targets = native_scanout.head_render_targets(output_id);
                                    let plans = sophia_engine::build_output_head_plans(
                                        &scene,
                                        &targets,
                                    )?;
                                    if plans.len() != targets.len() {
                                        return Err(
                                            "native head planner returned partial target coverage"
                                                .into(),
                                        );
                                    }
                                    for plan in &plans {
                                        trace_live_head_composition_plan(plan);
                                    }
                                    if initialize_native {
                                        let logical_target = plans
                                            .first()
                                            .map(|plan| plan.logical_content_checksum);
                                        if plans.iter().any(|plan| {
                                            Some(plan.logical_content_checksum) != logical_target
                                        }) {
                                            return Err(
                                                "native heads disagree on logical content checksum"
                                                    .into(),
                                            );
                                        }
                                        let prepared = plans
                                            .iter()
                                            .map(|plan| {
                                                Ok(crate::LiveProductionHeadCompositionFrame {
                                                    head: plan.head,
                                                    scene_generation: plan.scene_generation,
                                                    target_generation: plan.target_generation,
                                                    mapping: plan.mapping,
                                                    logical_content_checksum: plan
                                                        .logical_content_checksum,
                                                    frame: sophia_renderer_live::lower_cpu_head_composition_plan_with_caches(
                                                        plan,
                                                        &cpu_layers,
                                                        &mut indicator_strip_cache.borrow_mut(),
                                                        &mut text_cache.borrow_mut(),
                                                    )?,
                                                })
                                            })
                                            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>(
                                            )?;
                                        let queued_target = if needs_initialization {
                                            native_scanout.initialize_head_composition(
                                                output_id,
                                                runtime,
                                                prepared,
                                            )?;
                                            initialized_here = true;
                                            None
                                        } else {
                                            let frame = ordinary_repaint::admit(ordinary_repaints_pending, native_scanout, output_id, prepared)?;
                                            frame.and_then(|frame| logical_target.map(|checksum| {
                                                LiveProductionCpuTarget::new(frame, checksum)
                                            }))
                                        };
                                        if Some(output_id) == primary_output {
                                            primary_logical_target_ref.set(queued_target);
                                        }
                                    }
                                    if runtime.rendered_primary_plane_scanout_in_flight() {
                                        runtime.run_tick(input)?
                                    } else {
                                        native_scanout.run_tick(output_id, runtime, input)?
                                    }
                                }
                                None => runtime.run_tick(input)?,
                            })
                        });
                        if result.is_ok() && initialized_here {
                            outputs.mark_native_initialized(output_id)?;
                        }
                        result
                    },
                );
                (0..output_count)
                    .map(|index| output_adapter.run_output(index, committed))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .next()
                    .ok_or_else(|| "persistent backend runtime has no outputs".into())
            },
        );
        let report = production
            .run_cycle(&intakes, &mut adapter)
            .map_err(|error| {
                format!(
                    "production CPU cycle failed in phase {:?}: {}",
                    error.phase, error.source
                )
            })?;
        drop(adapter);
        cpu_progress.bind_primary_logical_target(primary_logical_target.get());
        if software_present_frame_required {
            if !report.submission.composed {
                return Err("software Present did not produce an immutable composed frame".into());
            }
            if native_enabled {
                self.frame_unframed_software_presents(scene, output_descriptors)?;
            } else if self.native_suspended {
                self.reject_software_presents();
            } else {
                self.settle_unframed_software_presents_without_native()?;
            }
        }
        if report.submission.composed {
            // Chrome follows what the heads show. While a Present owns the
            // scanout its candidate is on screen, not the set this turn's
            // intakes committed; observing the latter here and the former on
            // the Present turn framed one stale surface in two places on
            // alternate scanouts.
            let displayed = self.displayed_surface_view().to_vec();
            self.record_focus_ring_observation(
                &displayed,
                LiveChromeObservationSource::Production,
                false,
            )?;
        }
        // Native input advances only when retire_native_scanout_output
        // observes the corresponding accepted page flip.
        if !native_enabled && !self.native_suspended {
            self.publish_committed_input_layers();
        }
        Ok((report.submission, report.committed_surfaces, cpu_progress))
    }
}
