{
        let focused_client_ready = focus
            .focused_surface(seat)
            .is_some_and(|surface| applied_client_focus == Some(surface));
        let focused_content_ready = focus
            .focused_surface(seat)
            .is_some_and(|surface| input_content_surface == Some(surface));
        let focused_gpu_presented = focus
            .focused_surface(seat)
            .is_some_and(|surface| startup_surface_presentations.visual_detail(surface));
        // The same barrier startup uses: one flip strictly after the focused
        // surface first had visual detail, on every head it occupies. Without
        // native scanout there is no head to wait for.
        let focused_content_reached_scanout = native_scanout.as_ref().is_none_or(|native| {
            startup_required_submissions
                .as_ref()
                .and_then(|required| startup_output_evidence(native, Some(required)))
                .is_some_and(|outputs| all_startup_outputs_presented(&outputs))
        });
        let cpu_baseline_presented = current_cpu_frame_is_presented(
            scene.last_report().map(|report| report.nonzero_pixel_bytes),
            focused_content_reached_scanout,
            native_scanout.as_ref().map(|native| {
                native.heads.iter().map(|head| CpuScanoutHeadEvidence {
                    submissions: head.submissions,
                    presented_submissions: head.presented_submissions,
                })
            }),
        );
        let input_baseline_presented =
            input_baseline_is_presented(focused_gpu_presented, cpu_baseline_presented);
        let input_start_stable = if config.surface_resize_requested() {
            resize_proof_complete
        } else if config.expect_physical_text.is_some() {
            layout.pending.is_none()
                && wm_session
                    .as_ref()
                    .is_none_or(|wm_session| wm_session.committed > 0)
        } else {
            last_authority_update.elapsed() >= Duration::from_millis(config.input_quiet_msec)
                || wm_session.as_ref().is_some_and(|wm| {
                    wm.last_committed_at.is_some_and(|committed| {
                        committed.elapsed() >= Duration::from_millis(config.input_quiet_msec)
                    })
                })
        };
        if require_startup_focus
            && physical_input.is_some()
            && input_baseline_presented
            && focus_deadline_started_at.is_none()
        {
            focus_deadline_started_at = Some(Instant::now());
        }
        if injection_checksum.is_none()
            && config.input_proof_requested()
            && input_baseline_presented
            && input_start_stable
            && focused_client_ready
            && focused_content_ready
            && (!config.surface_resize_requested() || resize_proof_complete)
        {
            injection_checksum = scene.last_report().map(|report| report.checksum);
            input_change_submission_baseline = native_scanout
                .as_ref()
                .and_then(|native| native.heads.first())
                .map(|head| head.presented_submissions);
            input_change_frame_baseline = native_scanout
                .as_ref()
                .and_then(|native| native.heads.first())
                .map(|head| {
                    newest_head_composition_frame([
                        head.pending_content,
                        head.rendering_content,
                        head.submitted_content,
                        head.presented_content,
                    ]
                    .map(|content| content.map(|content| content.frame().raw())))
                });
            input_surface = focus.focused_surface(seat);
            input_surface_generation = input_surface.and_then(|surface| {
                runtime.as_ref().and_then(|runtime| {
                    scene.surface_buffer_generation(runtime.committed_surfaces(), surface)
                })
            });
            if let Some(text) = config.inject_text.as_deref() {
                let events = synthetic_text_input_events(text)?;
                let expected = events.len();
                let runtime = runtime
                    .as_ref()
                    .ok_or("synthetic routed input requires an initialized runtime")?;
                let report = route_input_events(
                    events,
                    &focus,
                    runtime.committed_surfaces(),
                    runtime.input_layers(),
                    &layout.client_routes,
                    input_sender,
                    &mut modifiers,
                    &mut key_repeat,
                    &key_repeat_map,
                    &mut client_keys,
                    &mut emergency_chord,
                    &mut virtual_terminal_chord,
                    &mut keyboard_coverage,
                    None,
                    &mut pointer,
                    false,
                    false,
                    false,
                    PhysicalInputRoutingMode::Full,
                    &mut input_delivery.next,
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    None,
                    None,
                    None,
                )?;
                if report.keys_routed != expected {
                    return Err(format!(
                        "synthetic input did not traverse committed Engine focus: expected={expected} routed={}",
                        report.keys_routed
                    )
                    .into());
                }
                input_delivery.events_expected = input_delivery
                    .events_expected
                    .saturating_add(report.deliveries.len());
                input_delivery.track(input_sender, report.deliveries.iter().copied(), false)?;
                input_delivery.wait_started_at = Some(Instant::now());
                input_delivery.source = Some("synthetic");
                input_batch_baseline = Some(metrics.batches);
                input_cpu_update_baseline = Some(metrics.cpu_buffer_updates);
                if !input_observations.key_routed {
                    crate::session_println!(
                        "sophia_live_session_input_pipeline schema=1 status=key_routed source=synthetic"
                    );
                    std::io::stdout().flush()?;
                    input_observations.key_routed = true;
                }
            } else {
                input_batch_baseline = Some(metrics.batches);
                input_cpu_update_baseline = Some(metrics.cpu_buffer_updates);
                physical_input_ready_at = Some(Instant::now());
                crate::session_println!(
                    "sophia_live_session_input schema=1 status=ready source=physical text={}",
                    config
                        .expect_physical_text
                        .as_deref()
                        .expect("checked above")
                );
                std::io::stdout().flush()?;
            }
        }
        if config.expect_physical_pointer
            && physical_input_completion_reported
            && input_pixel_change
            && pointer_phase_started_at.is_none()
            && pointer_cursor_checksum.is_none()
        {
            let runtime = runtime
                .as_ref()
                .ok_or("pointer proof became ready before the backend runtime")?;
            let focused_geometry = focus.focused_surface(seat).and_then(|surface| {
                runtime
                    .input_layers()
                    .iter()
                    .find(|layer| layer.surface == surface)
                    .map(|layer| layer.geometry)
            });
            pointer
                .arm_at_geometry_center(focused_geometry)
                .ok_or("pointer proof has no focused application surface to place the cursor")?;
            cursor_updates
                .dirty_since
                .get_or_insert_with(Instant::now);
            cursor_updates.dirty = true;
        }
        if application_surface_missing_since
            .is_some_and(|started| started.elapsed() >= Duration::from_millis(500))
        {
            return Err(
                "application proof surface disappeared before the required physical pointer selection"
                    .into(),
            );
        }
        // Once the proof surface is gone, the session owns no narrower
        // deadline and the global runtime budget intentionally stays out of
        // input proofs. A toolkit that destroyed its window but never exits
        // would otherwise leave the loop presenting blank frames forever;
        // bound that wait and fail closed with the exact exit-term states.
        if crate::input_proof::application_exit_overdue(
            config.application_proof_requested(),
            application_surface_gone_at.is_some(),
            primary_child_exited,
        ) && application_surface_gone_at.is_some_and(|gone_at| {
            gone_at.elapsed() >= Duration::from_millis(SESSION_COMPLETION_TIMEOUT_MSEC)
        }) {
            return Err(format!(
                "persistent live session application surface was removed but the client did not exit: presented_latency={} text_match={} completion_reported={} pointer_pixels={} buttons_routed={} child_exited={}",
                input_presented_latency.is_some(),
                input_text_match,
                physical_input_completion_reported,
                pointer_pixel_change,
                metrics.physical_pointer_buttons_routed,
                primary_child_exited,
            )
            .into());
        }
        let admission_pipeline_idle =
            layout.pending.is_none() && layout.next_unmanaged_surface().is_none();
        let stable_admission_surface = session_launches.admission().and_then(|admission| {
            admission.observed_surfaces().find(|surface| {
                runtime.as_ref().is_some_and(|runtime| {
                    runtime
                        .committed_surfaces()
                        .iter()
                        .any(|committed| committed.surface == *surface)
                        && (scene.surface_has_visual_detail(
                            runtime.committed_surfaces(),
                            *surface,
                        ) || retired_present_surfaces.contains_key(surface))
                })
            })
        });
        for (_, action, target) in &committed_session_actions {
            if *action == WmSessionAction::CloseFocused
                && let Some(surface) = target.or(applied_client_focus)
            {
                key_repeat.cancel_surface(surface);
                let cleared = clear_client_pressed_keys_state_only(
                    surface,
                    &mut client_keys,
                    &mut client_key_scratch,
                    &mut modifiers,
                    input_sender,
                    &mut routed_input_saturation,
                    &mut input_delivery.next,
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                )?;
                if cleared != 0 {
                    crate::session_println!(
                        "sophia_live_session_keys schema=1 status=cleared reason=close_surface surface={} count={cleared}",
                        surface.index(),
                    );
                }
            } else if *action == WmSessionAction::Logout
                && let Some(surface) = applied_client_focus
            {
                flush_client_keys!(surface, "logout");
            }
        }
        let withdrawn_admissions = layout.take_withdrawn_admissions();
        let session_requests = execute_committed_session_actions(
            SessionActionExecutionContext {
                config,
                xauthority,
                children: secondary_children,
                launches: session_launches,
                launch_admission_started_at: &mut launch_admission_started_at,
                admission_pipeline_idle,
                stable_admission_surface,
                withdrawn_admissions: &withdrawn_admissions,
                layout: &layout,
                focus: &focus,
                seat,
                session_controls: &mut session_controls,
            },
            &mut committed_session_actions,
        )?;
        if session_requests.open_launcher && let Some(shell)=metadata_shell.as_mut()
            && shell.queue_launcher(wm_session.as_ref().and_then(LiveWmSession::reference_output).unwrap_or(output.id))? {
            reference_capture.present(None);
            if let Some(runtime)=runtime.as_mut(){runtime.set_descriptor_overlay(None,scene,native_scanout.as_mut())?;}
        }
        if session_requests.reload_profile {
            profile_reload_requested = true;
        }
        if session_requests.restart_wm {
            wm_restart_requested = true;
        }
        if session_requests.logout {
            logout_requested = true;
            let discarded = input_delivery.pending.len();
            input_delivery.events_expected =
                input_delivery.events_expected.saturating_sub(discarded);
            input_delivery.pending.clear();
            input_delivery.wait_started_at = None;
            if discarded != 0 {
                crate::session_println!(
                    "sophia_live_session_input_pipeline schema=2 status=logout_discarded pending={discarded}"
                );
                std::io::stdout().flush()?;
            }
        }
        client_key_release_barrier
            .retain(|delivery| input_delivery.pending.contains_key(delivery));
        if session_logout_drain_decision(SessionLogoutDrainState {
            requested: logout_requested,
            pending_input_deliveries: input_delivery.pending.len(),
            pending_key_release_barriers: client_key_release_barrier.len(),
            pending_controls: session_controls.pending_len(),
            pending_wm_update: pending_wm_update.is_some(),
        }) == SessionLogoutDrainDecision::Complete
        {
            begin_session_quiescence!("logout_complete");
        }
        if input_presented_latency.is_none()
            && input_pixel_change
            && let Some(started) = input_proof_started_at
            && native_scanout.as_ref().is_none_or(|native| {
                input_change_submission_baseline.is_some_and(|baseline| {
                    native
                        .heads
                        .first()
                        .is_some_and(|head| head.presented_submissions > baseline)
                })
            })
        {
            input_presented_latency = Some(started.elapsed());
        }
        if let Some(runtime) = runtime.as_mut() {
            present_observer.drain_pending_feedback_with_allocation(
                runtime,
                &mut present_feedback,
                Some(window_allocation::LiveWindowAllocationView {
                    publisher: &window_allocation_publisher,
                    native: native_scanout.as_ref(),
                    layout: &layout,
                    outputs: &outputs,
                    topology_generation: output_topology_owner.publication_generation,
                }),
            )?;
            visual_progress.observe_committed(runtime.committed_surfaces());
        }
        if let Some(native) = native_scanout.as_ref() {
            visual_progress.observe_native(native);
        }
        if (config.exit_after_input_proof || config.inject_text.is_some())
            && input_presented_latency.is_some()
            && (config.expect_physical_text.is_none()
                || native_scanout.is_none()
                || input_presented_ust_usec.is_some())
            && input_text_match
            && (config.expect_physical_text.is_none() || physical_input_completion_reported)
            && (!config.expect_physical_pointer || pointer_pixel_change)
            && (!config.application_proof_requested() || primary_child_exited)
        {
            begin_session_quiescence!("input_proof_complete");
        }
}
