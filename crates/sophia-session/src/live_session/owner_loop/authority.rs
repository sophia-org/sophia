{
        let native_frame_service_request = match (runtime.as_ref(), native_scanout.as_ref()) {
            (Some(runtime), Some(native_scanout)) => {
                Some(runtime.native_output_service_request(native_scanout)?)
            }
            _ => None,
        };
        if runtime.as_ref().is_some_and(|r| r.frame_deadlines_pending()) || native_frame_service_request
            .as_ref()
            .is_some_and(native_frame_service_requires_owner_progress)
        {
            native_frame_service_deadline_armed = true;
            native_frame_idle_service_cycles = 0;
        }
        // No consecutive-cycle guard here, deliberately. The request-driven
        // path below keeps one, because that is what it was written for: a
        // continuously busy renderer must not starve X control and focus
        // handshakes. The paced path inherited the guard when it was added,
        // and on this path it is not fairness but a hard halving -- the pacer
        // only asks for a repaint when one is actually due, so refusing every
        // second request caps composition at half the head's refresh whenever
        // authority work is continuously available. Sustained pointer motion
        // makes it continuously available, which is how a 120Hz desktop
        // delivered thirty frames a second to a double-buffered client.
        let paced_repaint_preemption = runtime.is_some()
            && native_scanout.is_some()
            && (primary_frame_pacer.repaint_due(Instant::now()) || runtime.as_ref().is_some_and(|r| r.frame_deadline_due()));
        let native_frame_service_preemption = paced_repaint_preemption
            || native_frame_service_request
            .as_ref()
            .is_some_and(|request| {
                native_frame_service_should_preempt_authority(
                    request,
                    native_frame_service_preempted_previous_cycle,
                    control_is_pending(
                        session_controls.pending_len(),
                        explicit_pointer_grabs.pending(),
                    ),
                    native_frame_control_priority_cycles,
                    // Derived from the cadence rather than fixed. This was a
                    // hardcoded sixteen milliseconds, chosen as a sixty hertz
                    // watchdog before per-refresh pacing existed; on a 120Hz
                    // head that is two frame intervals, so it could not
                    // recover a tick the alternation had already lost.
                    native_frame_service_deadline_armed
                        && last_native_frame_service.elapsed() >= primary_frame_interval,
                )
            });
        let wm_only_cycle = initial_authority_batch.is_none()
            && pending_authority_batches.is_empty()
            && pending_wm_update.is_some();
        let output_topology_quarantined = active_output_topology_preparation.is_some();
        let authority_ingress = if session_quiescence
            .as_ref()
            .is_some_and(|quiescence| quiescence.frontend_authority_drained)
        {
            AuthorityIngressState::Disconnected
        } else {
            AuthorityIngressState::Open
        };
        let coordinator_transaction = pending_wm_update
            .as_ref()
            .map(|update| update.commit.transaction)
            .or_else(|| {
                runtime.as_ref().and_then(|runtime| {
                    runtime.released_surface_content_transaction()
                })
            });
        let authority_batch = match take_authority_work(
            &mut initial_authority_batch,
            &mut pending_authority_batches,
            coordinator_transaction,
            authority_ingress,
            output_topology_quarantined || native_frame_service_preemption,
        ) {
            Ok(batch) => Ok(batch),
            Err(AuthorityWorkWait::Service) => {
                // Preserve topology quarantine and alternate bounded frame
                // service with authority turns, including after frontend EOF.
                if (native_frame_service_preemption && !output_topology_quarantined)
                    || authority_ingress == AuthorityIngressState::Disconnected
                {
                    // A closed receiver cannot provide the usual bounded wait.
                    // Yield until the earliest frame/drain deadline instead of
                    // spinning while a renderer or policy response is pending.
                    let now = Instant::now();
                    let mut wait = primary_frame_pacer.cap_wait(now, Duration::from_millis(1));
                    if let Some(runtime) = runtime.as_ref() { wait = runtime.frame_deadline_cap_wait(now, wait); }
                    if let Some(quiescence) = session_quiescence.as_ref() {
                        wait = wait.min(quiescence.deadline.saturating_duration_since(now));
                    }
                    if !wait.is_zero() {
                        std::thread::sleep(wait);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            }
            Err(AuthorityWorkWait::Receive) => {
                let now = Instant::now();
                let maximum = authority_wait_timeout(
                    physical_input.is_some(),
                    cursor_updates.dirty,
                    session_controls.pending_len() != 0
                        || explicit_pointer_grabs.pending() != 0,
                );
                let maximum = runtime.as_ref().map_or(maximum, |r| r.frame_deadline_cap_wait(now, maximum));
                authority_receiver.recv_timeout(primary_frame_pacer.cap_wait(now, maximum))
            }
        };
        native_frame_service_preempted_previous_cycle = native_frame_service_preemption;
        // Nothing waiting, or the frame service already preempted: either way
        // the control path has no backlog to earn priority for.
        //
        // This must test what the preemption test above calls a pending
        // control. It counted session controls alone while the decision also
        // counted explicit pointer grabs, so a held grab with no session
        // control left the counter pinned at zero and the priority it gates
        // permanently unreachable -- the request path could then never
        // preempt for as long as the grab lasted.
        if control_priority_should_reset(
            session_controls.pending_len(),
            explicit_pointer_grabs.pending(),
            native_frame_service_preemption,
        ) {
            native_frame_control_priority_cycles = 0;
        } else {
            native_frame_control_priority_cycles =
                native_frame_control_priority_cycles.saturating_add(1);
        }
        match authority_batch {
            Ok(batch) => {
                let ingress = drain_queued_authority_batches(
                    authority_receiver,
                    &mut pending_authority_batches,
                    AUTHORITY_DRAIN_CAPACITY,
                    Duration::from_millis(2),
                    authority_ingress,
                );
                observe_authority_ingress(
                    ingress,
                    &mut session_quiescence,
                    Instant::now(),
                )?;
                // Commit everything the client has already produced in one
                // cycle. Composing per batch made a burst of draws cost one
                // frame each, so displayed content trailed the client by the
                // whole backlog.
                let merge_run_len = authority_merge_run_len(
                    &batch,
                    pending_authority_batches.iter(),
                    layout.authority_merge_quiescent(),
                    AUTHORITY_MERGE_RUN_LIMIT,
                );
                let mut authority_run = Vec::with_capacity(merge_run_len);
                authority_run.push(batch);
                for _ in 1..merge_run_len {
                    let Some(queued) = pending_authority_batches.pop_front() else {
                        break;
                    };
                    authority_run.push(queued);
                }
                let merge_run_len = authority_run.len();
                // A merged cycle already commits every available batch, so a
                // deferred frame is only correct when the bound, not the
                // queue, ended the run.
                let overload_defer_cpu_frame = runtime.is_some()
                    && merge_run_len >= AUTHORITY_MERGE_RUN_LIMIT
                    && pending_authority_batches
                        .front()
                        .is_some_and(authority_batch_may_follow_merge);
                for batch in &mut authority_run {
                    if !batch.raster_responses.is_empty()
                        && (batch.raster_responses.len() != batch.transactions.len()
                            || runtime.as_mut().is_none_or(|runtime| {
                                !batch.raster_responses.iter().copied().all(|identity| {
                                    runtime.accept_surface_raster_response(identity)
                                })
                            }))
                    {
                        tracing::warn!(
                            "sophia_live_surface_raster schema=1 status=stale_response transaction={} responses={} retained_buffer_updates={}",
                            batch.transaction.raw(),
                            batch.raster_responses.len(),
                            batch.cpu_buffer_updates.len(),
                        );
                        // Refusing the demand must not desynchronize buffer
                        // ownership. The authority has already published these
                        // handles and keeps naming them in later content sets,
                        // so dropping the updates alongside the transaction
                        // leaves a committed scene referencing buffers Engine
                        // never received. Decline the transaction; keep them.
                        batch.transactions.clear();
                        batch.raster_responses.clear();
                    }
                }
                if let Some(broker) = metadata_broker.as_mut() {
                    for batch in &authority_run {
                        for route in &batch.surface_routes {
                            if let Some(admission) = route.admission
                                && let Some(command) = broker.admit_surface(
                                    route.client,
                                    route.surface,
                                    admission.namespace.profile,
                                )?
                            {
                                    session_controls
                                        .enqueue(command, Instant::now())
                                        .map_err(|error| {
                                            format!(
                                                "failed to queue metadata disclosure rule: {error:?}"
                                                )
                                        })?;
                            }
                        }
                        for surface in &batch.removed_surfaces {
                            broker.retire_surface(*surface)?;
                        }
                    }
                }
                for batch in &authority_run {
                    for error in &batch.protocol_errors {
                        metrics.protocol_error_count = metrics.protocol_error_count.saturating_add(1);
                        first_protocol_error.get_or_insert(*error);
                        protocol_error_tally.observe(error);
                    }
                    metrics.expected_protocol_error_count = metrics.expected_protocol_error_count
                        .saturating_add(batch.expected_protocol_errors.len());
                    if batch.selection_owner_change {
                        selection_owner_changes = selection_owner_changes.saturating_add(1);
                        if config.firefox_proof_requested() {
                            crate::session_println!(
                                "sophia_firefox_m8 schema=1 status=selection_observed kind=owner_change count={selection_owner_changes} content=redacted"
                            );
                        }
                    }
                    if batch.selection_conversion {
                        selection_conversions = selection_conversions.saturating_add(1);
                        if config.firefox_proof_requested() {
                            crate::session_println!(
                                "sophia_firefox_m8 schema=1 status=selection_observed kind=conversion count={selection_conversions} content=redacted"
                            );
                        }
                    }
                    if config.firefox_proof_requested() {
                        for metadata in &batch.metadata {
                            if config.firefox_m10_rendering_proof
                                && !firefox_m10_rendering_page_ready
                                && metadata.property_name == "_NET_WM_NAME"
                                && metadata.byte_len == 249
                            {
                                firefox_m10_rendering_page_ready = true;
                                crate::session_println!(
                                    "sophia_firefox_rendering schema=1 status=page_ready title_bytes=249 content=redacted"
                                );
                            }
                            if config.firefox_m10_dialog_proof
                                && let Some(checkpoint) = firefox_m10_dialog_proof
                                    .observe(&metadata.property_name, metadata.byte_len)
                            {
                                crate::session_println!(
                                    "sophia_firefox_dialog schema=1 status=checkpoint checkpoint={checkpoint} title_bytes={} content=redacted",
                                    metadata.byte_len,
                                );
                            }
                            if config.firefox_m10_primary_proof
                                && metadata.property_name == "_NET_WM_NAME"
                            {
                                crate::session_println!(
                                    "sophia_firefox_primary schema=1 status=title_observed title_bytes={} content=redacted",
                                    metadata.byte_len
                                );
                                if let Some(checkpoint) = firefox_m10_primary_proof
                                    .observe(&metadata.property_name, metadata.byte_len)
                                {
                                    crate::session_println!(
                                        "sophia_firefox_primary schema=1 status=checkpoint checkpoint={checkpoint} title_bytes={} content=redacted",
                                        metadata.byte_len,
                                    );
                                }
                            }
                            if !firefox_m8_page_ready_reported
                                && metadata.property_name == "_NET_WM_NAME"
                                && metadata.byte_len == 36
                            {
                                firefox_m8_page_ready_reported = true;
                                crate::session_println!(
                                    "sophia_firefox_m8 schema=1 status=page_ready title_bytes=36 content=redacted"
                                );
                            }
                            if !firefox_m8_navigation_ready_reported
                                && firefox_m8_proof.navigation_ready(
                                    &metadata.property_name,
                                    metadata.byte_len,
                                )
                            {
                                firefox_m8_navigation_ready_reported = true;
                                crate::session_println!(
                                    "sophia_firefox_m8 schema=1 status=navigation_ready content=redacted"
                                );
                            }
                            if !firefox_m8_dialog_ready_reported
                                && firefox_m8_proof.dialog_ready(
                                    &metadata.property_name,
                                    metadata.byte_len,
                                )
                            {
                                firefox_m8_dialog_ready_reported = true;
                                crate::session_println!(
                                    "sophia_firefox_m8 schema=1 status=dialog_ready content=redacted"
                                );
                            }
                            for (stage, index, title_bytes) in
                                firefox_m8_proof.observe(&metadata.property_name, metadata.byte_len)
                            {
                                crate::session_println!(
                                    "sophia_firefox_m8 schema=1 status=stage_complete stage={stage} index={index} title_bytes={} content=redacted",
                                    title_bytes,
                                );
                            }
                            if (config.firefox_m10_proof || config.firefox_m10_lifecycle_proof)
                                && let Some((terminal, checkpoint)) = firefox_m10_kitty_proof
                                    .observe(&metadata.property_name, metadata.byte_len)
                            {
                                crate::session_println!(
                                    "sophia_firefox_m10 schema=1 status=kitty_checkpoint terminal={terminal} checkpoint={checkpoint} content=redacted"
                                );
                            }
                            if config.firefox_m10_selection_proof
                                && let Some(checkpoint) = firefox_m10_selection_kitty_proof
                                    .observe(&metadata.property_name, metadata.byte_len)
                            {
                                crate::session_println!(
                                    "sophia_firefox_selection schema=1 status=kitty_checkpoint checkpoint={checkpoint} content=redacted"
                                );
                            }
                        }
                    }
                }
                let has_engine_work = authority_run.iter().any(authority_batch_has_engine_work)
                    || runtime
                        .as_ref()
                        .is_some_and(LiveProductionVisualRuntime::has_released_surface_content);
                if !has_engine_work && pending_wm_update.is_none() {
                    for batch in &authority_run {
                        explicit_grab_queue.account(batch);
                    }
                    continue;
                }
                let mut removed_surfaces = Vec::new();
                for batch in &authority_run {
                    if !wm_only_cycle {
                        last_authority_update = Instant::now();
                        metrics.batches = metrics.batches.saturating_add(1);
                        metrics.transactions = metrics.transactions.saturating_add(
                            authority_transaction_count(&batch.transactions),
                        );
                        metrics.cpu_buffer_updates = metrics
                            .cpu_buffer_updates
                            .saturating_add(batch.cpu_buffer_updates.len());
                        metrics.cpu_buffer_replacements =
                            metrics.cpu_buffer_replacements.saturating_add(
                                batch
                                    .cpu_buffer_updates
                                    .iter()
                                    .filter(|update| update.is_replacement())
                                    .count(),
                            );
                        metrics.cpu_buffer_patch_updates =
                            metrics.cpu_buffer_patch_updates.saturating_add(
                                batch
                                    .cpu_buffer_updates
                                    .iter()
                                    .filter(|update| !update.is_replacement())
                                    .count(),
                            );
                        metrics.cpu_buffer_patch_rects =
                            metrics.cpu_buffer_patch_rects.saturating_add(
                                batch
                                    .cpu_buffer_updates
                                    .iter()
                                    .map(sophia_x_authority::XAuthorityCpuBufferUpdate::patch_rects)
                                    .sum::<usize>(),
                            );
                        metrics.cpu_buffer_payload_bytes =
                            metrics.cpu_buffer_payload_bytes.saturating_add(
                                batch
                                    .cpu_buffer_updates
                                    .iter()
                                    .map(sophia_x_authority::XAuthorityCpuBufferUpdate::payload_bytes)
                                    .sum::<usize>(),
                            );
                        metrics.dma_buf_registrations_observed = metrics
                            .dma_buf_registrations_observed
                            .saturating_add(batch.dma_buf_registrations.len());
                        metrics.fence_registrations_observed = metrics
                            .fence_registrations_observed
                            .saturating_add(batch.fence_registrations.len());
                        metrics.present_submissions_observed = metrics
                            .present_submissions_observed
                            .saturating_add(batch.present_submissions.len());
                        metrics.software_present_submissions_observed = metrics
                            .software_present_submissions_observed
                            .saturating_add(batch.software_present_submissions.len());
                    }
                    removed_surfaces.extend(batch.removed_surfaces.iter().copied());
                    for surface in &batch.removed_surfaces {
                        let seats = application_route_leases
                            .leases()
                            .filter(|lease| {
                                lease.target_surface == *surface
                                    && !matches!(
                                        lease.phase,
                                        sophia_engine::ApplicationRouteLeasePhase::Releasing { .. }
                                    )
                            })
                            .map(|lease| lease.identity.seat)
                            .collect::<Vec<_>>();
                        for lease_seat in seats {
                            request_application_route_lease_release(
                                &mut application_route_leases,
                                &layout.client_routes,
                                route_lease_release_sender,
                                lease_seat,
                                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                            )?;
                        }
                    }
                    if let Some(wm_session) = wm_session.as_mut() {
                        for surface in &batch.removed_surfaces {
                            require_wm_request_admission(
                                wm_session.enqueue_surface_removed(*surface)?,
                                "surface_removed",
                            )?;
                        }
                    }
                    // Eligibility before the batch, so what it ends can be
                    // named. Collected only for batches that can end it: a
                    // repaint cannot, and sweeping one would put an allocation
                    // on the steady-state path. An empty set allocates nothing
                    // and names nothing.
                    let eligible_before = if layout.batch_can_end_input_eligibility(batch) {
                        layout.input_eligible_surfaces()
                    } else {
                        BTreeSet::new()
                    };
                    let layout_observation = layout.observe_authority_batch(batch);
                    if layout_observation.client_route_invalid {
                        return Err("frontend surface-owner route changed without retirement".into());
                    }
                    if let Some(error) = layout_observation.admission_group_error {
                        return Err(error.into());
                    }
                    if layout_observation.admission_group_overflowed {
                        return Err("pre-admission authority-group capacity exceeded".into());
                    }
                    // Handled here, at the observation boundary, rather than
                    // after production: pending focus can advance during layout
                    // service, and a surface must not still be a focus
                    // candidate once it can no longer answer input.
                    //
                    // Nothing below tears down content, admission or buffers. A
                    // hidden surface keeps all of it and may become eligible
                    // again; only the standing it can no longer act on is given
                    // up.
                    for surface in layout.newly_ineligible_surfaces(&eligible_before) {
                        // A grabbed popup keeps its application route lease
                        // unless it is released here, and a lease with no
                        // eligible surface behind it still lets the
                        // application-owned guard consume pointer events.
                        // Released while the owner route is still retained.
                        let seats = application_route_leases
                            .leases()
                            .filter(|lease| {
                                lease.target_surface == surface
                                    && !matches!(
                                        lease.phase,
                                        sophia_engine::ApplicationRouteLeasePhase::Releasing { .. }
                                    )
                            })
                            .map(|lease| lease.identity.seat)
                            .collect::<Vec<_>>();
                        for lease_seat in seats {
                            request_application_route_lease_release(
                                &mut application_route_leases,
                                &layout.client_routes,
                                route_lease_release_sender,
                                lease_seat,
                                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                            )?;
                        }
                        let held_focus = focus.focused_surface(seat) == Some(surface);
                        release_surface_input_standing!(surface, "surface_hidden");
                        // Clearing the session's own record is not enough: the
                        // frontend still holds the X input focus, so a hidden
                        // window that had the keyboard keeps answering for it.
                        // ClearFocus reverts that to the root through the
                        // existing control protocol, carrying the transaction
                        // that ended eligibility so the acknowledgement
                        // correlates. Choosing the replacement stays with the
                        // window manager.
                        if !held_focus {
                            continue;
                        }
                        let Some(client) = layout.client_routes.client_for_surface(surface) else {
                            continue;
                        };
                        session_controls
                            .enqueue(
                                XAuthorityClientControlCommand {
                                    client,
                                    command: XAuthorityControlCommand::ClearFocus {
                                        transaction: batch.transaction,
                                        surface,
                                    },
                                },
                                Instant::now(),
                            )
                            .map_err(|error| {
                                format!("failed to queue hidden-surface focus clearing: {error:?}")
                            })?;
                        crate::session_println!(
                            "sophia_live_wm schema=1 status=hidden_focus_cleared transaction={}",
                            batch.transaction.raw(),
                        );
                    }
                    explicit_grab_queue.account(batch);
                    if layout_observation.output_reservations_changed
                        && let Some(wm_session) = wm_session.as_mut()
                    {
                        require_wm_request_admission(
                            wm_session.update_output_work_areas(&layout, &outputs, output)?,
                            "work_area",
                        )?;
                    }
                    if let Some(wm_session) = wm_session.as_mut() {
                        for surface in &layout_observation.withdrawn_surfaces {
                            require_wm_request_admission(
                                wm_session.enqueue_surface_removed(*surface)?,
                                "surface_withdrawn",
                            )?;
                        }
                    }
                    for surface in layout_observation.new_surfaces {
                        if layout.is_policy_managed(surface)
                            && layout.layout_facts(surface).is_some_and(|f| f.kind == sophia_protocol::LayoutNodeKind::Toplevel)
                            && let Some(admission) = layout.client_routes.admission_for_surface(surface)
                            && let Ok(mut origins) = launch_origins.lock() {
                            origins.observe_toplevel(surface, admission);
                        }
                        // A concurrently connecting application is not evidence for
                        // whichever registered launch happens to be awaiting a window.
                        let registered_process = secondary_children.iter()
                            .find(|child| child.matches_admission(session_launches))
                            .and_then(|child| child.process_identity);
                        let registered_owner = registered_process.is_some_and(|process| {
                            layout.client_routes.admission_for_surface(surface).is_some_and(|admission| {
                                launch_origins.lock().is_ok_and(|r| r.belongs_to_process(admission, process))
                            })
                        });
                        if registered_owner
                            && layout.layout_facts(surface).is_some_and(|f| f.kind == sophia_protocol::LayoutNodeKind::Toplevel)
                            && let Some(observation) = session_launches.observe_surface(surface) {
                            if let Some(destination) = observation.destination {
                                let applied = launch_origins.lock().is_ok_and(|mut origins|
                                    origins.register_catalog_origin(surface, observation.intent.transaction, destination));
                                crate::session_println!(
                                    "sophia_catalog_placement schema=1 status={} transaction={} surface={} surface_generation={} output={} output_generation={} wm_epoch={} token={}",
                                    if applied { "attributed" } else { "origin_unavailable" },
                                    observation.intent.transaction.raw(), surface.index(), surface.generation(),
                                    destination.output.raw(), destination.output_generation, destination.epoch, destination.token);
                            }
                            if let Some(classification) = observation.placement_classification
                                && let Some(wm_session) = wm_session.as_mut()
                            {
                                wm_session.register_launch_placement(surface, classification)?;
                                crate::session_println!(
                                    "sophia_session_launch_placement schema=1 status=issued transaction={} surface={} class={} source=registered_launch metadata=none",
                                    observation.intent.transaction.raw(),
                                    surface.index(),
                                    classification,
                                );
                            }
                            crate::session_println!(
                                "sophia_session_app schema=2 status=surface_observed source=action transaction={} surface={}",
                                observation.intent.transaction.raw(),
                                surface.index(),
                            );
                        }
                    }
                }
                service_layout_progress!("authority");
                let mut wm_update = pending_wm_update.take();
                if !resize_proof_complete
                    && let Some((transaction, surface, size)) = resize_proof
                    && layout.pending.is_none()
                    && layout.layout_epochs.committed_size(surface) == Some(size)
                {
                    if resize_proof_targets.len() > 1 {
                        crate::session_println!(
                            "sophia_live_resize schema=2 status=committed transaction={} surface={} width={} height={} configure_delivered=true pixels=true step={} total={}",
                            transaction.raw(),
                            surface.index(),
                            size.width,
                            size.height,
                            resize_proof_completed + 1,
                            resize_proof_targets.len(),
                        );
                    } else {
                        crate::session_println!(
                            "sophia_live_resize schema=1 status=committed transaction={} surface={} width={} height={} configure_delivered=true pixels=true",
                            transaction.raw(),
                            surface.index(),
                            size.width,
                            size.height,
                        );
                    }
                    // Only exact committed geometry and pixels advance the sequence.
                    resize_proof = None;
                    resize_proof_completed = resize_proof_completed.saturating_add(1);
                    resize_proof_complete = resize_proof_completed == resize_proof_targets.len();
                    if resize_proof_complete && resize_proof_targets.len() > 1 {
                        crate::session_println!(
                            "sophia_live_resize_storm schema=1 status=complete steps={} surface={} exact_pixels=true",
                            resize_proof_completed,
                            surface.index(),
                        );
                    }
                }
                if wm_update.is_none()
                    && let Some(result) =
                        layout.expire_pending(&mut session_controls)?
                    {
                        wm_update = Some(apply_wm_commit_result!(
                            result,
                            focus.focused_surface(seat)
                        ));
                    }
                if wm_update.is_none()
                    && layout.pending.is_none()
                    && let Some(surface) = layout.next_unmanaged_surface()
                    && let Some(wm_session) = wm_session.as_mut()
                {
                    require_wm_request_admission(
                        wm_session.enqueue_manage(surface, &layout, output)?,
                        "manage",
                    )?;
                }
                if resize_proof.is_none()
                    && !resize_proof_complete
                    && let Some(size) = resize_proof_targets.get(resize_proof_completed).copied()
                    && startup_ready_reported
                    && wm_session.as_ref().is_none_or(|wm| wm.committed > 0)
                    && layout.layers.len() >= if config.secondary_terminal { 2 } else { 1 }
                    && layout.pending.is_none()
                {
                    let surface = layout
                        .layers
                        .keys()
                        .next()
                        .copied()
                        .ok_or("surface resize proof has no target")?;
                    let transaction = TransactionId::from_raw(
                        2_000_000u64.saturating_add(
                            u64::try_from(resize_proof_completed).unwrap_or(u64::MAX),
                        ),
                    );
                    let mut layers = layout.layers.values().cloned().collect::<Vec<_>>();
                    let layer = layers
                        .iter_mut()
                        .find(|layer| layer.surface == surface)
                        .ok_or("surface resize proof lost its target")?;
                    layer.geometry.width = size.width;
                    layer.geometry.height = size.height;
                    let proposal = LiveWmProposal {
                        transaction,
                        layers,
                        requested_sizes: BTreeMap::from([(surface, size)]),
                        presentation_states: BTreeMap::new(),
                        configure_deliveries: 0,
                        focus: None,
                        timeout: Duration::from_secs(2),
                        update: WmTransactionUpdate {
                            commit: TransactionCommit {
                                transaction,
                                outcome: TransactionOutcome::Committed,
                                applied_surfaces: vec![surface],
                            },
                        },
                        moved_surfaces: 0,
                        source: None,
                        policy_settlement: None,
                    };
                    if let Some(result) =
                        layout.stage(proposal, &mut session_controls)?
                    {
                        wm_update = Some(apply_wm_commit_result!(
                            result,
                            focus.focused_surface(seat)
                        ));
                    }
                    resize_proof = Some((transaction, surface, size));
                    if resize_proof_targets.len() > 1 {
                        crate::session_println!(
                            "sophia_live_resize schema=2 status=requested transaction={} surface={} width={} height={} step={} total={}",
                            transaction.raw(),
                            surface.index(),
                            size.width,
                            size.height,
                            resize_proof_completed + 1,
                            resize_proof_targets.len(),
                        );
                    } else {
                        crate::session_println!(
                            "sophia_live_resize schema=1 status=requested transaction={} surface={} width={} height={}",
                            transaction.raw(),
                            surface.index(),
                            size.width,
                            size.height,
                        );
                    }
                }
                layout.write_pending_cpu_buffer_handles(&mut staged_cpu_buffer_handles);
                // One production envelope, built in arrival order. Group
                // bucketing happens per projection call, and the merge
                // selector already refused any run with a repeated
                // transaction identity, so concatenation cannot split a group.
                let mut production_batch = LiveProductionAuthorityBatch {
                    groups: Vec::new(),
                    dma_buf_registrations: Vec::new(),
                    fence_registrations: Vec::new(),
                    released_dma_bufs: Vec::new(),
                    released_fences: Vec::new(),
                };
                let mut committed_transactions = 0usize;
                for batch in &authority_run {
                    let (batch, released_admission_groups) = layout.projected_batch(batch);
                    let part =
                        production_authority_batch(&batch, &released_admission_groups, &layout)?;
                    committed_transactions = committed_transactions
                        .saturating_add(authority_transaction_count(&batch.transactions));
                    production_batch.groups.extend(part.groups);
                    production_batch
                        .dma_buf_registrations
                        .extend(part.dma_buf_registrations);
                    production_batch
                        .fence_registrations
                        .extend(part.fence_registrations);
                    production_batch.released_dma_bufs.extend(part.released_dma_bufs);
                    production_batch.released_fences.extend(part.released_fences);
                }
                // Primary cadence owns CPU-composed pixels only. A DMA-BUF
                // presentation has its own release/presentation timing and
                // must never arm a later synthetic CPU repaint.
                let cpu_cadence_eligible = !production_batch.has_dma_buf_present_submissions()
                    && runtime
                        .as_ref()
                        .is_none_or(|runtime| !runtime.released_surface_content_requires_gpu());
                let cadence_defer_cpu_frame = runtime.is_some()
                    && native_scanout.is_some()
                    && cpu_cadence_eligible
                    && primary_frame_pacer
                        .defer_production(Instant::now(), overload_defer_cpu_frame);
                let defer_cpu_frame = overload_defer_cpu_frame || cadence_defer_cpu_frame;
                if cadence_defer_cpu_frame {
                    metrics.cadence_deferred_batches =
                        metrics.cadence_deferred_batches.saturating_add(1);
                }
                if merge_run_len > 1 {
                    metrics.merged_batches = metrics
                        .merged_batches
                        .saturating_add(merge_run_len.saturating_sub(1));
                    metrics.max_merge_run = metrics.max_merge_run.max(merge_run_len);
                    tracing::debug!(
                        "sophia_live_authority_merge schema=1 status=committed batches={merge_run_len} transactions={committed_transactions} deferred_frame={defer_cpu_frame} queued={}",
                        pending_authority_batches.len(),
                    );
                }
                include!("authority_production.rs");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Frame-service and preemption turns are where the producer
                // actually gets ahead. Buffering here lets the next authority
                // turn see the whole backlog and merge it, instead of
                // discovering one batch at a time.
                if !output_topology_quarantined {
                    let ingress = drain_queued_authority_batches(
                        authority_receiver,
                        &mut pending_authority_batches,
                        AUTHORITY_DRAIN_CAPACITY,
                        Duration::from_millis(2),
                        authority_ingress,
                    );
                    observe_authority_ingress(
                        ingress,
                        &mut session_quiescence,
                        Instant::now(),
                    )?;
                    let expired = layout.expire_pending(&mut session_controls)?;
                    if let Some(result) = expired {
                        pending_wm_update = Some(apply_wm_commit_result!(
                            result,
                            focus.focused_surface(seat)
                        ));
                    }
                    if layout.pending.is_none()
                        && let Some(surface) = layout.next_unmanaged_surface()
                        && let Some(wm_session) = wm_session.as_mut()
                    {
                        require_wm_request_admission(
                            wm_session.enqueue_manage(surface, &layout, output)?,
                            "manage",
                        )?;
                    }
                }
                if let (Some(runtime), Some(native_scanout)) =
                    (runtime.as_mut(), native_scanout.as_mut())
                    && native_scanout.output_topology_allows_frame_service()
                {
                    // Releasing is what lets a topology quiescence wait finish:
                    // that wait blocks until no head holds pending content, and
                    // this is the step that drains it. Gating it on the owner's
                    // quarantine deadlocked the two against each other, because
                    // the quarantine is set for the whole wait, so the wait
                    // could only ever end by expiring. Once the backend really
                    // holds a preparation the original guard still applies:
                    // releasing then would present content composed for the
                    // outgoing topology.
                    if native_scanout.output_topology_preparation_phase().is_none()
                        && layout.pending.is_none()
                    {
                        runtime.release_layout_deferred_presentations();
                    }
                    let repaint_now = Instant::now();
                    if layout.pending.is_none()
                        && native_scanout.output_topology_preparation_phase().is_none()
                        && primary_frame_pacer.repaint_due(repaint_now)
                    {
                        let raised_surface = layout
                            .top_client_positioned_surface()
                            .or_else(|| focus.focused_surface(seat));
                        match runtime.run_ordinary_cpu_repaint(
                            scene,
                            raised_surface,
                            focus.focused_surface(seat),
                            LiveProductionCursorPresentation::HardwarePlane,
                            &outputs,
                            native_scanout,
                        )? {
                            None => {
                                // Keep the obligation without leaving cap_wait at
                                // zero while native ownership blocks publication.
                                primary_frame_pacer.observe_repaint_deferred(repaint_now);
                                tracing::trace!(
                                    "sophia_live_primary_pacer schema=1 status=deferred interval_usec={}",
                                    primary_frame_interval.as_micros(),
                                );
                            }
                            Some(repaint) => {
                                let mut repaint_progress =
                                    sophia_backend_live::LiveProductionCpuProgress::default();
                                repaint_progress
                                    .bind_primary_logical_target(repaint.primary_logical_target);
                                cpu_visual_progress
                                    .observe_production(&repaint_progress, repaint_now)?;
                                primary_frame_pacer.observe_repaint(repaint_now);
                                metrics.cadence_repaints =
                                    metrics.cadence_repaints.saturating_add(1);
                                metrics.cpu_compositions =
                                    metrics.cpu_compositions.saturating_add(1);
                                metrics.max_compose =
                                    metrics.max_compose.max(repaint.compose_elapsed);
                                tracing::trace!(
                                    "sophia_live_primary_pacer schema=1 status=repainted checksum={} interval_usec={}",
                                    repaint.composition.checksum,
                                    primary_frame_interval.as_micros(),
                                );
                            }
                        }
                    }
                    let service = runtime.service_native(native_scanout, scene)?;
                    last_native_frame_service = Instant::now();
                    let native_work_remains = native_frame_service_requires_owner_progress(
                        &runtime.native_output_service_request(native_scanout)?,
                    );
                    if native_work_remains || runtime.frame_deadlines_pending() {
                        native_frame_service_deadline_armed = true;
                        native_frame_idle_service_cycles = 0;
                    } else if native_frame_service_deadline_armed {
                        native_frame_idle_service_cycles =
                            native_frame_idle_service_cycles.saturating_add(1);
                        if native_frame_idle_service_cycles >= 4 {
                            native_frame_service_deadline_armed = false;
                            native_frame_idle_service_cycles = 0;
                        }
                    }
                    if let Some(retired) = service.retired_present {
                        let NativePresentRetirementObservation {
                            surface,
                            stable,
                            ust_usec: _,
                            msc: _,
                        } =
                            record_native_present_retirement(
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
                    for retired in service.retired_software_presents {
                        record_native_software_present_retirement(&mut layout, retired);
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
                    metrics.backend_ticks = metrics
                        .backend_ticks
                        .saturating_add(service.ticks.len());
                    metrics.runtime_surfaces =
                        u64::try_from(runtime.committed_surfaces().len()).unwrap_or(u64::MAX);
                    reconcile_initial_session_focus(InitialSessionFocusContext {
                        runtime,
                        focus: &mut focus,
                        seat,
                        wm_session_present: wm_session.is_some(),
                        layout: &layout,
                        session_controls: &mut session_controls,
                        next_focus_control_transaction: &mut next_focus_control_transaction,
                    })?;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => observe_authority_ingress(
                AuthorityIngressState::Disconnected,
                &mut session_quiescence,
                Instant::now(),
            )?,
        }

        if let (Some(runtime), Some(native)) = (runtime.as_ref(), native_scanout.as_ref()) {
            window_allocation_publisher.poll(Instant::now(), output_topology_owner.publication_generation,
                runtime, native, &layout, &outputs, frontend_service_sender)?;
        }

        if !physical_input_completion_reported
            && input_pixel_change
            && input_text_match
            && let (Some(text), Some(proof)) = (
                config.expect_physical_text.as_deref(),
                physical_text_proof.as_ref(),
            )
            && proof.is_complete()
        {
            crate::session_println!(
                "sophia_live_session_input schema=2 status=complete source=physical text={} expected_events={} matched_events={} pixel_change=true",
                text,
                proof.expected_events(),
                proof.matched_events(),
            );
            std::io::stdout().flush()?;
            physical_input_completion_reported = true;
        }

}
