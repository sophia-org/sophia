if let (Some(runtime), Some(native_scanout)) = (runtime.as_mut(), native_scanout.as_mut())
            && native_scanout.output_topology_allows_frame_service()
        {
            if layout.pending.is_none() {
                runtime.release_layout_deferred_presentations();
            }
            // Deliberately outside the guard above. A first candidate is
            // parked while admission waits for it, and admission is what
            // makes a layout pending, so servicing parked candidates only
            // when nothing is pending cannot release the ones that matter.
            runtime.service_first_visibility_presentations(Instant::now());
            // A frame presented before its window mapped escaped admission
            // entirely, so waiting on visibility for it only delays the redraw
            // that can actually be admitted. Skip it as soon as the authority
            // has confirmed the map, and only once production still holds it:
            // a request dropped on a queue miss would strand the client the
            // way the unbounded wait used to.
            let skippable = layout.skippable_escaped_presents();
            for key in skippable {
                // Consumed once production has taken the candidate, settled or
                // not: it is out of the queue either way, and asking again for
                // a candidate that is gone would spin forever. A queue miss is
                // different -- intake may not have reached it -- so that record
                // stands.
                if runtime.skip_escaped_pre_admission(key).is_some() {
                    layout.consume_escaped_present(key);
                }
            }
            let service = match runtime.service_native(native_scanout, scene) {
                Ok(service) => Some(service),
                Err(error) => {
                    let Some(execution) = active_output_topology_preparation.as_mut() else {
                        return Err(error);
                    };
                    let transaction = execution.effect.transaction;
                    let failure = error.to_string();
                    let recovered = begin_output_topology_first_presentation_rollback(
                        &mut execution.phase,
                        transaction,
                        &failure,
                        |reason| native_scanout.request_output_topology_rollback(reason),
                        |transaction| {
                            wm_session
                                .as_mut()
                                .ok_or_else(|| {
                                    Box::<dyn std::error::Error>::from(
                                        "first-presentation rollback lost its WM owner",
                                    )
                                })?
                                .reject_output_topology_effect(
                                    transaction,
                                    sophia_engine::OutputTopologyTransactionFailure::FirstPresentation,
                                )
                        },
                    )?;
                    if !recovered {
                        return Err(error);
                    }
                    tracing::warn!(
                        "sophia_live_output_authority schema=2 status=rollback_started transaction={} reason=first_presentation_service error={error} published=false",
                        transaction.raw(),
                    );
                    None
                }
            };
            if let Some(service) = service {
                for retired in service.retired_software_presents {
                    record_native_software_present_retirement(&mut layout, retired);
                }
                record_discarded_presents(&service.discarded_presents);
                if let Some(retired) = service.retired_present {
                    let NativePresentRetirementObservation {
                        surface,
                        stable,
                        ust_usec: _,
                        msc: _,
                    } = record_native_present_retirement(
                        &mut layout,
                        runtime,
                        native_scanout,
                        retired,
                        &mut retired_present_surfaces,
                        &mut startup_surface_presentations,
                        &mut startup_readiness,
                    );
                    if stable_gpu_frame_proves_post_input_pixels(
                        input_proof_started_at.is_some(),
                        input_surface,
                        surface,
                        stable,
                    ) {
                        input_pixel_change = true;
                    }
                }
            }
            correlate_physical_input_page_flip(
                input_proof_started_at.is_some(),
                input_pixel_change,
                input_raw_ingress_msec,
                input_change_submission_baseline,
                input_change_frame_baseline,
                native_scanout,
                &mut input_presented_ust_usec,
                &mut input_submit_to_page_flip,
            );
            if let Some(head) = native_scanout.heads.first() {
                input_latency_samples.observe_page_flip(
                    head.presented_submissions,
                    head.presented_content
                        .map_or(0, |content| content.frame().raw()),
                    head.presented_submission_ust_usec,
                    head.presented_page_flip_ust_usec,
                );
            }
            metrics.record_runtime_surfaces(runtime.committed_surfaces().len());
            reconcile_initial_session_focus(InitialSessionFocusContext {
                runtime,
                focus: &mut focus,
                seat,
                wm_session_present: wm_session.is_some(),
                layout: &layout,
                session_controls: &mut session_controls,
                next_focus_control_transaction: &mut next_focus_control_transaction,
            })?;
            // Admission focus can become eligible on a page-flip retirement.
            // Reconcile it here so an idle client does not need to emit another
            // authority batch before it can receive focus.
            reconcile_pending_wm_focus!(runtime);
        }
        // Retirement may leave the native request idle before its service
        // deadline is armed. Deliver its feedback before an authority-only
        // batch can skip the remainder of this owner turn.
