use sophia_session::diagnostics::reduced_record;

#[test]
fn render_work_measurements_survive_diagnostic_reduction() {
    let record = "sophia_live_render_work schema=1 uptime_msec=5000 cpu_raster_count=1 cpu_raster_reuse_count=29 capture_context_creations_count=2 capture_context_reuses_count=28 capture_surface_creations_count=30 capture_failures_count=0 composition_full_frames_count=3 composition_partial_frames_count=27 composition_repaint_pixels_count=200 composition_target_pixels_count=2000 pipeline_creations_count=5 snapshot_captures_count=30 import_cache_imports_count=30 import_cache_hits_count=40 capture_setup_elapsed_nsec=100 capture_copy_elapsed_nsec=200 capture_cleanup_elapsed_nsec=300 composition_elapsed_nsec=400 capture_setup_cpu_nsec=10 capture_copy_cpu_nsec=20 capture_cleanup_cpu_nsec=30 composition_cpu_nsec=40 timing_enabled=1 cpu_scene_elapsed_nsec=600 cpu_scene_cpu_nsec=60 transfer_captures_count=2 transfer_attempts_count=2 transfer_failures_count=0";
    assert_eq!(
        reduced_record(&format!("{record} path=/private title=secret payload=123")),
        Some(record.to_owned())
    );
}
