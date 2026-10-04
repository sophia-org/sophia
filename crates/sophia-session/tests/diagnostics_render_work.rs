use sophia_session::diagnostics::reduced_record;

#[test]
fn render_work_measurements_survive_diagnostic_reduction() {
    let record = "sophia_live_render_work schema=1 uptime_msec=5000 cpu_raster_count=1 cpu_raster_reuse_count=29 capture_context_creations_count=2 capture_context_reuses_count=28 capture_surface_creations_count=30 capture_failures_count=0 composition_full_frames_count=3 composition_partial_frames_count=27 composition_repaint_pixels_count=200 composition_target_pixels_count=2000 pipeline_creations_count=5 snapshot_captures_count=30 import_cache_imports_count=30 import_cache_hits_count=40 capture_setup_elapsed_nsec=100 capture_copy_elapsed_nsec=200 capture_cleanup_elapsed_nsec=300 composition_elapsed_nsec=400 capture_setup_cpu_nsec=10 capture_copy_cpu_nsec=20 capture_cleanup_cpu_nsec=30 composition_cpu_nsec=40 timing_enabled=1 cpu_scene_elapsed_nsec=600 cpu_scene_cpu_nsec=60 transfer_captures_count=2 transfer_attempts_count=2 transfer_failures_count=0";
    assert_eq!(
        reduced_record(&format!("{record} path=/private title=secret payload=123")),
        Some(record.to_owned())
    );
}

#[test]
fn damage_reasons_and_geometry_cohort_are_numeric_only() {
    let fields = [
        "damage_full_no_table_count",
        "damage_full_disabled_count",
        "damage_full_unknown_age_count",
        "damage_full_no_history_count",
        "damage_full_beyond_history_count",
        "damage_full_damage_unavailable_count",
        "damage_full_plan_count",
        "damage_full_plan_unspecified_count",
        "damage_full_plan_capacity_count",
        "damage_full_plan_rect_limit_count",
        "damage_full_plan_coverage_count",
        "damage_stable_geometry_frames_count",
        "damage_stable_geometry_full_count",
        "damage_stable_geometry_partial_count",
        "damage_stable_geometry_repaint_pixels_count",
        "damage_stable_geometry_target_pixels_count",
    ];
    for key in fields {
        let record = format!("sophia_live_render_work schema=1 {key}=123");
        assert_eq!(reduced_record(&record), Some(record));
        for value in [
            "private",
            "-1",
            "+1",
            "340282366920938463463374607431768211456",
        ] {
            assert_eq!(
                reduced_record(&format!("sophia_live_render_work schema=1 {key}={value}")),
                Some("sophia_live_render_work schema=1".to_owned())
            );
        }
    }
}

#[test]
fn present_clock_query_and_completion_counters_keep_only_bounded_numbers() {
    let input = "sophia_present_clock_service schema=1 queries=3 completions=1 observations=9 observation_runtime_locks=4 completion_runtime_locks=2 completion_historical_samples=1 admission_errors=2 admission_fake_retries=1 admission_settled=1 idle_signal_failures=1 scrap_sample_fallbacks=1 title=secret other=9";
    assert_eq!(
        sophia_session::diagnostics::reduced_record(input).as_deref(),
        Some(
            "sophia_present_clock_service schema=1 queries=3 completions=1 observations=9 observation_runtime_locks=4 completion_runtime_locks=2 completion_historical_samples=1 admission_errors=2 admission_fake_retries=1 admission_settled=1 idle_signal_failures=1 scrap_sample_fallbacks=1"
        )
    );
    assert_eq!(
        sophia_session::diagnostics::reduced_record(
            "sophia_present_clock_service schema=1 queries=-1 completions=18446744073709551616 observations=no observation_runtime_locks=18446744073709551616 completion_runtime_locks=no completion_historical_samples=-1 admission_errors=-1 admission_fake_retries=no admission_settled=18446744073709551616 idle_signal_failures=bad scrap_sample_fallbacks=-1"
        )
        .as_deref(),
        Some("sophia_present_clock_service schema=1")
    );
}

#[test]
fn wire_admission_measurements_preserve_only_numeric_values() {
    for key in [
        "service_runtime_locks",
        "deadline_runtime_locks",
        "wire_prepared",
        "wire_published",
        "wire_owner_notifications",
        "wire_bound",
        "wire_hardware_bound",
        "wire_executions",
        "wire_execution_wait_usec",
        "wire_execution_wait_max_usec",
        "owner_passes",
        "owner_waits",
        "owner_ring_ready",
        "owner_fd_ready",
        "owner_wait_deadlines",
        "owner_immediate_items",
    ] {
        let valid = format!("sophia_present_clock_service schema=1 {key}=23");
        assert_eq!(
            reduced_record(&format!("{valid} window=secret")),
            Some(valid)
        );
        for value in ["-1", "secret", "18446744073709551616"] {
            assert_eq!(
                reduced_record(&format!(
                    "sophia_present_clock_service schema=1 {key}={value}"
                )),
                Some("sophia_present_clock_service schema=1".to_owned())
            );
        }
    }
}

#[test]
fn damage_attribution_keeps_every_named_counter_and_rejects_payloads() {
    let records = [
        (
            "sophia_live_damage_causes",
            vec![
                "new_output",
                "output_changed",
                "compositor",
                "order",
                "geometry",
                "sampling",
                "generation",
                "missing_identity",
                "no_matching_transition",
                "invalid_transition",
                "origin",
                "rect_limit",
                "precision_restricted",
                "coordinate_overflow",
                "terminal_identity",
                "history_limit",
                "rebased",
                "precise_surface",
                "preview_identity",
                "cursor",
            ],
        ),
        (
            "sophia_present_damage",
            vec![
                "absent",
                "explicit_full_rect",
                "explicit_regions",
                "effective_empty",
                "source_pixels",
                "rect_pixels",
                "rects",
            ],
        ),
    ];
    for (name, fields) in records {
        let mut fields: Vec<String> = fields.into_iter().map(str::to_owned).collect();
        if name == "sophia_live_damage_causes" {
            fields.extend(fields.clone().into_iter().map(|key| format!("full_{key}")));
        }
        for key in fields
            .into_iter()
            .chain(["observed_monotonic_usec".to_owned()])
        {
            let line = format!("{name} schema=1 {key}=42");
            assert_eq!(
                reduced_record(&format!("{line} surface=99 title=secret")),
                Some(line)
            );
            for invalid in ["-1", "secret", "18446744073709551616"] {
                assert_eq!(
                    reduced_record(&format!("{name} schema=1 {key}={invalid}")),
                    Some(format!("{name} schema=1"))
                );
            }
        }
    }
}
