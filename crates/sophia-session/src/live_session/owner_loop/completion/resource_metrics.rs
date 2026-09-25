{
    crate::session_println!(
        "sophia_live_native_resources schema=12 status=complete target_creations={} pipeline_creations={} frame_surface_creations={} cpu_target_creations={} dmabuf_target_creations={} composition_target_creations={} composition_target_reuses={} generation_replacements={} recovery_replacements={} snapshot_captures={} snapshot_promotions={} snapshot_rollbacks={} snapshot_evictions={} snapshot_live_entries={} snapshot_live_bytes={} import_cache_imports={} import_cache_hits={} import_cache_evictions={} import_cache_live_entries={} import_cache_descriptor_mismatches={} import_cache_capacity_rejections={} exact_nearest_draws={} sharp_downscale_draws={} sharp_upscale_draws={} linear_fallback_draws={} worker_requests={} worker_completions={} worker_failures={} worker_soft_stalls={} worker_hard_stalls={} worker_release_enqueue_failures={} frame_slot_acquisitions={} frame_slot_reuses={} frame_slot_deferrals={} frame_slot_stale_releases={} frame_slots_leased={} frame_slots_high_watermark={} max_in_flight_per_output={} pending_frame_supersessions={} frame_slot_partial_repaints={} frame_slot_full_repaints={} frame_slot_history_invalidations={} frame_slot_history_records={} max_worker_request_msec={} renderer_workers={} worker_result_misroutes={} worker_max_service_skew={} direct_scanout_attempts={} direct_scanout_flips={} direct_scanout_tests={} direct_scanout_test_rejections={} direct_scanout_refusals={} direct_scanout_unsupported={} direct_scanout_fallbacks={}",
        native_resources.target_creations,
        native_resources.pipeline_creations,
        native_resources.frame_surface_creations,
        native_resources.cpu_target_creations,
        native_resources.dmabuf_target_creations,
        native_resources.composition_target_creations,
        native_resources.composition_target_reuses,
        native_resources.generation_replacements,
        native_resources.recovery_replacements,
        native_resources.snapshot_captures,
        native_resources.snapshot_promotions,
        native_resources.snapshot_rollbacks,
        native_resources.snapshot_evictions,
        native_resources.snapshot_live_entries,
        native_resources.snapshot_live_bytes,
        native_resources.import_cache_imports,
        native_resources.import_cache_hits,
        native_resources.import_cache_evictions,
        native_resources.import_cache_live_entries,
        native_resources.import_cache_descriptor_mismatches,
        native_resources.import_cache_capacity_rejections,
        native_resources.exact_nearest_draws,
        native_resources.sharp_downscale_draws,
        native_resources.sharp_upscale_draws,
        native_resources.linear_fallback_draws,
        native_resources.worker_requests,
        native_resources.worker_completions,
        native_resources.worker_failures,
        native_resources.worker_soft_stalls,
        native_resources.worker_hard_stalls,
        native_resources.worker_release_enqueue_failures,
        native_resources.frame_slot_acquisitions,
        native_resources.frame_slot_reuses,
        native_resources.frame_slot_deferrals,
        native_resources.frame_slot_stale_releases,
        native_resources.frame_slots_leased,
        native_resources.frame_slots_high_watermark,
        native_totals.max_in_flight_per_output,
        native_totals.pending_frame_supersessions,
        native_resources.frame_slot_partial_repaints,
        native_resources.frame_slot_full_repaints,
        native_resources.frame_slot_history_invalidations,
        native_resources.frame_slot_history_records,
        native_resources.max_worker_request.as_millis(),
        native_resources.renderer_workers,
        native_resources.worker_result_misroutes,
        native_totals.max_service_skew,
        direct_scanout_totals.attempts,
        direct_scanout_totals.flips,
        direct_scanout_totals.tests,
        direct_scanout_totals.test_rejections,
        direct_scanout_totals.refusals,
        direct_scanout_totals.unsupported,
        direct_scanout_totals.fallbacks,
    );
    // Why frames were or were not eligible, so a run in which direct scanout
    // never fired can say which of "the path was off", "the scene was never
    // eligible", and "the proof is wrong" it was. Zeros in the counters above
    // cannot distinguish those, and that is the first question anyone asks of
    // a gate that measured nothing.
    {
        let heads = native_scanout.as_ref().map_or_else(
            Vec::new,
            sophia_backend_live::LiveProductionNativeScanout::direct_scanout_head_verdicts,
        );
        let totals = native_totals.verdicts;
        for (output, head, verdicts) in &heads {
            let mut record = format!(
                "sophia_live_direct_scanout_verdicts schema=2 status=head output={} head={}",
                output.raw(),
                head.raw(),
            );
            for (verdict, count) in
                std::iter::zip(sophia_engine::DirectScanoutVerdict::VERDICTS, verdicts)
            {
                record.push_str(&format!(" {}={count}", verdict.reduced_name()));
            }
            crate::session_println!("{record}");
        }
        let mut record =
            String::from("sophia_live_direct_scanout_verdicts schema=2 status=complete");
        for (verdict, count) in
            std::iter::zip(sophia_engine::DirectScanoutVerdict::VERDICTS, totals)
        {
            record.push_str(&format!(" {}={count}", verdict.reduced_name()));
        }
        crate::session_println!("{record}");
    }
    // What a frame cost, split by how it reached the plane. Direct scanout
    // skips a composition pass, so the offer-to-submit half is where a
    // difference has to show; the submit-to-flip half is measured beside it
    // because "the display engine does not care how the buffer got there" is
    // an assumption worth being able to check.
    //
    // Absent populations are omitted rather than reported as zero. A session
    // that never composed has nothing to compare against, and a zero would
    // read as free instead of as absent.
    if native_evidence.enabled() {
        let cost = &native_totals.cost;
        for (population, samples) in [("direct", &cost.direct), ("composed", &cost.composed)] {
            let offer = samples.offer_to_submit.summary();
            let flip = samples.submit_to_flip.summary();
            // Absent means absent: a population with no samples at all never
            // happened, and belongs in no record. But a population with only
            // one half *did* happen and was half-measured, which is a defect
            // in the measuring rather than a fact about the run -- so it is
            // reported with the empty half showing zero frames, where the
            // gate can name it. Requiring both halves here instead made the
            // whole record vanish, and the only symptom was the comparison
            // reporting no direct frames at all.
            if offer.is_none() && flip.is_none() {
                continue;
            }
            let offer = offer.unwrap_or(sophia_backend_live::DirectScanoutCostSummary {
                frames: 0,
                min: 0,
                p50: 0,
                p99: 0,
                max: 0,
                saturated: false,
            });
            let flip = flip.unwrap_or(sophia_backend_live::DirectScanoutCostSummary {
                frames: 0,
                min: 0,
                p50: 0,
                p99: 0,
                max: 0,
                saturated: false,
            });
            crate::session_println!(
                "sophia_live_direct_scanout_cost schema=1 population={population} frames={} offer_submit_us_min={} offer_submit_us_p50={} offer_submit_us_p99={} offer_submit_us_max={} submit_flip_frames={} submit_flip_us_min={} submit_flip_us_p50={} submit_flip_us_p99={} submit_flip_us_max={} saturated={}",
                offer.frames,
                offer.min,
                offer.p50,
                offer.p99,
                offer.max,
                flip.frames,
                flip.min,
                flip.p50,
                flip.p99,
                flip.max,
                offer.saturated || flip.saturated,
            );
        }
    }
    if native_evidence.enabled() {
        crate::session_println!(
            "sophia_live_page_flip_clock schema=1 status=complete source=kernel_monotonic timestamps={} fallbacks={} pending={}",
            native_totals.kernel_page_flip_timestamps,
            native_totals.kernel_page_flip_timestamp_missing,
            native_scanout.as_ref().map_or(0, LiveProductionNativeScanout::pending_kernel_page_flip_timestamps),
        );
    }
    // The sampled population, reported without a verdict. Whether it grew is
    // decided by the verifier from the samples themselves: an emitter that
    // graded its own health would be the only witness to its own failure.
    resource_sampler.report();
    // Schema 2 adds what the copy-on-write backing costs and bounds.
    //
    // `cpu_cow_splits` counts patches that had to copy because a presentation
    // still held the bytes. Near zero is the steady state; tracking the update
    // count means presentations outlive the updates that follow them, which is
    // real work rather than a defect but is not the work this path was
    // optimized for. `cpu_resident_buffers_peak` and `cpu_resident_bytes_peak`
    // are what "bounded" is a claim about: a registry that ends empty having
    // peaked at a thousand buffers reads identically to one that never held
    // more than three, and only the second is bounded.
    crate::session_println!(
        "sophia_live_rendering_efficiency schema=2 status=complete cpu_updates={} cpu_replacements={} cpu_patch_updates={} cpu_patch_rects={} cpu_payload_bytes={} exact_pixel_metric_frames={} damage_scoped_metric_frames={} composition_target_reuses={} cpu_cow_splits={} cpu_resident_buffers_peak={} cpu_resident_bytes_peak={}",
        cpu_buffer_updates,
        cpu_buffer_replacements,
        cpu_buffer_patch_updates,
        cpu_buffer_patch_rects,
        cpu_buffer_payload_bytes,
        scene.exact_pixel_metric_frames(),
        scene.damage_scoped_metric_frames(),
        native_resources.composition_target_reuses,
        scene.cpu_cow_splits(),
        scene.peak_resident_buffers(),
        scene.peak_resident_buffer_bytes(),
    );
    crate::session_println!(
        "{}",
        cpu_visual_progress.record(Instant::now(), startup_ready_msec.unwrap_or_default())
    );
    crate::session_println!(
        "sophia_live_session_scheduler schema=2 authority_batches={batches} cpu_compositions={cpu_compositions} coalesced_batches={coalesced_batches} cadence_deferred_batches={cadence_deferred_batches} cadence_repaints={cadence_repaints} frame_interval_usec={} merged_batches={merged_batches} max_merge_run={max_merge_run}",
        primary_frame_interval.as_micros(),
    );
    crate::session_println!(
        "sophia_live_owner_timing schema=2 status=complete max_child_reap_msec={} max_input_phase_msec={}",
        max_child_reap.as_millis(),
        max_input_phase.as_millis(),
    );
    if let Some(wm) = wm_session.as_ref() {
        crate::session_println!(
            "sophia_live_wm_transport schema=2 status=complete peak_depth={} pending={} rejected={} action_ordered={} action_coalesced=0 stale_responses={} max_queue_dwell_msec={} max_round_trip_msec={}",
            wm.request_peak_depth,
            wm.pending_request_count(),
            wm.request_rejections,
            wm.action_requests_ordered,
            wm.stale_responses,
            wm.max_queue_dwell.as_millis(),
            wm.max_request().as_millis(),
        );
    }
    crate::session_println!(
        "sophia_live_session_cursor schema=7 path={} plane={} moves_coalesced={} max_motion_to_submit_msec={} initialization_max_msec={} initialization_deferrals={} max_update_msec={} legacy_updates_primary_in_flight={} buttons_routed={} hardware_updates={} hidden_updates={} hardware_failures={} queued={} backend_coalesced={} rides={} cursor_only={} cursor_only_max_msec={} cursor_only_total_msec={} combined_drops={} fallbacks={} pending={}",
        match native_scanout.as_ref().map(|scanout| scanout.cursor_path) {
            Some(sophia_backend_live::HardwareCursorPath::AtomicPlane) => "atomic_plane",
            _ => "legacy_ioctl",
        },
        // What the card would accept, which is not what the session chose.
        // A run on the legacy ioctl over a card that offers a cursor plane
        // says so, and a run claiming the atomic path over a card that
        // refused one is a contradiction a reader can catch.
        match native_scanout
            .as_ref()
            .and_then(|scanout| scanout.cursor_plane_probe())
        {
            Some(sophia_backend_live::CursorPlaneProbe::Accepted) => "accepted",
            Some(sophia_backend_live::CursorPlaneProbe::Refused) => "refused",
            None => "unprobed",
        },
        cursor_moves_coalesced,
        cursor_max_motion_to_submit.max(
            native_totals.max_cursor_queue_delay
        ).as_millis(),
        native_totals.max_cursor_initialization.as_millis(),
        native_totals.cursor_initialization_deferrals,
        native_totals.max_cursor_update.as_millis(),
        native_totals.legacy_cursor_updates_primary_in_flight,
        physical_pointer_buttons_routed,
        native_totals.cursor_updates,
        native_totals.cursor_hidden_updates,
        native_totals.cursor_update_failures,
        native_totals.cursor_updates_queued,
        native_totals.cursor_updates_coalesced,
        native_totals.cursor_updates_ridden,
        native_totals.cursor_only_commits,
        native_totals.max_cursor_only_commit.as_millis(),
        native_totals.cursor_only_commit_total.as_millis(),
        native_totals.cursor_combined_drops,
        native_totals.cursor_legacy_fallbacks,
        native_scanout
            .as_ref()
            .map_or(0, |scanout| scanout.pending_atomic_cursor_count()),
    );
    crate::session_println!(
        "sophia_live_session_health schema=1 status=clean protocol_errors={} pending_wm={} pending_actions={} pending_input={} wm_degraded={}",
        protocol_error_count,
        usize::from(layout.pending.is_some())
            .saturating_add(usize::from(pending_wm_update.is_some()))
            .saturating_add(
                wm_session
                    .as_ref()
                    .map_or(0, LiveWmSession::pending_request_count),
        ),
        committed_session_actions.len(),
        input_delivery.pending.len(),
        wm_session.as_ref().is_some_and(|wm| wm.degraded),
    );
    if let Some(monitor) = output_topology_monitor.as_ref() {
        let stats = monitor.stats();
        crate::session_println!(
            "sophia_live_output_topology_monitor schema=1 source=kernel_and_udev status=complete observed={} coalesced={} delivered={}",
            stats.observed, stats.coalesced, stats.delivered,
        );
    }
    crate::session_println!(
        "sophia_live_output_topology_health schema=1 status=clean quarantined={}",
        output_topology_owner.input_quarantined(),
    );
    crate::session_println!(
        "sophia_live_layout_health schema=2 status=clean recovery_extents={} standing_targets={} constraint_relayout_pending={}",
        recovery_extent_count,
        standing_target_count,
        layout.constraint_relayout_required(),
    );
    // WmWorkspaceState rejects hidden configure/render commands before they
    // can enter an Engine transaction; a clean completion therefore proves
    // the invariant without retaining client identity.
    crate::session_println!(
        "sophia_live_layout_authority schema=1 status=clean hidden_surface_commands=0"
    );
    crate::session_println!(
        "sophia_live_session_protocol_errors schema=1 expected={} unexpected={}",
        expected_protocol_error_count, protocol_error_count,
    );
    crate::session_println!(
        "sophia_live_selection schema=1 status=complete owner_changes={} conversions={} content=redacted",
        selection_owner_changes, selection_conversions,
    );

    if let Some(runtime) = runtime.as_ref() {
        let diagnostics = runtime.diagnostics();
        crate::session_println!(
            "sophia_live_present_scheduler schema=2 status=complete surface_content_capacity={} pending_limit=1 in_flight_limit=1 pending_supersessions={} surface_content_supersessions={} scheduler_supersessions={} max_surface_content_deferred={} max_latest_deferred_per_surface={} max_pending_queued={} max_total_queued={} max_live_sources={} max_live_fences={} max_live_presentations={} present_rejections={} native_suspend_present_rejections={} shutdown_present_rejections={} other_present_rejections={} paced_skips={} max_frame_tick_parked={} frame_tick_overflows={}",
            sophia_engine::SURFACE_CONTENT_STREAM_CAPACITY,
            diagnostics.pending_supersessions,
            diagnostics.surface_content_supersessions,
            diagnostics.scheduler_supersessions,
            diagnostics.max_surface_content_deferred,
            diagnostics.max_latest_deferred_per_surface,
            diagnostics.max_pending_queued,
            diagnostics.max_total_queued,
            diagnostics.max_live_sources,
            diagnostics.max_live_fences,
            diagnostics.max_live_presentations,
            diagnostics.present_rejections,
            diagnostics.native_suspend_present_rejections,
            diagnostics.shutdown_present_rejections,
            diagnostics.other_present_rejections,
            diagnostics.paced_skips,
            diagnostics.max_frame_tick_parked,
            diagnostics.frame_tick_overflows,
        );
    }
}
