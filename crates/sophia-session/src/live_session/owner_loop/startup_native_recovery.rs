{
startup_native_recovery_attempted = true;
let current = native_scanout
    .as_mut()
    .ok_or("startup native recovery lost the active scanout")?;
let suspended = runtime
    .as_mut()
    .ok_or("startup native recovery lost the visual runtime")?
    .suspend_native_scanout(current, &outputs, Duration::from_millis(100))?;
native_evidence.observe_settlement(suspended.outcome.drained(), suspended.abandoned_scanouts);
*suspended_renderer_images = Some(capture_renderer_image_handoff(
    runtime
        .as_ref()
        .ok_or("startup native recovery lost the visual runtime")?,
    current,
)?);
close_native_owner!("startup_recovery", RetirementMode::from_suspend(suspended.outcome));
if !native_recovery_allowed!() { continue; }
        native_owner_retirement::finish_before_replacement(runtime.as_ref(), native_retirement)?;
let replacement = LiveProductionNativeScanout::new_with_seat_mirroring_mapping_and_cursor(
    &seat_controller
        .as_ref()
        .ok_or("startup native recovery lost the seat controller")?
        .device_opener(),
    mirror_grouping,
    initial_head_mapping,
    config.cursor_resolution.asset.clone(),
)?;
*native_scanout = Some(replacement);
native_retirement.admit(native_scanout.as_ref().expect("just adopted"))?;
let replacement = native_scanout.as_mut().expect("just adopted");
if replacement.outputs() != outputs {
    schedule_output_topology_rebuild!("startup_recovery", false);
    startup_topology_recovery_pending = true;
    close_native_owner!("replacement_mismatch");
    tracing::warn!(
        "sophia_live_session_startup schema=3 status=recovery_deferred reason=output_topology_changed"
    );
} else {
    let runtime = runtime
        .as_mut()
        .ok_or("startup native recovery lost the visual runtime")?;
    let restored_renderer_images = resume_native_scanout_from_scene(
        runtime,
        replacement,
        &outputs,
        scene,
        suspended_renderer_images,
    )?;
    native_evidence.open("startup_recovery");
    let _ = reduce_session_startup(
        &mut startup_readiness,
        SessionStartupEvent::NativeRecovered,
    );
    crate::session_println!(
        "sophia_live_session_startup schema=3 status=recovered attempt=1 reason={} outcome={} drained={} abandoned_scanouts={}",
        recovery_reason.reduced_name(),
        suspended.outcome.reduced_name(),
        suspended.outcome.drained(),
        suspended.abandoned_scanouts,
    );
    crate::session_println!(
        "sophia_live_renderer_handoff schema=1 status=restored images={restored_renderer_images} source=startup_recovery"
    );
    std::io::stdout().flush()?;
}
retired_present_surfaces.clear();
startup_surface_presentations.clear();
startup_content_ready = false;
native_presentation_admitted = false;
startup_required_submissions = None;
input_content_surface = None;
startup_outputs_ready_reported = false;
}
